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
import androidx.compose.foundation.layout.Box
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
    content: @Composable ColumnScope.() -> Unit,
) {
    val p = LocalPalette.current
    val shape = RoundedCornerShape(14.dp)
    Column(
        modifier
            .fillMaxWidth()
            .clip(shape)
            .background(p.surface)
            .border(BorderStroke(1.dp, border ?: p.border), shape)
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(padding),
        verticalArrangement = Arrangement.spacedBy(10.dp),
        content = content,
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
