package dev.warpunblock

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.VpnService
import android.os.ParcelFileDescriptor
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import hev.htproxy.TProxyService
import java.io.File

/**
 * VPN-сервис: поднимает ядро mihomo (локальный mixed-порт), создаёт TUN и пробрасывает его в ядро
 * через hev-socks5-tunnel. Само приложение исключено из VPN, чтобы WireGuard-трафик ядра не зациклился.
 */
class WarpVpnService : VpnService() {
    private var worker: Thread? = null
    @Volatile private var stopping = false
    private var mihomo: Process? = null
    private var tun: ParcelFileDescriptor? = null
    private var tproxy: TProxyService? = null
    private var account: WarpAccount? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        AppState.init(this)
        if (intent?.action == ACTION_STOP) {
            shutdown(UiState())
            return START_NOT_STICKY
        }
        if (Selector.running) { stopSelf(startId); return START_NOT_STICKY }
        if (intent == null && !alwaysOn() && !Preferences(this).autoConnect && !Preferences(this).boot) { stopSelf(startId); return START_NOT_STICKY }
        if (worker?.isAlive != true) {
            startForegroundCompat("Подключаю…")
            stopping = false
            worker = Thread { run() }.also { it.start() }
        }
        return if (alwaysOn() || Preferences(this).autoConnect || Preferences(this).boot) START_STICKY else START_NOT_STICKY
    }

    private fun alwaysOn() = android.os.Build.VERSION.SDK_INT >= 29 && isAlwaysOn

    private fun run() {
        try {
            AppState.set(UiState(Phase.BUSY, "Подключаю…", "Готовлю ядро"))
            val core = Core(this)
            core.prepare()
            var account = WarpAccount.load(filesDir)
            if (account == null) {
                account = Bypass.register(this) {
                    AppState.set(UiState(Phase.BUSY, "Подключаю…", it))
                    AppState.log(it)
                }
            }
            if (stopping) return
            this.account = account
            val catalog = Strategies.load(this)
            val strategy = catalog.resolve(AppState.strategyIndex.value, catalog.endpointIndex(AppState.endpoint.value))
            AppState.log("Профиль: ${strategy.name} · ${strategy.endpoint}:${strategy.port}")
            core.writeProfile(strategy, account)
            val process = core.start(false)
            mihomo = process
            AppState.set(UiState(Phase.BUSY, "Подключаю…", "Проверяю тоннель WARP"))
            val delay = core.waitDelay(Core.API_PORT, WarpProbe.CONNECT_BUDGET_MS) { process.runningCompat() && !stopping }
            if (stopping) return
            ProfileStats.record(this, AppState.strategyIndex.value, delay)
            if (delay == null) {
                error("WARP не ответил. Попробуйте другой профиль или автоподбор.\n" + core.logTail(8))
            }
            AppState.log("WARP отвечает, $delay мс")
            establishTun()
            AppState.delay.value = delay
            AppState.traffic.value = 0L to 0L
            AppState.since.value = System.currentTimeMillis()
            AppState.set(UiState(Phase.CONNECTED, "Обход работает", "WARP отвечает · $delay мс"))
            updateNotification("Обход работает")
            health(core, process)
        } catch (error: Throwable) {
            if (!stopping) {
                AppState.log("Ошибка: ${error.message}")
                shutdown(UiState(Phase.ERROR, "Не удалось подключиться", error.message ?: ""))
            }
        }
    }

    private fun establishTun() {
        val builder = Builder()
            .setSession("WARP Unblock")
            .setMtu(1500)
            .addAddress("198.18.0.1", 30)
            .addAddress("fdfe:dcba:9876::1", 126)
            .addRoute("0.0.0.0", 0)
            .addRoute("::", 0)
            .addDnsServer("1.1.1.1")
        val preferences = Preferences(this)
        val installed = preferences.apps.filter { pkg ->
            pkg != packageName && runCatching { packageManager.getApplicationInfo(pkg, 0) }.isSuccess
        }
        when (preferences.appMode) {
            "allow" -> {
                check(installed.isNotEmpty()) { "Выберите хотя бы одно установленное приложение для VPN" }
                installed.forEach { builder.addAllowedApplication(it) }
            }
            "exclude" -> {
                builder.addDisallowedApplication(packageName)
                installed.forEach { builder.addDisallowedApplication(it) }
            }
            else -> builder.addDisallowedApplication(packageName)
        }
        val fd = builder.establish() ?: error("Система не выдала VPN-интерфейс (разрешение отозвано?)")
        tun = fd
        val config = File(filesDir, "hev.yml")
        config.writeText(
            """
            tunnel:
              mtu: 1500
              ipv4: 198.18.0.1
              ipv6: 'fdfe:dcba:9876::1'
            socks5:
              port: ${Core.PROXY_PORT}
              address: 127.0.0.1
              udp: 'udp'
            misc:
              task-stack-size: 81920
              log-level: warn
            """.trimIndent() + "\n",
        )
        val service = TProxyService()
        tproxy = service
        service.TProxyStartService(config.absolutePath, fd.fd)
    }

    /**
     * Проверка тоннеля раз в 30 секунд; ошибка проверки тоннель не рвёт. Два промаха подряд —
     * и, если включено, поиск другого рабочего профиля с переподключением ядра (TUN остаётся).
     */
    private fun health(core: Core, started: Process) {
        var process = started
        var counter = 0
        var failures = 0
        while (!stopping && process.runningCompat()) {
            Thread.sleep(1000)
            runCatching {
                val stats = tproxy?.TProxyGetStats()
                if (stats != null && stats.size >= 4) AppState.traffic.value = stats[1] to stats[3]
            }
            if (++counter < 30) continue
            counter = 0
            val delay = core.waitDelay(Core.API_PORT, WarpProbe.CONNECT_BUDGET_MS) { process.runningCompat() && !stopping }
            if (stopping) return
            AppState.delay.value = delay
            if (delay != null) {
                failures = 0
                AppState.set(UiState(Phase.CONNECTED, "Обход работает", "WARP отвечает · $delay мс"))
                continue
            }
            failures++
            val auto = Preferences(this).autoSwitch
            if (failures >= 2 && auto) {
                failures = 0
                switchProfile(core)?.let { process = it }
                continue
            }
            // Один промах бывает от сети; второй проверяем уже через 10 с.
            counter = 20
            AppState.set(UiState(Phase.CONNECTED, "Нет связи с WARP", if (auto) "Повторю проверку через 10 с" else "Проверка не прошла, повторю через 10 с"))
        }
        if (!stopping) {
            AppState.log("Ядро остановилось")
            shutdown(UiState(Phase.ERROR, "Ядро остановилось", core.logTail(6)))
        }
    }

    /**
     * WARP перестал отвечать: проверяет тот же профиль на следующих адресах пула (сломанный —
     * последним), затем другие профили, и перезапускает основное ядро на первой рабочей паре.
     * Провайдер обычно режет конкретный адрес, а маскировка работает на соседнем. Возвращает ядро.
     */
    private fun switchProfile(core: Core): Process? {
        val account = this.account ?: return null
        val catalog = Strategies.load(this)
        val profile = AppState.strategyIndex.value.coerceIn(catalog.profiles.indices)
        val broken = catalog.endpointIndex(AppState.endpoint.value)
        AppState.log("WARP не отвечает: ${catalog.profiles[profile].name} · ${catalog.endpoints[broken]}. Ищу другой адрес")
        updateNotification("Ищу другой адрес…")
        val budget = WarpProbe.pickBudget(ProfileStats.load(this).values)
        val order = catalog.searchOrder(profile, broken + 1)
        for ((attempt, pair) in order.withIndex()) {
            if (stopping) return null
            val (index, endpoint) = pair
            val strategy = catalog.resolve(index, endpoint)
            val address = catalog.endpoints[endpoint]
            AppState.set(UiState(Phase.BUSY, "WARP пропал · ищу адрес", "${attempt + 1}/${order.size} · ${strategy.name} · $address"))
            val delay = runCatching { Selector.probe(core, strategy, account, budget) { !stopping } }.getOrNull()
            ProfileStats.record(this, index, delay)
            if (delay == null || stopping) continue
            AppState.selectPair(index, address)
            AppState.log("Переключаюсь: ${strategy.name} · $address ($delay мс)")
            core.writeProfile(strategy, account)
            mihomo?.let { old ->
                old.stopCompat()
                val deadline = System.nanoTime() + 2_000_000_000L
                while (old.runningCompat() && System.nanoTime() < deadline) Thread.sleep(20)
            }
            val fresh = runCatching { core.start(false) }.getOrElse {
                AppState.log("Ядро не перезапустилось: ${it.message}")
                return null
            }
            mihomo = fresh
            val ok = core.waitDelay(Core.API_PORT, WarpProbe.CONNECT_BUDGET_MS) { fresh.runningCompat() && !stopping }
            AppState.delay.value = ok
            AppState.set(
                if (ok != null) UiState(Phase.CONNECTED, "Обход работает", "WARP отвечает · $ok мс")
                else UiState(Phase.CONNECTED, "Нет связи с WARP", "Новый профиль не ответил, повторю проверку"),
            )
            updateNotification("Обход работает")
            return fresh
        }
        AppState.log("Ни один профиль не ответил; повторю позже")
        AppState.set(UiState(Phase.CONNECTED, "Нет связи с WARP", "Другие профили тоже не ответили, повторю позже"))
        updateNotification("Нет связи с WARP")
        return null
    }

    private fun shutdown(result: UiState) {
        stopping = true
        runCatching { tproxy?.TProxyStopService() }
        tproxy = null
        runCatching { tun?.close() }
        tun = null
        mihomo?.stopCompat()
        mihomo = null
        AppState.since.value = null
        AppState.delay.value = null
        AppState.set(result)
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onRevoke() = shutdown(UiState())

    override fun onDestroy() {
        stopping = true
        runCatching { tproxy?.TProxyStopService() }
        runCatching { tun?.close() }
        mihomo?.stopCompat()
        super.onDestroy()
    }

    private fun notification(text: String): Notification {
        val manager = getSystemService(NotificationManager::class.java)
        if (android.os.Build.VERSION.SDK_INT >= 26) {
            manager.createNotificationChannel(
                NotificationChannel(CHANNEL, "Подключение", NotificationManager.IMPORTANCE_LOW),
            )
        }
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val stop = PendingIntent.getService(
            this, 1, Intent(this, WarpVpnService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(android.R.drawable.ic_lock_lock)
            .setContentTitle("WARP Unblock")
            .setContentText(text)
            .setContentIntent(open)
            .addAction(0, "Отключить", stop)
            .setOngoing(true)
            .build()
    }

    private fun startForegroundCompat(text: String) {
        ServiceCompat.startForeground(
            this, NOTIFICATION_ID, notification(text),
            if (android.os.Build.VERSION.SDK_INT >= 34) ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE else 0,
        )
    }

    private fun updateNotification(text: String) {
        getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification(text))
    }

    companion object {
        const val ACTION_START = "dev.warpunblock.START"
        const val ACTION_STOP = "dev.warpunblock.STOP"
        private const val CHANNEL = "connection"
        private const val NOTIFICATION_ID = 1
    }
}
