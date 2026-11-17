package dev.warpunblock

import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.net.URLEncoder

/** Итог проверки одного сервиса: задержка в мс или null и причина. */
data class ServiceResult(val name: String, val delay: Int?, val detail: String)

/**
 * HTTPS-доступность сервисов через группу WARP работающего ядра — тот же набор, что в
 * Windows-версии (`features::check_services`). Голос и видео так не проверить: это только ответ сайта.
 */
object ServiceCheck {
    val targets = listOf(
        "Discord" to "https://discord.com/api/v10/gateway",
        "YouTube" to "https://www.youtube.com/",
        "Telegram" to "https://telegram.org/",
        "Meta" to "https://www.instagram.com/",
    )

    private var thread: Thread? = null

    val running: Boolean get() = thread?.isAlive == true

    /** Только при живом подключении: проверяется основное ядро, а не тестовое. */
    fun start() {
        if (running || AppState.ui.value.phase != Phase.CONNECTED) return
        AppState.services.value = emptyList()
        AppState.servicesChecking.value = true
        thread = Thread({
            try {
                val results = mutableListOf<ServiceResult>()
                for ((name, url) in targets) {
                    results += check(Core.API_PORT, name, url)
                    AppState.services.value = results.toList()
                }
                val ok = results.count { it.delay != null }
                AppState.log("Сервисы через WARP: доступно $ok из ${results.size} · " + results.joinToString(" · ") { "${it.name} ${it.detail}" })
            } finally {
                AppState.servicesChecking.value = false
            }
        }, "service-check").apply { isDaemon = true; start() }
    }

    internal fun delayUrl(apiPort: Int, url: String): String =
        "http://127.0.0.1:$apiPort/proxies/WARP/delay?timeout=8000&url=" + URLEncoder.encode(url, "UTF-8")

    private fun check(apiPort: Int, name: String, url: String): ServiceResult {
        val connection = URL(delayUrl(apiPort, url)).openConnection() as HttpURLConnection
        return try {
            connection.connectTimeout = 1_000
            connection.readTimeout = 12_000
            val code = connection.responseCode
            val body = (if (code == 200) connection.inputStream else connection.errorStream)
                ?.bufferedReader()?.use { it.readText() }.orEmpty()
            val json = runCatching { JSONObject(body) }.getOrNull()
            val delay = json?.optInt("delay", 0) ?: 0
            when {
                code == 200 && delay > 0 -> ServiceResult(name, delay, "$delay мс")
                code == 408 || code == 504 -> ServiceResult(name, null, "нет ответа")
                else -> ServiceResult(name, null, json?.optString("message")?.takeIf { it.isNotBlank() } ?: "ошибка $code")
            }
        } catch (error: Exception) {
            ServiceResult(name, null, error.message ?: "ошибка")
        } finally {
            connection.disconnect()
        }
    }
}
