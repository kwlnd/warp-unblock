package dev.warpunblock

import android.content.Context
import org.json.JSONObject
import java.io.File

class Preferences(context: Context) {
    private val prefs = context.applicationContext.getSharedPreferences("options", Context.MODE_PRIVATE)
    var boot: Boolean
        get() = prefs.getBoolean("boot", false)
        set(value) { prefs.edit().putBoolean("boot", value).apply() }
    var autoConnect: Boolean
        get() = prefs.getBoolean("auto", false)
        set(value) { prefs.edit().putBoolean("auto", value).apply() }
    var appMode: String
        get() = prefs.getString("mode", "all") ?: "all"
        set(value) { require(value in listOf("all", "allow", "exclude")); prefs.edit().putString("mode", value).apply() }
    var apps: Set<String>
        get() = prefs.getStringSet("apps", emptySet())!!.toSet()
        set(value) { prefs.edit().putStringSet("apps", value.toSet()).apply() }
    /** WARP перестал отвечать — подобрать другой профиль и переподключиться самому. */
    var autoSwitch: Boolean
        get() = prefs.getBoolean("auto_switch", true)
        set(value) { prefs.edit().putBoolean("auto_switch", value).apply() }
    /** Уже спрашивали исключение из экономии батареи — второй раз сами не предлагаем. */
    var askedBackground: Boolean
        get() = prefs.getBoolean("asked_background", false)
        set(value) { prefs.edit().putBoolean("asked_background", value).apply() }
    var checkUpdates: Boolean
        get() = prefs.getBoolean("check_updates", true)
        set(value) { prefs.edit().putBoolean("check_updates", value).apply() }
    /** Ренат мод — пасхалка: фото на фоне, радуга и «моггерские» надписи. */
    var renat: Boolean
        get() = prefs.getBoolean("renat", false)
        set(value) { prefs.edit().putBoolean("renat", value).apply() }
    var skipVersion: String
        get() = prefs.getString("skip_version", "") ?: ""
        set(value) { prefs.edit().putString("skip_version", value).apply() }
}

/** Portable domain-list v1; identical rules on Windows and Android. */
object DomainLists {
    val names = listOf("proxy-domains.txt", "direct-domains.txt")
    fun normalize(text: String): String {
        require(text.toByteArray(Charsets.UTF_8).size <= 256 * 1024) { "Список больше 256 КиБ" }
        val entries = sortedSetOf<String>()
        text.removePrefix("\uFEFF").lineSequence().forEachIndexed { index, line ->
            val value = line.substringBefore('#').trim().lowercase(java.util.Locale.ROOT)
            if (value.isEmpty()) return@forEachIndexed
            val wildcard = value.startsWith("+.") || value.startsWith("*.")
            val name = if (wildcard) value.substring(2) else value
            require(name.length <= 253 && (wildcard || '.' in name) && !Regex("[0-9.]+").matches(name) &&
                name.split('.').all { it.length in 1..63 && !it.startsWith('-') && !it.endsWith('-') && Regex("[a-z0-9-]+").matches(it) }) {
                "Строка ${index + 1}: нужен домен без URL и IP (например +.example.com)"
            }
            entries.add((if (wildcard) "+." else "") + name)
        }
        return entries.joinToString("\n", postfix = if (entries.isEmpty()) "" else "\n")
    }
    fun file(context: Context, name: String): File {
        require(name in names)
        return File(context.filesDir, "mihomo/lists/$name")
    }
    fun read(context: Context, name: String): String {
        val file = file(context, name)
        return if (file.isFile) file.readText() else context.assets.open("lists/$name").bufferedReader().use { it.readText() }
    }
    fun save(context: Context, proxy: String, direct: String) {
        val normalized = listOf(normalize(proxy), normalize(direct))
        names.zip(normalized).forEach { (name, text) ->
            val file = file(context, name)
            file.parentFile!!.mkdirs()
            android.util.AtomicFile(file).apply {
                val stream = startWrite()
                try { stream.write(text.toByteArray()); finishWrite(stream) } catch (error: Throwable) { failWrite(stream); throw error }
            }
        }
    }
}

data class ProfileStat(val attempts: Long = 0, val successes: Long = 0, val totalMs: Long = 0)
object ProfileStats {
    private fun file(context: Context) = File(context.filesDir, "profile-stats.json")
    @Synchronized fun load(context: Context): Map<Int, ProfileStat> = runCatching {
        val json = JSONObject(file(context).readText())
        json.keys().asSequence().associate { key ->
            val v = json.getJSONObject(key)
            key.toInt() to ProfileStat(v.getLong("attempts"), v.getLong("successes"), v.getLong("total_ms"))
        }
    }.getOrDefault(emptyMap())
    @Synchronized fun clear(context: Context) {
        file(context).delete()
    }
    @Synchronized fun record(context: Context, index: Int, delay: Int?) {
        val all = load(context).toMutableMap()
        val old = all[index] ?: ProfileStat()
        all[index] = ProfileStat(old.attempts + 1, old.successes + if (delay != null) 1 else 0, old.totalMs + (delay ?: 0))
        val json = JSONObject()
        all.forEach { (i, s) -> json.put(i.toString(), JSONObject().put("attempts", s.attempts).put("successes", s.successes).put("total_ms", s.totalMs)) }
        val atomic = android.util.AtomicFile(file(context))
        val out = atomic.startWrite()
        try { out.write(json.toString().toByteArray()); atomic.finishWrite(out) } catch (e: Throwable) { atomic.failWrite(out); throw e }
    }
    fun best(context: Context, passed: Map<Int, Int>): Int? {
        val all = load(context)
        return passed.keys.minWithOrNull(compareByDescending<Int> {
            val s = all[it] ?: ProfileStat()
            s.successes.toDouble() / s.attempts.coerceAtLeast(1)
        }.thenBy { passed[it] })
    }
}
