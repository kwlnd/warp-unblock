package dev.warpunblock

import org.junit.Assert.*
import org.junit.Test
import java.io.File

/** Общие с Windows файлы: windows/app/config/strategies.json и registration.json. */
class SharedCatalogTest {
    private fun shared(name: String) = File("../../windows/app/config/$name").readText()

    @Test fun androidUsesProfilesWithoutWinws() {
        val catalog = Strategies.parse(shared("strategies.json"))
        val list = catalog.profiles
        assertTrue(list.size >= 3)
        assertTrue(catalog.endpoints.size >= 8)
        assertEquals(list.size, list.map { it.name }.toSet().size)
        assertEquals(catalog.endpoints.size, catalog.endpoints.toSet().size)
        // Профили с полем zapret (winws) есть только в Windows-версии.
        assertEquals(0, list.count { it.name.contains("fake", ignoreCase = true) || it.name.contains("фейк") })
        assertNotEquals(Strategies.fingerprint(catalog), Strategies.fingerprint(catalog.copy(profiles = list.drop(1))))
    }

    @Test fun searchTriesEveryAddressOfTheCurrentProfileFirst() {
        val catalog = Strategies.parse(shared("strategies.json"))
        val m = catalog.endpoints.size
        val order = catalog.searchOrder(1, 3)
        assertEquals(catalog.size * m, order.size)
        assertEquals(1 to 3, order[0])
        assertEquals(1 to 2, order[m - 1])
        assertTrue(order.take(m).all { it.first == 1 })
        assertEquals(0 to 3, order[m])
        val s = catalog.resolve(0, 0)
        assertEquals(catalog.endpoints[0], "${s.endpoint}:${s.port}")
    }

    @Test fun registrationMethodsAreSharedAndSafe() {
        val methods = Bypass.parse(shared("registration.json"))
        assertTrue(methods.size >= 7)
        assertTrue(methods.any { it.second == listOf("--fake", "-1", "--ttl", "8") })
        assertThrows(IllegalArgumentException::class.java) {
            Bypass.parse("""[{"name":"x","byedpi":["--port","9999"]}]""")
        }
    }
}
