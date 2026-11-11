package dev.warpunblock

import androidx.test.core.app.ApplicationProvider
import com.github.takahirom.roborazzi.captureRoboImage
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** Снимки экранов без телефона: gradle testDebugUnitTest -Pscreenshots=1 → app/build/screenshots/. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [34], qualifiers = "w400dp-h860dp-xxhdpi")
class ScreenshotTest {
    @Before
    fun only() {
        assumeTrue(System.getProperty("warp.screenshots").orEmpty().isNotEmpty())
        AppState.init(ApplicationProvider.getApplicationContext())
        Motion.frozen = true
    }

    private fun shot(name: String, night: Boolean, tab: Int) {
        RuntimeEnvironment.setQualifiers(if (night) "+night" else "+notnight")
        captureRoboImage("build/screenshots/$name.png") {
            WarpTheme { Screen(onToggle = {}, onAuto = {}, initialTab = tab) }
        }
    }

    @Test
    fun homeIdleDark() {
        AppState.set(UiState())
        shot("home-idle-dark", night = true, tab = 0)
    }

    @Test
    fun homeConnectedLight() {
        AppState.set(UiState(Phase.CONNECTED, "Обход работает", "WARP отвечает"))
        AppState.since.value = System.currentTimeMillis() - 754_000
        AppState.delay.value = 42
        AppState.traffic.value = 3_400_000L to 128_000_000L
        shot("home-connected-light", night = false, tab = 0)
    }

    @Test
    fun homeBusyLight() {
        AppState.set(UiState(Phase.BUSY, "Подключаю…"))
        shot("home-busy-light", night = false, tab = 0)
    }

    @Test
    fun homeRenat() {
        Mog.on = true
        try {
            AppState.set(UiState(Phase.CONNECTED, "Обход работает", "WARP отвечает"))
            shot("home-renat", night = true, tab = 0)
        } finally {
            Mog.on = false
        }
    }

    @Test
    fun profilesDark() {
        AppState.set(UiState())
        shot("profiles-dark", night = true, tab = 1)
    }

    @Test
    fun settingsDark() {
        AppState.set(UiState())
        shot("settings-dark", night = true, tab = 3)
    }

    @Test
    fun listsLight() {
        AppState.set(UiState())
        shot("lists-light", night = false, tab = 2)
    }
}
