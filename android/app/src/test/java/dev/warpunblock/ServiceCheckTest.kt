package dev.warpunblock

import org.junit.Assert.*
import org.junit.Test

class ServiceCheckTest {
    @Test fun delayUrlEncodesTarget() {
        val url = ServiceCheck.delayUrl(9090, "https://discord.com/api/v10/gateway")
        assertEquals(
            "http://127.0.0.1:9090/proxies/WARP/delay?timeout=8000&url=https%3A%2F%2Fdiscord.com%2Fapi%2Fv10%2Fgateway",
            url,
        )
    }

    @Test fun sameServicesAsWindows() {
        assertEquals(listOf("Discord", "YouTube", "Telegram", "Meta"), ServiceCheck.targets.map { it.first })
    }
}
