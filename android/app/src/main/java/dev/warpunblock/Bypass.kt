package dev.warpunblock

import android.content.Context
import org.json.JSONArray
import java.io.File
import java.net.InetSocketAddress
import java.net.Proxy

/**
 * Обход DPI для регистрации WARP без root: локальный SOCKS-прокси ByeDPI (аналог zapret/tpws),
 * через который идёт только запрос регистрации. Сначала пробуем напрямую, потом по очереди
 * способы из общего с Windows registration.json (там же их варианты для winws).
 */
object Bypass {
    private const val PORT = 1081

    /** Ключи ByeDPI, которые можно задавать в общем файле: только десинхронизация. */
    private val ALLOWED = setOf("--split", "--disorder", "--fake", "--ttl", "--tlsrec", "--oob", "--disoob", "--mod-http", "--fake-offset")

    /** Способы обхода из общего с Windows registration.json; у Android — вариант `byedpi`. */
    internal fun parse(json: String): List<Pair<String, List<String>>> {
        val array = JSONArray(json)
        return (0 until array.length()).map { array.getJSONObject(it) }.mapNotNull { item ->
            val args = item.optJSONArray("byedpi") ?: return@mapNotNull null
            val list = (0 until args.length()).map { args.getString(it) }
            require(list.isNotEmpty() && list.size <= 16) { "Пустой способ: ${item.getString("name")}" }
            list.forEachIndexed { i, arg ->
                if (arg.startsWith("--")) require(arg in ALLOWED) { "Недопустимый ключ ByeDPI: $arg" }
                else require(i > 0 && Regex("[-+0-9a-z:]{1,16}").matches(arg)) { "Недопустимое значение: $arg" }
            }
            item.getString("name") to list
        }
    }

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
        val methods = parse(context.assets.open("registration.json").bufferedReader().use { it.readText() })
        for ((index, method) in methods.withIndex()) {
            val (name, args) = method
            report("Обход DPI для регистрации: ${index + 1}/${methods.size} · $name")
            val process = ProcessBuilder(
                listOf(binary.absolutePath, "--ip", "127.0.0.1", "--port", "$PORT") + args,
            ).logged(log)
            try {
                Thread.sleep(500)
                if (!process.runningCompat()) continue
                val proxy = Proxy(Proxy.Type.SOCKS, InetSocketAddress("127.0.0.1", PORT))
                val account = WarpAccount.register(dir, proxy, 10_000)
                AppState.log("Регистрация прошла с обходом DPI: $name")
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
