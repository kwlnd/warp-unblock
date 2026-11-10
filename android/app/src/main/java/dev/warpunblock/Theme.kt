package dev.warpunblock

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight

/** Палитра общая с Windows-версией (windows/source/src/ui.rs). */
@Immutable
data class WarpPalette(
    val background: Color,
    val sidebar: Color,
    val surface: Color,
    val surfaceHover: Color,
    val border: Color,
    val text: Color,
    val muted: Color,
    val accent: Color,
    val onAccent: Color,
    val ok: Color,
    val warn: Color,
    val error: Color,
)

val DarkPalette = WarpPalette(
    background = Color(0xFF131418),
    sidebar = Color(0xFF18191E),
    surface = Color(0xFF1F2127),
    surfaceHover = Color(0xFF2A2D35),
    border = Color(0xFF33363F),
    text = Color(0xFFF0F2F5),
    muted = Color(0xFF9AA1AD),
    accent = Color(0xFFF6821F),
    onAccent = Color(0xFF1C1004),
    ok = Color(0xFF4CC38A),
    warn = Color(0xFFF0B429),
    error = Color(0xFFF2666D),
)

val LightPalette = WarpPalette(
    background = Color(0xFFF4F5F7),
    sidebar = Color(0xFFEAECF0),
    surface = Color(0xFFFFFFFF),
    surfaceHover = Color(0xFFE6E8ED),
    border = Color(0xFFD7DAE1),
    text = Color(0xFF15171C),
    muted = Color(0xFF5C6370),
    accent = Color(0xFFE8700F),
    onAccent = Color(0xFFFFFFFF),
    ok = Color(0xFF1A9A5E),
    warn = Color(0xFFB7791F),
    error = Color(0xFFD4343D),
)

val LocalPalette = staticCompositionLocalOf { DarkPalette }

/** Manrope вшит в APK (res/font), как и в Windows-версию. */
val Manrope = FontFamily(
    Font(R.font.manrope_regular, FontWeight.Normal),
    Font(R.font.manrope_medium, FontWeight.Medium),
    Font(R.font.manrope_semibold, FontWeight.SemiBold),
    Font(R.font.manrope_bold, FontWeight.Bold),
)

private fun typography(): Typography {
    val base = Typography()
    fun TextStyle.m() = copy(fontFamily = Manrope)
    return Typography(
        displayLarge = base.displayLarge.m(), displayMedium = base.displayMedium.m(), displaySmall = base.displaySmall.m(),
        headlineLarge = base.headlineLarge.m(), headlineMedium = base.headlineMedium.m(), headlineSmall = base.headlineSmall.m(),
        titleLarge = base.titleLarge.m().copy(fontWeight = FontWeight.Bold), titleMedium = base.titleMedium.m(), titleSmall = base.titleSmall.m(),
        bodyLarge = base.bodyLarge.m(), bodyMedium = base.bodyMedium.m(), bodySmall = base.bodySmall.m(),
        labelLarge = base.labelLarge.m(), labelMedium = base.labelMedium.m(), labelSmall = base.labelSmall.m(),
    )
}

/** Тема следует за системой: светлая или тёмная, без динамических цветов Material You. */
@Composable
fun WarpTheme(content: @Composable () -> Unit) {
    val p = if (isSystemInDarkTheme()) DarkPalette else LightPalette
    val dark = p == DarkPalette
    val common = { s: androidx.compose.material3.ColorScheme ->
        s.copy(
            primary = p.accent, onPrimary = p.onAccent,
            primaryContainer = p.accent.copy(alpha = 0.18f), onPrimaryContainer = p.text,
            secondaryContainer = p.surfaceHover, onSecondaryContainer = p.text,
            background = p.background, onBackground = p.text,
            surface = p.background, onSurface = p.text,
            surfaceVariant = p.surfaceHover, onSurfaceVariant = p.muted,
            surfaceContainerLowest = p.surface, surfaceContainerLow = p.surface,
            surfaceContainer = p.surface, surfaceContainerHigh = p.surface,
            surfaceContainerHighest = p.surfaceHover,
            outline = p.border, outlineVariant = p.border,
            error = p.error, onError = Color.White,
            errorContainer = p.error.copy(alpha = 0.16f), onErrorContainer = p.text,
        )
    }
    val scheme = common(if (dark) darkColorScheme() else lightColorScheme())
    CompositionLocalProvider(LocalPalette provides p) {
        MaterialTheme(colorScheme = scheme, typography = typography(), content = content)
    }
}
