package dev.warpunblock

import org.junit.Assert.*
import org.junit.Test

class PortableSettingsTest {
    @Test fun domainListNormalization() {
        assertEquals("+.example.com\nfoo.example\n", DomainLists.normalize("\uFEFF# note\n*.Example.COM\n+.example.com # duplicate\nfoo.example\n"))
        assertEquals("", DomainLists.normalize("# empty\n"))
        assertEquals("+.ru\n+.su\n", DomainLists.normalize("+.ru\n+.su"))
        for (bad in listOf("https://a.com", "1.1.1.1", "bad..com", "a-.com", "a.com,REJECT", "пример.рф", "a.com/path")) {
            assertTrue(bad, runCatching { DomainLists.normalize(bad) }.isFailure)
        }
    }
    @Test fun stableVersionsOnly() {
        assertEquals(listOf(0, 10, 0), stableVersion("0.10.0"))
        for (bad in listOf("android-v0.3.0", "0.3.0-rc1", "0.3", "999999999999.0.0")) assertNull(stableVersion(bad))
    }
    @Test fun desktopProfileQrCanBeImported() {
        val key = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        val text = "[Interface]\nPrivateKey = $key\nAddress = 172.16.0.2/32, 2606:4700:110::1/128\n[Peer]\nPublicKey = $key\nReserved = 1, 2, 255\n"
        val account = WarpAccount.parseWgcf(text)
        assertEquals(listOf(1, 2, 255), account.reserved)
        assertEquals("172.16.0.2", account.ipv4)
        assertTrue(runCatching { WarpAccount.parseWgcf(text.replace("1, 2, 255", "1, 2, 999")) }.isFailure)
        assertTrue(runCatching { WarpAccount.parseWgcf(text.replace(key, "bad")) }.isFailure)
    }
}
