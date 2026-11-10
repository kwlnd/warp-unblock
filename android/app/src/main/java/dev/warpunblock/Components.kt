package dev.warpunblock

import androidx.annotation.DrawableRes
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Spacer
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.drawscope.CanvasDrawScope
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.platform.LocalInspectionMode
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.floor
import kotlin.math.exp
import kotlin.math.pow
import kotlin.math.sin
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** Карточка в стиле Windows-версии: поверхность, тонкая рамка, скругление 14 dp. */
@Composable
fun WarpCard(
    modifier: Modifier = Modifier,
    border: Color? = null,
    onClick: (() -> Unit)? = null,
    padding: Dp = 18.dp,
    /** Слой под содержимым, но над фоном карточки (например, светящаяся сетка). */
    backdrop: (@Composable BoxScope.() -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    val p = LocalPalette.current
    val shape = RoundedCornerShape(14.dp)
    Box(
        modifier
            .fillMaxWidth()
            .clip(shape)
            .background(p.surface)
            .border(BorderStroke(1.dp, border ?: p.border), shape)
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier),
    ) {
        backdrop?.invoke(this)
        Column(
            Modifier.fillMaxWidth().padding(padding),
            verticalArrangement = Arrangement.spacedBy(10.dp),
            content = content,
        )
    }
}

/** Тесты снимков экранов выключают бесконечные анимации, чтобы кадр «успокоился». */
object Motion {
    @Volatile var frozen = false
}

enum class GlowMode { IDLE, WAVES, ON, ERROR }

/** Детерминированный шум 0..1 (как в Windows-версии): направления волн и «звёзды». */
internal fun noise(x: Int, y: Int, salt: Int): Float {
    var n = x * 0x27d4eb2d + y * 0x165667b1 + salt * -0x61c88647
    n = n xor (n ushr 15); n *= -0x7a143595
    n = n xor (n ushr 13); n *= -0x3d4d51cb
    n = n xor (n ushr 16)
    return (n.toLong() and 0xffffffffL).toFloat() / 0xffffffffL.toFloat()
}

/**
 * Светящаяся сетка, как в Windows-версии. Простой: плывут мягкие пятна. Подключение: резкие
 * волны в случайном направлении. Подключено: сетка ровно светится, подмигивают «звёзды».
 */
@Composable
fun GlowGrid(mode: GlowMode, color: Color, modifier: Modifier = Modifier) {
    val dark = LocalPalette.current == DarkPalette
    val still = LocalInspectionMode.current || Motion.frozen
    // ~30 кадров в секунду: чаще для такой анимации не нужно.
    val time by produceState(if (still) 4f else 0f) {
        if (still) return@produceState // предпросмотр и снимки экранов: один статичный кадр
        val start = withFrameNanos { it }
        var last = 0L
        while (true) withFrameNanos {
            if (it - last >= 33_000_000L) {
                last = it
                value = (it - start) / 1_000_000_000f
            }
        }
    }
    val star = androidx.compose.ui.graphics.lerp(color, Color(0xFFFFE7D1), 0.6f)
    val floor = if (dark) 0.045f else 0.035f
    Spacer(
        modifier.drawWithCache {
            // Неподвижная часть запекается один раз на размер и цвет: тусклая сетка и ровное
            // свечение «подключено». Каждый кадр рисуются картинка и только живые клетки.
            val step = 7.dp.toPx()
            val cell = 3.6.dp.toPx()
            val corner = CornerRadius(1.dp.toPx())
            val w = size.width
            val h = size.height
            val cols = (w / step).toInt()
            val rows = (h / step).toInt()
            val ox = (w - (cols - 1) * step - cell) / 2
            val oy = (h - (rows - 1) * step - cell) / 2
            val xs = FloatArray(cols * rows)
            val ys = FloatArray(cols * rows)
            val seeds = FloatArray(cols * rows)
            for (row in 0 until rows) for (col in 0 until cols) {
                val i = row * cols + col
                xs[i] = ox + col * step
                ys[i] = oy + row * step
                seeds[i] = noise(col, row, 3)
            }
            fun bake(alpha: Float): ImageBitmap {
                val bitmap = ImageBitmap(w.toInt().coerceAtLeast(1), h.toInt().coerceAtLeast(1))
                CanvasDrawScope().draw(this, layoutDirection, androidx.compose.ui.graphics.Canvas(bitmap), size) {
                    for (i in xs.indices) drawRoundRect(color.copy(alpha = alpha), Offset(xs[i], ys[i]), Size(cell, cell), corner)
                }
                return bitmap
            }
            val baked = bake(if (mode == GlowMode.ON) 0.52f else floor)
            val scale = minOf(w, h).coerceAtLeast(1f)
            onDrawBehind {
                drawImage(baked)
                val t = time
                val blobs = arrayOf(
                    floatArrayOf(0.5f + 0.34f * sin(t * 0.37f), 0.38f + 0.22f * sin(t * 0.53f + 1f), 0.3f, 1f),
                    floatArrayOf(0.5f + 0.38f * cos(t * 0.29f + 2f), 0.66f + 0.2f * cos(t * 0.41f), 0.24f, 0.9f),
                    floatArrayOf(0.5f + 0.25f * cos(t * 0.19f), 0.5f + 0.25f * sin(t * 0.19f), 0.16f, 0.55f),
                )
                val period = 0.9f
                val waves = arrayOf(0f, period / 2).map { offset ->
                    val n = floor((t + offset) / period)
                    val phase = (t + offset) / period - n
                    val angle = noise(n.toInt(), 7, 11) * 2 * PI.toFloat()
                    floatArrayOf(cos(angle), sin(angle), -0.25f + 1.5f * phase)
                }
                if (mode == GlowMode.ON) drawRect(color.copy(alpha = 0.04f * (1 + sin(t * 1.1f))))
                for (i in xs.indices) {
                    val x = xs[i]
                    val y = ys[i]
                    var fill = color
                    val alpha = when (mode) {
                        GlowMode.IDLE, GlowMode.ERROR -> {
                            var glow = 0f
                            for (b in blobs) {
                                val dx = (x - b[0] * w) / (b[2] * scale)
                                val dy = (y - b[1] * h) / (b[2] * scale)
                                glow += b[3] * exp(-(dx * dx + dy * dy))
                            }
                            (if (mode == GlowMode.IDLE) 0.8f else 1f) * glow.coerceAtMost(1f).pow(1.5f)
                        }
                        GlowMode.WAVES -> {
                            val u = (x + cell / 2) / w
                            val v = (y + cell / 2) / h
                            var glow = 0f
                            for (wv in waves) {
                                val d = ((u - 0.5f) * wv[0] + (v - 0.5f) * wv[1]) / 1.42f + 0.5f - wv[2]
                                // Резкий передний край и короткий светящийся хвост.
                                glow += if (d > 0) exp(-(d / 0.012f).pow(2)) else exp(d / 0.09f)
                            }
                            0.95f * glow.coerceAtMost(1f)
                        }
                        GlowMode.ON -> {
                            val seed = seeds[i]
                            if (seed >= 0.07f) continue
                            fill = star
                            val twinkle = sin(t * (1.2f + seed * 20) + seed * 97).coerceAtLeast(0f).pow(10)
                            if (twinkle > 0.02f) 0.4f + 0.6f * twinkle else 0f
                        }
                    }
                    if (alpha < 0.03f) continue
                    drawRoundRect(fill.copy(alpha = alpha.coerceAtMost(1f)), Offset(x, y), Size(cell, cell), corner)
                }
            }
        },
    )
}

@Composable
fun Lucide(@DrawableRes id: Int, tint: Color, size: Dp = 22.dp, modifier: Modifier = Modifier) {
    Icon(painterResource(id), contentDescription = null, tint = tint, modifier = modifier.size(size))
}

@Composable
fun Dot(color: Color, size: Dp = 8.dp) {
    Box(Modifier.size(size).clip(CircleShape).background(color))
}

/** Плашка статуса, как по центру заголовка окна в Windows. */
@Composable
fun StatusPill(text: String, color: Color, emphasized: Boolean) {
    val p = LocalPalette.current
    Row(
        Modifier
            .clip(RoundedCornerShape(50))
            .background(color.copy(alpha = 0.14f))
            .padding(horizontal = 12.dp, vertical = 5.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(7.dp),
    ) {
        Dot(color, 7.dp)
        Text(
            text,
            fontSize = 12.sp,
            fontWeight = FontWeight.SemiBold,
            color = if (emphasized) color else p.muted,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

/** Цвет состояния: общий для плашки, кнопки питания и щита. */
@Composable
fun stateColor(ui: UiState): Color {
    val p = LocalPalette.current
    val lost = ui.phase == Phase.CONNECTED && ui.title.startsWith("Нет связи")
    return when {
        ui.phase == Phase.BUSY -> p.warn
        ui.phase == Phase.ERROR || lost -> p.error
        ui.phase == Phase.CONNECTED -> p.ok
        else -> p.muted
    }
}

/** Круглая кнопка питания; внешнее кольцо дышит, пока идёт подключение или подбор. */
@Composable
fun PowerButton(ui: UiState, onClick: () -> Unit) {
    val p = LocalPalette.current
    val healthy = ui.phase == Phase.CONNECTED && !ui.title.startsWith("Нет связи")
    val (ring, fill, glyph) = when {
        healthy -> Triple(p.accent, p.accent, p.onAccent)
        ui.phase == Phase.BUSY -> Triple(p.warn, p.surfaceHover, p.warn)
        ui.phase == Phase.CONNECTED || ui.phase == Phase.ERROR -> Triple(p.error, p.surfaceHover, p.error)
        else -> Triple(p.border, p.surfaceHover, p.accent)
    }
    val pulse by rememberInfiniteTransition(label = "pulse").animateFloat(
        initialValue = 0.25f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(tween(700, easing = LinearEasing), RepeatMode.Reverse),
        label = "halo",
    )
    Box(Modifier.size(154.dp), contentAlignment = Alignment.Center) {
        Box(
            Modifier
                .size(154.dp)
                .alpha(if (ui.phase == Phase.BUSY) pulse else if (healthy) 0.35f else 0.25f)
                .border(2.dp, ring, CircleShape),
        )
        Box(
            Modifier
                .size(128.dp)
                .clip(CircleShape)
                .background(fill)
                .border(2.dp, ring, CircleShape)
                .clickable(onClick = onClick),
            contentAlignment = Alignment.Center,
        ) {
            Lucide(R.drawable.ic_lucide_power, glyph, 50.dp)
        }
    }
}

@Composable
fun Metric(label: String, value: String, @DrawableRes icon: Int, modifier: Modifier = Modifier) {
    val p = LocalPalette.current
    Row(
        modifier
            .clip(RoundedCornerShape(12.dp))
            .background(p.surfaceHover)
            .padding(12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Lucide(icon, p.muted, 18.dp)
        Column {
            Text(label, fontSize = 12.sp, color = p.muted, maxLines = 1)
            Text(value, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = p.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
fun SectionTitle(title: String, hint: String) {
    val p = LocalPalette.current
    Column(Modifier.padding(top = 8.dp, bottom = 4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(title, fontSize = 22.sp, fontWeight = FontWeight.Bold, color = p.text)
        Text(hint, fontSize = 13.sp, color = p.muted)
    }
}
