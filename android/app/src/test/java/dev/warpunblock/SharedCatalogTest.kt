package dev.warpunblock

import org.junit.Assert.*
import org.junit.Test
import java.io.File

/** Общие с Windows файлы: windows/app/config/strategies.json и registration.json. */
class SharedCatalogTest {
    private fun shared(name: String) = File("../../windows/app/config/$name").readText()

    @Test fun androidUsesProfilesWithoutWinws() {
        val list = Strategies.parse(shared("strategies.json"))
        assertTrue(list.size >= 8)
        assertEquals(list.size, list.map { it.name }.toSet().size)
        // Профили с полем zapret (winws) есть только в Windows-версии.
        assertEquals(0, list.count { it.name.contains("fake", ignoreCase = true) || it.name.contains("фейк") })
        assertNotEquals(Strategies.fingerprint(list), Strategies.fingerprint(list.drop(1)))
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
