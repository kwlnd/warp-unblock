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

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        AppState.init(this)
        if (intent?.action == ACTION_STOP) {
            shutdown(UiState())
            return START_NOT_STICKY
        }
        if (worker?.isAlive != true) {
            startForegroundCompat("Подключаю…")
            stopping = false
            worker = Thread { run() }.also { it.start() }
        }
        return START_NOT_STICKY
    }

    private fun run() {
        try {
            AppState.set(UiState(Phase.BUSY, "Подключаю…", "Готовлю ядро"))
            val core = Core(this)
            core.prepare()
            var account = WarpAccount.load(filesDir)
            if (account == null) {
                AppState.set(UiState(Phase.BUSY, "Подключаю…", "Регистрирую аккаунт WARP"))
                AppState.log("Регистрирую аккаунт WARP")
                account = WarpAccount.register(filesDir)
            }
            val strategies = Strategies.load(this)
            val strategy = strategies[AppState.strategyIndex.value.coerceIn(strategies.indices)]
            AppState.log("Профиль: ${strategy.name}")
            core.writeProfile(strategy, account)
            val process = core.start(false)
            mihomo = process
            AppState.set(UiState(Phase.BUSY, "Подключаю…", "Проверяю тоннель WARP"))
            val delay = core.waitDelay(Core.API_PORT, 40) { process.isAlive && !stopping }
            if (stopping) return
            if (delay == null) {
                error("WARP не ответил. Попробуйте другой профиль или автоподбор.\n" + core.logTail(8))
            }
            AppState.log("WARP отвечает, $delay мс")
            establishTun()
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
            .addDisallowedApplication(packageName)
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
        if (!service.TProxyStartService(config.absolutePath, fd.fd)) error("Не запустился tun2socks")
    }

    /** Проверка тоннеля раз в 30 секунд; ошибка проверки тоннель не рвёт. */
    private fun health(core: Core, process: Process) {
        var counter = 0
        while (!stopping && process.isAlive) {
            Thread.sleep(1000)
            if (++counter < 30) continue
            counter = 0
            val delay = core.waitDelay(Core.API_PORT, 12) { process.isAlive && !stopping }
            if (stopping) return
            AppState.set(
                if (delay != null) {
                    UiState(Phase.CONNECTED, "Обход работает", "WARP отвечает · $delay мс")
                } else {
                    UiState(Phase.CONNECTED, "Нет связи с WARP", "Проверка не прошла, повторю через 30 с")
                },
            )
        }
        if (!stopping) {
            AppState.log("Ядро остановилось")
            shutdown(UiState(Phase.ERROR, "Ядро остановилось", core.logTail(6)))
        }
    }

    private fun shutdown(result: UiState) {
        stopping = true
        runCatching { tproxy?.TProxyStopService() }
        tproxy = null
        runCatching { tun?.close() }
        tun = null
        mihomo?.destroyForcibly()
        mihomo = null
        AppState.set(result)
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onRevoke() = shutdown(UiState())

    override fun onDestroy() {
        stopping = true
        runCatching { tproxy?.TProxyStopService() }
        runCatching { tun?.close() }
        mihomo?.destroyForcibly()
        super.onDestroy()
    }

    private fun notification(text: String): Notification {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, "Подключение", NotificationManager.IMPORTANCE_LOW),
        )
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
            ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE,
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
