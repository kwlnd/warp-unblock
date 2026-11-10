package dev.warpunblock

import android.content.Context
import java.io.File
import java.net.InetSocketAddress
import java.net.Proxy

/**
 * Обход DPI для регистрации WARP без root: локальный SOCKS-прокси ByeDPI (аналог zapret/tpws),
 * через который идёт только запрос регистрации. Сначала пробуем напрямую, потом по очереди
 * несколько стратегий десинхронизации.
 */
object Bypass {
    private const val PORT = 1081

    private val STRATEGIES = listOf(
        listOf("--split", "1+s"),
        listOf("--disorder", "1"),
        listOf("--split", "1+s", "--disorder", "3+s"),
        listOf("--fake", "-1", "--ttl", "8"),
        listOf("--tlsrec", "3+s", "--split", "1+s"),
        listOf("--oob", "1"),
        listOf("--split", "2", "--tlsrec", "1+s"),
    )

    fun register(context: Context, report: (String) -> Unit): WarpAccount {
        val dir = context.filesDir
        report("Регистрирую аккаунт WARP")
        var last: Throwable? = null
        try {
            return WarpAccount.register(dir, null, 8_000)
        } catch (error: Exception) {
            last = error
            AppState.log("Напрямую не вышло: ${error.message?.take(80)}")
        }
        val binary = File(context.applicationInfo.nativeLibraryDir, "libbyedpi.so")
        if (!binary.isFile) throw IllegalStateException(last?.message ?: "Регистрация не удалась")
        val log = File(context.filesDir, "byedpi.log")
        for ((index, args) in STRATEGIES.withIndex()) {
            report("Обход DPI для регистрации: ${index + 1}/${STRATEGIES.size}")
            val process = ProcessBuilder(
                listOf(binary.absolutePath, "--ip", "127.0.0.1", "--port", "$PORT") + args,
            ).logged(log)
            try {
                Thread.sleep(500)
                if (!process.runningCompat()) continue
                val proxy = Proxy(Proxy.Type.SOCKS, InetSocketAddress("127.0.0.1", PORT))
                val account = WarpAccount.register(dir, proxy, 10_000)
                AppState.log("Регистрация прошла с обходом DPI: ${args.joinToString(" ")}")
                return account
            } catch (error: Exception) {
                last = error
            } finally {
                process.stopCompat()
            }
        }
        throw IllegalStateException(
            "Не удалось зарегистрировать WARP даже с обходом DPI. " +
                "Импортируйте wgcf-профиль с компьютера на вкладке «Профили».",
            last,
        )
    }
}
