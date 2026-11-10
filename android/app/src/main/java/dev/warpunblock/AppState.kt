package dev.warpunblock

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import org.json.JSONObject

enum class Phase { IDLE, BUSY, CONNECTED, ERROR }

data class UiState(
    val phase: Phase = Phase.IDLE,
    val title: String = "Готов к подключению",
    val detail: String = "",
    /** Автоподбор: (проверено, всего, не прошло). */
    val batch: Triple<Int, Int, Int>? = null,
    /** true — идёт автоподбор профиля (не подключение). */
    val selecting: Boolean = false,
)

/** Общее состояние приложения: сервис и интерфейс живут в одном процессе. */
object AppState {
    val ui = MutableStateFlow(UiState())
    val logs = MutableStateFlow<List<String>>(emptyList())
    val strategyIndex = MutableStateFlow(0)
    /** Результаты подбора: индекс профиля -> задержка в мс (0 — не прошёл). */
    val results = MutableStateFlow<Map<Int, Int>>(emptyMap())
    /** Последняя задержка WARP в мс. */
    val delay = MutableStateFlow<Int?>(null)
    /** Байты (отправлено, получено) через тоннель. */
    val traffic = MutableStateFlow(0L to 0L)
    /** Момент подключения (мс), null — не подключено. */
    val since = MutableStateFlow<Long?>(null)

    private lateinit var prefs: android.content.SharedPreferences

    fun init(context: Context) {
        if (::prefs.isInitialized) return
        prefs = context.applicationContext.getSharedPreferences("warp", Context.MODE_PRIVATE)
        strategyIndex.value = prefs.getInt("strategy", 0)
        results.value = runCatching {
            val json = JSONObject(prefs.getString("results", "{}") ?: "{}")
            json.keys().asSequence().associate { it.toInt() to json.getInt(it) }
        }.getOrDefault(emptyMap())
    }

    fun selectStrategy(index: Int) {
        strategyIndex.value = index
        prefs.edit().putInt("strategy", index).apply()
    }

    fun saveResults(map: Map<Int, Int>) {
        results.value = map
        val json = JSONObject()
        map.forEach { (key, value) -> json.put(key.toString(), value) }
        prefs.edit().putString("results", json.toString()).apply()
    }

    fun log(text: String) {
        logs.value = (logs.value + text).takeLast(200)
    }

    fun set(state: UiState) {
        ui.value = state
    }
}
