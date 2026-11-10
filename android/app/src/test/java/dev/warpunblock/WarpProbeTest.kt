package dev.warpunblock

import org.junit.Assert.*
import org.junit.Test
import java.net.ServerSocket
import java.net.InetAddress
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread

class WarpProbeTest {
    @Test fun stalledResponseCannotExceedOverallBudget() {
        ServerSocket(0, 1, InetAddress.getByName("127.0.0.1")).use { server ->
            val worker = thread {
                server.accept().use { socket ->
                    socket.getInputStream().read(ByteArray(2048))
                    Thread.sleep(1000)
                }
            }
            val started = System.nanoTime()
            assertNull(WarpProbe.waitDelay(server.localPort, 250) { true })
            val elapsed = TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - started)
            assertTrue("Probe exceeded budget: $elapsed ms", elapsed < 750)
            worker.join()
        }
    }
    @Test fun availableProfileReturnsWithoutWaitingForTimeout() {
        ServerSocket(0, 1, InetAddress.getByName("127.0.0.1")).use { server ->
            val worker = thread {
                server.accept().use { socket ->
                    socket.getInputStream().read(ByteArray(2048))
                    val body = "{\"delay\":42}"
                    socket.getOutputStream().write("HTTP/1.1 200 OK\r\nContent-Length: ${body.length}\r\nConnection: close\r\n\r\n$body".toByteArray())
                }
            }
            assertEquals(42, WarpProbe.waitDelay(server.localPort, 1000) { true })
            worker.join()
        }
    }
}
