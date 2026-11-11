package dev.warpunblock

import android.content.Context
import org.json.JSONObject

/** Профиль маскировки: мусорные пакеты перед handshake (AmneziaWG). Адрес берётся из пула. */
data class Profile(
    val name: String,
    val jc: Int,
    val jmin: Int,
    val jmax: Int,
)

/** Профиль, собранный с конкретным адресом из пула: так он пишется в конфиг ядра. */
data class Strategy(
    val name: String,
    val endpoint: String,
    val port: Int,
    val jc: Int,
    val jmin: Int,
    val jmax: Int,
)

/**
 * Каталог: пул адресов WARP («ip:порт», в порядке предпочтения) и профили маскировки. Адрес
 * программа подбирает сама: провайдер режет связку «адрес + порт», а маскировку — отдельно.
 */
data class Catalog(val endpoints: List<String>, val profiles: List<Profile>) {
    val size: Int get() = profiles.size

    /** Номер адреса в пуле; неизвестный или пустой — первый. */
    fun endpointIndex(endpoint: String): Int = endpoints.indexOf(endpoint).coerceAtLeast(0)

    fun resolve(profile: Int, endpoint: Int): Strategy {
        val p = profiles[profile.coerceIn(profiles.indices)]
        val (ip, port) = endpoints[Math.floorMod(endpoint, endpoints.size)].split(':')
        return Strategy(p.name, ip, port.toInt(), p.jc, p.jmin, p.jmax)
    }

    /**
     * Порядок подбора: сначала все адреса текущего профиля, начиная с [endpoint], затем остальные
     * профили. Режут обычно адрес — тогда хватает соседнего; не прошёл ни один — значит узнали
     * саму маскировку, и дальше идут другие профили.
     */
    fun searchOrder(profile: Int, endpoint: Int): List<Pair<Int, Int>> {
        val first = profile.coerceIn(profiles.indices)
        return (listOf(first) + profiles.indices.filter { it != first }).flatMap { p ->
            endpoints.indices.map { k -> p to (endpoint + k) % endpoints.size }
        }
    }
}

/**
 * Каталог общий с Windows (windows/app/config/strategies.json копируется при сборке). Профили с
 * полем `zapret` используют winws и есть только в Windows-версии — здесь они пропускаются.
 */
object Strategies {
    private val address = Regex("""(\d{1,3}(\.\d{1,3}){3}):(\d{1,5})""")

    fun load(context: Context): Catalog = parse(context.assets.open("strategies.json").bufferedReader().use { it.readText() })

    internal fun parse(json: String): Catalog {
        val root = JSONObject(json)
        val list = root.getJSONArray("endpoints")
        val endpoints = (0 until list.length()).map { list.getString(it) }
        require(endpoints.isNotEmpty() && endpoints.all { address.matches(it) }) { "Неверный пул адресов WARP" }
        val array = root.getJSONArray("profiles")
        val profiles = (0 until array.length()).map { array.getJSONObject(it) }.filter { !it.has("zapret") }.map { item ->
            Profile(
                // На Android zapret нет вовсе, пометка «без zapret» только занимает место.
                name = item.getString("name").removeSuffix(" · без zapret"),
                jc = item.getInt("jc"),
                jmin = item.getInt("jmin"),
                jmax = item.getInt("jmax"),
            )
        }
        require(profiles.isNotEmpty()) { "В каталоге нет профилей для Android" }
        return Catalog(endpoints, profiles)
    }

    /** Отпечаток каталога: при смене профилей статистика и выбор по номеру сбрасываются. */
    fun fingerprint(catalog: Catalog): String =
        (catalog.endpoints + catalog.profiles.map { it.toString() }).joinToString("|").hashCode().toUInt().toString(16)
}
