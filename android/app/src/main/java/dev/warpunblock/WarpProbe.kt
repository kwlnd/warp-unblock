package dev.warpunblock

import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

/** A single monotonic budget covers startup retries, connect, headers and response body. */
internal object WarpProbe {
    fun waitDelay(apiPort: Int, timeoutMillis: Long, alive: () -> Boolean): Int? {
        val deadline = System.nanoTime() + TimeUnit.MILLISECONDS.toNanos(timeoutMillis)
        val active = AtomicReference<HttpURLConnection?>()
        val timer = Executors.newSingleThreadScheduledExecutor { task ->
            Thread(task, "warp-probe-deadline").apply { isDaemon = true }
        }
        val expiry = timer.schedule({ active.get()?.disconnect() }, timeoutMillis, TimeUnit.MILLISECONDS)
        fun remaining() = TimeUnit.NANOSECONDS.toMillis((deadline - System.nanoTime()).coerceAtLeast(0))
        try {
            while (remaining() > 0 && alive()) {
                val budget = remaining().coerceAtMost(Int.MAX_VALUE.toLong()).toInt()
                if (budget <= 0) break
                val url = "http://127.0.0.1:$apiPort/proxies/WARP/delay?timeout=${(budget - 100).coerceAtLeast(1)}" +
                    "&url=https%3A%2F%2Fwww.gstatic.com%2Fgenerate_204"
                val connection = URL(url).openConnection() as HttpURLConnection
                active.set(connection)
                val delay = try {
                    connection.connectTimeout = budget.coerceAtMost(250)
                    connection.readTimeout = budget
                    connection.connect()
                    connection.readTimeout = remaining().coerceIn(1, Int.MAX_VALUE.toLong()).toInt()
                    if (connection.responseCode == 200) {
                        JSONObject(connection.inputStream.bufferedReader().use { it.readText() }).optInt("delay", 0)
                    } else 0
                } catch (_: Exception) {
                    0
                } finally {
                    active.compareAndSet(connection, null)
                    connection.disconnect()
                }
                if (remaining() <= 0 || !alive()) break
                if (delay > 0) return delay
                Thread.sleep(remaining().coerceAtMost(100))
            }
            return null
        } finally {
            expiry.cancel(true)
            timer.shutdownNow()
            active.getAndSet(null)?.disconnect()
        }
    }
}
