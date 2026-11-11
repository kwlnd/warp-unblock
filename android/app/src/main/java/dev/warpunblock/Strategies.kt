package dev.warpunblock

import android.content.Context
import org.json.JSONArray

/** Профиль доставки WireGuard до WARP: эндпоинт, порт и мусорные пакеты перед handshake (AmneziaWG). */
data class Strategy(
    val name: String,
    val endpoint: String,
    val port: Int,
    val jc: Int,
    val jmin: Int,
    val jmax: Int,
)

/**
 * Каталог общий с Windows (windows/app/config/strategies.json копируется при сборке). Профили с
 * полем `zapret` используют winws и есть только в Windows-версии — здесь они пропускаются.
 */
object Strategies {
    fun load(context: Context): List<Strategy> = parse(context.assets.open("strategies.json").bufferedReader().use { it.readText() })

    internal fun parse(json: String): List<Strategy> {
        val array = JSONArray(json)
        return (0 until array.length()).map { array.getJSONObject(it) }.filter { !it.has("zapret") }.map { item ->
            Strategy(
                name = item.getString("name").removePrefix("WARP · "),
                endpoint = item.getString("endpoint"),
                port = item.getInt("port"),
                jc = item.getInt("jc"),
                jmin = item.getInt("jmin"),
                jmax = item.getInt("jmax"),
            )
        }
    }

    /** Отпечаток каталога: при смене профилей статистика и выбор по номеру сбрасываются. */
    fun fingerprint(list: List<Strategy>): String = list.joinToString("|").hashCode().toUInt().toString(16)
}
