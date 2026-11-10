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

object Strategies {
    fun load(context: Context): List<Strategy> {
        val json = context.assets.open("strategies.json").bufferedReader().use { it.readText() }
        val array = JSONArray(json)
        return (0 until array.length()).map {
            val item = array.getJSONObject(it)
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
}
