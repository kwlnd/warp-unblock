package dev.warpunblock

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class UpdaterTest {
    private fun release(tag: String, assets: List<String>, draft: Boolean = false, pre: Boolean = false) =
        JSONObject()
            .put("tag_name", tag).put("draft", draft).put("prerelease", pre)
            .put("body", "## Что нового\n- **быстрее**").put("html_url", "https://example/$tag")
            .put("assets", JSONArray(assets.map {
                JSONObject().put("name", it).put("browser_download_url", "https://dl/$it").put("size", 1024)
            }))

    private fun apks(version: String) = listOf(
        "WarpUnblock-android-v$version-arm64-v8a.apk",
        "WarpUnblock-android-v$version-armeabi-v7a.apk",
        "SHA256SUMS.txt",
    )

    @Test fun picksNewestStableAndroidReleaseForAbi() {
        val list = JSONArray(listOf(
            release("android-v0.3.1", apks("0.3.1")),
            release("android-v0.5.0", apks("0.5.0"), pre = true),
            release("v9.0.0", listOf("WarpUnblock-v9.0.0-win-x64.zip")),
            release("android-v0.4.0", apks("0.4.0")),
            release("android-v0.4.5", listOf("notes.txt")),
        ))
        val picked = Updater.pick(list, listOf("armeabi-v7a"), "0.3.1")!!
        assertEquals("android-v0.4.0", picked.tag)
        assertEquals("WarpUnblock-android-v0.4.0-armeabi-v7a.apk", picked.apkName)
        assertEquals("https://dl/SHA256SUMS.txt", picked.sumsUrl)
        assertEquals("Что нового\n- быстрее", picked.notes)
        assertNull(Updater.pick(list, listOf("arm64-v8a"), "0.4.0"))
        assertNull(Updater.pick(list, listOf("x86_64"), "0.3.1"))
    }

    @Test fun readsChecksumForExactFile() {
        val sum = "a".repeat(64)
        val sums = "${"b".repeat(64)}  other.apk\n$sum  WarpUnblock-android-v0.4.0-arm64-v8a.apk\n"
        assertEquals(sum, Updater.expectedSum(sums, "WarpUnblock-android-v0.4.0-arm64-v8a.apk"))
        assertNull(Updater.expectedSum(sums, "missing.apk"))
        assertNull(Updater.expectedSum("short  x.apk", "x.apk"))
    }
}
