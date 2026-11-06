package dev.warpunblock

import android.content.Context
import java.io.File
import java.net.HttpURLConnection
import java.net.URL

/** Ядро: рабочая папка mihomo, конфиги, запуск процесса и проверка WARP через API ядра. */
class Core(private val context: Context) {
    val home = File(context.filesDir, "mihomo")
    private val binary = File(context.applicationInfo.nativeLibraryDir, "libmihomo.so")

    fun prepare() {
        home.mkdirs()
        File(home, "lists").mkdirs()
        copyAsset("config.yaml", File(home, "config.yaml"))
        if (!File(home, "lists/proxy-domains.txt").exists()) copyAsset("lists/proxy-domains.txt", File(home, "lists/proxy-domains.txt"))
        if (!File(home, "lists/direct-domains.txt").exists()) copyAsset("lists/direct-domains.txt", File(home, "lists/direct-domains.txt"))
        File(home, "test.yaml").writeText(
            """
            mixed-port: $TEST_PROXY_PORT
            external-controller: 127.0.0.1:$TEST_API_PORT
            log-level: warning
            ipv6: true
            mode: rule
            proxy-providers:
              warp:
                type: file
                path: ./warp.yaml
            proxy-groups:
              - name: WARP
                type: fallback
                use:
                  - warp
                url: https://www.gstatic.com/generate_204
                interval: 300
            rules:
              - MATCH,WARP
            """.trimIndent() + "\n",
        )
    }

    /** Провайдер с WireGuard-прокси WARP; формат тот же, что пишет warp.ps1 на Windows. */
    fun writeProfile(strategy: Strategy, account: WarpAccount) {
        val reserved = if (account.reserved.size == 3) {
            "\n                reserved: " + account.reserved.joinToString(", ", "[", "]")
        } else {
            ""
        }
        File(home, "warp.yaml").writeText(
            """
            proxies:
              - name: WARP-WG
                type: wireguard
                server: ${strategy.endpoint}
                port: ${strategy.port}
                ip: ${account.ipv4}
                ipv6: ${account.ipv6}
                private-key: ${account.privateKey}
                public-key: ${account.publicKey}
                mtu: 1280
                udp: true$reserved
                amnezia-wg-option:
                  jc: ${strategy.jc}
                  jmin: ${strategy.jmin}
                  jmax: ${strategy.jmax}
            """.trimIndent() + "\n",
        )
    }

    fun start(test: Boolean): Process {
        check(binary.isFile) { "Нет ядра mihomo (${binary.name}) для этой архитектуры" }
        val log = File(home, if (test) "mihomo-test.log" else "mihomo.log")
        return ProcessBuilder(
            binary.absolutePath, "-d", home.absolutePath, "-f",
            File(home, if (test) "test.yaml" else "config.yaml").absolutePath,
        )
            .directory(home)
            .logged(log)
    }

    /** Ждёт ответа WARP через delay-тест ядра. Возвращает задержку в мс или null по таймауту. */
    fun waitDelay(apiPort: Int, timeoutSeconds: Int, alive: () -> Boolean): Int? {
        val url = "http://127.0.0.1:$apiPort/proxies/WARP/delay?timeout=10000" +
            "&url=https%3A%2F%2Fwww.gstatic.com%2Fgenerate_204"
        val deadline = System.currentTimeMillis() + timeoutSeconds * 1000L
        while (System.currentTimeMillis() < deadline && alive()) {
            val delay = runCatching {
                val connection = URL(url).openConnection() as HttpURLConnection
                connection.connectTimeout = 3_000
                connection.readTimeout = 14_000
                try {
                    if (connection.responseCode == 200) {
                        org.json.JSONObject(connection.inputStream.bufferedReader().use { it.readText() })
                            .optInt("delay", 0)
                    } else {
                        0
                    }
                } finally {
                    connection.disconnect()
                }
            }.getOrDefault(0)
            if (delay > 0) return delay
            Thread.sleep(1000)
        }
        return null
    }

    fun logTail(lines: Int = 40): String {
        val file = File(home, "mihomo.log")
        if (!file.isFile) return ""
        return java.io.RandomAccessFile(file, "r").use { input ->
            val offset = (input.length() - 64 * 1024).coerceAtLeast(0)
            input.seek(offset)
            val bytes = ByteArray((input.length() - offset).toInt())
            input.readFully(bytes)
            val text = bytes.toString(Charsets.UTF_8)
            (if (offset > 0) text.substringAfter('\n', "") else text).lineSequence().toList().takeLast(lines).joinToString("\n")
        }
    }

    private fun copyAsset(name: String, target: File) {
        context.assets.open(name).use { input -> target.outputStream().use { input.copyTo(it) } }
    }

    companion object {
        const val PROXY_PORT = 7890
        const val API_PORT = 9090
        const val TEST_PROXY_PORT = 7891
        const val TEST_API_PORT = 9091
    }
}
