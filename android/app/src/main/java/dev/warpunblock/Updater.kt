package dev.warpunblock

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import org.json.JSONArray
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

/** Android-релиз из GitHub с APK под ABI этого телефона. */
data class AndroidRelease(
    val tag: String,
    val version: String,
    val notes: String,
    val page: String,
    val apkName: String,
    val apkUrl: String,
    val size: Long,
    val sumsUrl: String?,
)

internal fun stableVersion(tag: String): List<Int>? {
    if (!Regex("[0-9]+\\.[0-9]+\\.[0-9]+").matches(tag)) return null
    return tag.split('.').map { it.toIntOrNull() ?: return null }
}

internal fun compareVersion(a: List<Int>, b: List<Int>): Int {
    for (i in 0..2) if (a[i] != b[i]) return a[i].compareTo(b[i])
    return 0
}

/** Markdown релиза в простой текст для окна обновления. */
internal fun plainNotes(markdown: String): String =
    markdown.lines().joinToString("\n") { it.trimStart('#').trimStart().replace("**", "").replace("`", "") }.trim()

object Updater {
    private const val API = "https://api.github.com/repos/kwlnd/warp-unblock/releases?per_page=100&page="
    private const val MAX_APK = 200L * 1024 * 1024

    /** ABI, под которые публикуются APK, в порядке предпочтения телефона. */
    fun abis(): List<String> = Build.SUPPORTED_ABIS.filter { it == "arm64-v8a" || it == "armeabi-v7a" }

    /** Версия из тега: общий `v0.7.0` или старый `android-v0.3.1`; иначе null. */
    internal fun tagVersion(tag: String): List<Int>? {
        val bare = tag.removePrefix("android-")
        if (!bare.startsWith("v")) return null
        return stableVersion(bare.removePrefix("v"))
    }

    /**
     * Самый свежий стабильный релиз новее [current], у которого есть APK для одной из [abis].
     * С v0.7.0 Windows и Android выходят одним релизом `v*`; старые `android-v*` тоже учитываются.
     * Чистая функция — проверяется unit-тестом.
     */
    internal fun pick(releases: JSONArray, abis: List<String>, current: String): AndroidRelease? {
        val currentVersion = stableVersion(current) ?: return null
        var best: Pair<List<Int>, AndroidRelease>? = null
        for (i in 0 until releases.length()) {
            val release = releases.getJSONObject(i)
            if (release.optBoolean("draft", true) || release.optBoolean("prerelease", true)) continue
            val tag = release.optString("tag_name")
            val version = tagVersion(tag) ?: continue
            if (compareVersion(version, currentVersion) <= 0) continue
            if (best != null && compareVersion(version, best.first) <= 0) continue
            val assets = release.optJSONArray("assets") ?: continue
            var apk: Triple<String, String, Long>? = null
            var sums: String? = null
            for (abi in abis) {
                for (j in 0 until assets.length()) {
                    val asset = assets.getJSONObject(j)
                    val name = asset.optString("name")
                    if (apk == null && name.endsWith("-$abi.apk")) {
                        apk = Triple(name, asset.optString("browser_download_url"), asset.optLong("size"))
                    }
                }
            }
            for (j in 0 until assets.length()) {
                val asset = assets.getJSONObject(j)
                if (asset.optString("name") == "SHA256SUMS.txt") sums = asset.optString("browser_download_url")
            }
            val (name, url, size) = apk ?: continue
            best = version to AndroidRelease(
                tag = tag,
                version = version.joinToString("."),
                notes = plainNotes(release.optString("body")),
                page = release.optString("html_url"),
                apkName = name,
                apkUrl = url,
                size = size,
                sumsUrl = sums,
            )
        }
        return best?.second
    }

    private fun open(url: String, timeout: Int = 15_000): HttpURLConnection =
        (URL(url).openConnection() as HttpURLConnection).apply {
            connectTimeout = 10_000
            readTimeout = timeout
            setRequestProperty("User-Agent", "warp-unblock-android")
            instanceFollowRedirects = true
        }

    /** null — установлена актуальная версия. */
    fun check(): AndroidRelease? {
        var best: AndroidRelease? = null
        for (page in 1..10) {
            val connection = open(API + page)
            val releases = try {
                check(connection.responseCode == 200) { "GitHub: HTTP ${connection.responseCode}" }
                JSONArray(connection.inputStream.bufferedReader().use { it.readText() })
            } finally { connection.disconnect() }
            val found = pick(releases, abis(), BuildConfig.VERSION_NAME)
            if (found != null && (best == null ||
                    compareVersion(stableVersion(found.version)!!, stableVersion(best.version)!!) > 0)) best = found
            if (releases.length() < 100) break
        }
        return best
    }

    internal fun expectedSum(sums: String, name: String): String? =
        sums.lineSequence()
            .map { it.trim().split(Regex("\\s+"), limit = 2) }
            .firstOrNull { it.size == 2 && it[1].removePrefix("*") == name }
            ?.get(0)?.lowercase()
            ?.takeIf { Regex("[0-9a-f]{64}").matches(it) }

    /** Скачивает APK в кэш и сверяет SHA-256 по SHA256SUMS.txt релиза. */
    fun download(context: Context, release: AndroidRelease, progress: (Long, Long) -> Unit): File {
        val sumsUrl = release.sumsUrl ?: error("У релиза нет SHA256SUMS.txt — установка отменена")
        val sums = open(sumsUrl).let { c ->
            try { check(c.responseCode == 200) { "SHA256SUMS: HTTP ${c.responseCode}" }; c.inputStream.bufferedReader().use { it.readText() } }
            finally { c.disconnect() }
        }
        val expected = expectedSum(sums, release.apkName) ?: error("Нет контрольной суммы для ${release.apkName}")
        val dir = File(context.cacheDir, "updates").apply { deleteRecursively(); mkdirs() }
        val file = File(dir, release.apkName)
        val digest = MessageDigest.getInstance("SHA-256")
        val connection = open(release.apkUrl, timeout = 60_000)
        try {
            check(connection.responseCode == 200) { "APK: HTTP ${connection.responseCode}" }
            val total = connection.contentLengthLong.takeIf { it > 0 } ?: release.size
            check(total <= MAX_APK) { "APK слишком большой" }
            var done = 0L
            connection.inputStream.use { input ->
                file.outputStream().use { output ->
                    val buffer = ByteArray(64 * 1024)
                    while (true) {
                        val read = input.read(buffer)
                        if (read < 0) break
                        done += read
                        check(done <= MAX_APK) { "APK слишком большой" }
                        digest.update(buffer, 0, read)
                        output.write(buffer, 0, read)
                        progress(done, total)
                    }
                }
            }
        } finally { connection.disconnect() }
        val actual = digest.digest().joinToString("") { "%02x".format(it) }
        if (actual != expected) {
            file.delete()
            error("Контрольная сумма не совпала — файл повреждён или подменён")
        }
        return file
    }

    /** Нужно ли сначала разрешить установку из этого приложения (Android 8+). */
    fun needsInstallPermission(context: Context): Boolean =
        Build.VERSION.SDK_INT >= 26 && !context.packageManager.canRequestPackageInstalls()

    fun openInstallPermission(context: Context) {
        if (Build.VERSION.SDK_INT >= 26) {
            context.startActivity(
                Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${context.packageName}"))
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            )
        }
    }

    /** Открывает системный установщик для скачанного APK. */
    fun install(context: Context, apk: File) {
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.updates", apk)
        context.startActivity(
            Intent(Intent.ACTION_VIEW)
                .setDataAndType(uri, "application/vnd.android.package-archive")
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        )
    }
}
