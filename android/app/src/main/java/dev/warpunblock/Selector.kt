package dev.warpunblock

import android.content.Context

/**
 * Автоподбор: для каждого профиля поднимает отдельный экземпляр ядра без VPN и ждёт ответа WARP.
 * Останавливается на первом рабочем и сохраняет выбор. Это проверка доступности, а не качества связи.
 */
object Selector {
    @Volatile private var cancelled = false
    private var thread: Thread? = null

    val running: Boolean get() = thread?.isAlive == true

    fun cancel() {
        cancelled = true
    }

    fun start(context: Context, best: Boolean = false) {
        AppState.init(context)
        if (running || AppState.ui.value.phase == Phase.CONNECTED || AppState.ui.value.phase == Phase.BUSY) return
        cancelled = false
        val app = context.applicationContext
        thread = Thread { run(app, best) }.also { it.start() }
    }

    /**
     * Проверка одного профиля отдельным экземпляром ядра на тестовых портах, без VPN.
     * Возвращает задержку WARP в мс или null. Годится и при живом подключении: основное
     * ядро слушает свои порты, а приложение исключено из VPN.
     */
    internal fun probe(core: Core, strategy: Strategy, account: WarpAccount, budget: Long, alive: () -> Boolean): Int? {
        core.writeProfile(strategy, account)
        val test = core.start(true)
        try {
            return core.waitDelay(Core.TEST_API_PORT, budget) { test.runningCompat() && alive() }
        } finally {
            test.stopCompat()
            val stopDeadline = System.nanoTime() + 250_000_000L
            while (test.runningCompat() && System.nanoTime() < stopDeadline) Thread.sleep(10)
            check(!test.runningCompat()) { "Не удалось остановить тестовое ядро" }
        }
    }

    private fun run(context: Context, best: Boolean) {
        try {
            val core = Core(context)
            core.prepare()
            var account = WarpAccount.load(context.filesDir)
            if (account == null) {
                account = Bypass.register(context) {
                    AppState.set(UiState(Phase.BUSY, "Подбор профиля", it, selecting = true))
                    AppState.log(it)
                }
            }
            val catalog = Strategies.load(context)
            val order = catalog.searchOrder(AppState.strategyIndex.value, catalog.endpointIndex(AppState.endpoint.value))
            val results = mutableMapOf<Int, Int>()
            // Режим «лучший»: у профиля достаточно одного рабочего адреса.
            val found = mutableMapOf<Int, String>()
            val budget = WarpProbe.pickBudget(ProfileStats.load(context).values)
            AppState.log("Подбор: ${catalog.size} профилей × ${catalog.endpoints.size} адресов, на ответ WARP %.1f с".format(budget / 1000.0))
            var failed = 0
            for ((attempt, pair) in order.withIndex()) {
                if (cancelled) break
                val (index, endpoint) = pair
                if (best && index in found) continue
                val strategy = catalog.resolve(index, endpoint)
                val address = catalog.endpoints[endpoint]
                val name = "${strategy.name} · $address"
                AppState.set(
                    UiState(
                        Phase.BUSY, "Подбор профиля",
                        "${attempt + 1}/${order.size} · $name",
                        Triple(attempt, order.size, failed), selecting = true,
                    ),
                )
                val delay = probe(core, strategy, account, budget) { !cancelled }
                if (cancelled) break
                ProfileStats.record(context, index, delay)
                // У профиля может быть несколько попыток на разных адресах: важна удачная.
                if (delay != null || index !in results) results[index] = delay ?: 0
                if (delay == null) failed++ else found[index] = address
                AppState.saveResults(AppState.results.value + results)
                if (delay != null && !best) {
                    AppState.selectPair(index, address)
                    AppState.log("Рабочая пара: $name ($delay мс)")
                    AppState.set(UiState(Phase.IDLE, "Профиль WARP подобран", "$name · $delay мс"))
                    return
                }
                AppState.log("${if (delay != null) "Прошёл" else "Не прошёл"}: $name")
            }
            if (best && !cancelled) {
                val index = ProfileStats.best(context, results.filterValues { it > 0 })
                if (index != null) {
                    AppState.selectPair(index, found.getValue(index))
                    AppState.set(UiState(Phase.IDLE, "Лучший профиль сохранён", "${catalog.profiles[index].name} · ${found[index]} · ${results[index]} мс"))
                    return
                }
            }
            AppState.set(
                if (cancelled) UiState() else UiState(
                    Phase.ERROR, "Ни один профиль не прошёл",
                    "Проверьте интернет и отключите другой VPN",
                ),
            )
        } catch (error: Throwable) {
            AppState.log("Ошибка подбора: ${error.message}")
            AppState.set(UiState(Phase.ERROR, "Подбор не удался", error.message ?: ""))
        }
    }
}
