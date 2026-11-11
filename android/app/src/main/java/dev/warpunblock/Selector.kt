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

    private fun run(context: Context, best: Boolean) {
        var process: Process? = null
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
            val strategies = Strategies.load(context)
            val preferred = AppState.strategyIndex.value.coerceIn(strategies.indices)
            val order = listOf(preferred) + strategies.indices.filter { it != preferred }
            val results = mutableMapOf<Int, Int>()
            val budget = WarpProbe.pickBudget(ProfileStats.load(context).values)
            AppState.log("Подбор: на ответ WARP %.1f с на профиль".format(budget / 1000.0))
            var failed = 0
            for ((attempt, index) in order.withIndex()) {
                if (cancelled) break
                val strategy = strategies[index]
                AppState.set(
                    UiState(
                        Phase.BUSY, "Подбор профиля",
                        "${attempt + 1}/${strategies.size} · ${strategy.name}",
                        Triple(attempt, strategies.size, failed), selecting = true,
                    ),
                )
                core.writeProfile(strategy, account)
                val test = core.start(true)
                process = test
                val delay = core.waitDelay(Core.TEST_API_PORT, budget) { test.runningCompat() && !cancelled }
                test.stopCompat()
                val stopDeadline = System.nanoTime() + 250_000_000L
                while (test.runningCompat() && System.nanoTime() < stopDeadline) Thread.sleep(10)
                check(!test.runningCompat()) { "Не удалось остановить тестовое ядро" }
                process = null
                if (cancelled) break
                ProfileStats.record(context, index, delay)
                results[index] = delay ?: 0
                if (delay == null) failed++
                AppState.saveResults(AppState.results.value + results)
                if (delay != null && !best) {
                    AppState.selectStrategy(index)
                    AppState.log("Рабочий профиль: ${strategy.name} ($delay мс)")
                    AppState.set(UiState(Phase.IDLE, "Профиль WARP подобран", "${strategy.name} · $delay мс"))
                    return
                }
                AppState.log("${if (delay != null) "Прошёл" else "Не прошёл"}: ${strategy.name}")
            }
            if (best && !cancelled) {
                val index = ProfileStats.best(context, results.filterValues { it > 0 })
                if (index != null) {
                    AppState.selectStrategy(index)
                    AppState.set(UiState(Phase.IDLE, "Лучший профиль сохранён", "${strategies[index].name} · ${results[index]} мс"))
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
        } finally {
            process?.stopCompat()
        }
    }
}
