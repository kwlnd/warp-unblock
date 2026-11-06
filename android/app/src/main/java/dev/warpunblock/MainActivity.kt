package dev.warpunblock

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CenterAlignedTopAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import kotlinx.coroutines.delay

private val Brand = Color(0xFFF6821F)
private val Ok = Color(0xFF4CC38A)
private val Bad = Color(0xFFF2666D)

/** Material You, как в FlClash/FlClashX: системные динамические цвета, а где их нет — оранжевая тема WARP. */
@Composable
private fun WarpTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val context = LocalContext.current
    val scheme = when {
        Build.VERSION.SDK_INT >= 31 -> if (dark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        dark -> darkColorScheme(primary = Brand, onPrimary = Color(0xFF2B1600), primaryContainer = Color(0xFF5C3100), onPrimaryContainer = Color(0xFFFFDCC2))
        else -> lightColorScheme(primary = Color(0xFFA14F00), primaryContainer = Color(0xFFFFDCC2), onPrimaryContainer = Color(0xFF331700))
    }
    MaterialTheme(colorScheme = scheme, content = content)
}

class MainActivity : ComponentActivity() {
    private val vpnPermission = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        if (it.resultCode == RESULT_OK) startVpn()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        AppState.init(this)
        setContent {
            WarpTheme {
                Screen(
                    onToggle = ::toggle,
                    onAuto = { Selector.start(this) },
                )
            }
        }
    }

    private fun toggle() {
        val state = AppState.ui.value
        when {
            Selector.running -> Selector.cancel()
            state.phase == Phase.CONNECTED || state.phase == Phase.BUSY -> stopVpn()
            else -> {
                if (Build.VERSION.SDK_INT >= 33 &&
                    checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
                ) {
                    requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 1)
                }
                val intent = VpnService.prepare(this)
                if (intent != null) vpnPermission.launch(intent) else startVpn()
            }
        }
    }

    private fun startVpn() {
        ContextCompat.startForegroundService(
            this,
            Intent(this, WarpVpnService::class.java).setAction(WarpVpnService.ACTION_START),
        )
    }

    private fun stopVpn() {
        startService(Intent(this, WarpVpnService::class.java).setAction(WarpVpnService.ACTION_STOP))
    }
}


@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun Screen(onToggle: () -> Unit, onAuto: () -> Unit) {
    val ui by AppState.ui.collectAsState()
    var tab by remember { mutableIntStateOf(0) }
    val active = ui.phase == Phase.CONNECTED || ui.phase == Phase.BUSY
    Scaffold(
        topBar = {
            CenterAlignedTopAppBar(title = { Text("WARP Unblock", fontWeight = FontWeight.SemiBold) })
        },
        bottomBar = {
            NavigationBar {
                listOf("Панель", "Профили", "Журнал").forEachIndexed { index, title ->
                    NavigationBarItem(
                        selected = tab == index,
                        onClick = { tab = index },
                        icon = { NavIcon(index, MaterialTheme.colorScheme.onSurfaceVariant) },
                        label = { Text(title) },
                    )
                }
            }
        },
        floatingActionButton = { ConnectButton(ui, active, onToggle) },
    ) { padding ->
        Column(
            Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp)
                .padding(bottom = 96.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            when (tab) {
                0 -> Dashboard(ui)
                1 -> Profiles(ui, onAuto)
                else -> LogPage()
            }
        }
    }
}

@Composable
private fun ConnectButton(ui: UiState, active: Boolean, onToggle: () -> Unit) {
    val since by AppState.since.collectAsState()
    val now by produceState(System.currentTimeMillis()) {
        while (true) {
            delay(1000)
            value = System.currentTimeMillis()
        }
    }
    val label = when {
        ui.phase == Phase.CONNECTED -> since?.let { uptime(now - it) } ?: "Отключить"
        ui.phase == Phase.BUSY -> "Отменить"
        else -> "Подключить"
    }
    ExtendedFloatingActionButton(
        onClick = onToggle,
        containerColor = if (active) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.primaryContainer,
        contentColor = if (active) MaterialTheme.colorScheme.onSecondaryContainer else MaterialTheme.colorScheme.onPrimaryContainer,
        shape = RoundedCornerShape(20.dp),
        icon = { PlayStopIcon(stop = active) },
        text = { Text(label, fontWeight = FontWeight.SemiBold, fontSize = 16.sp) },
    )
}

@Composable
private fun PlayStopIcon(stop: Boolean) {
    val color = LocalContentColorCompat()
    Canvas(Modifier.size(24.dp)) {
        if (stop) {
            drawRoundRect(color, Offset(size.width * 0.22f, size.height * 0.22f), Size(size.width * 0.56f, size.height * 0.56f), CornerRadius(4f))
        } else {
            val path = Path().apply {
                moveTo(size.width * 0.28f, size.height * 0.18f)
                lineTo(size.width * 0.82f, size.height * 0.5f)
                lineTo(size.width * 0.28f, size.height * 0.82f)
                close()
            }
            drawPath(path, color)
        }
    }
}

@Composable
private fun LocalContentColorCompat(): Color = androidx.compose.material3.LocalContentColor.current

@Composable
private fun NavIcon(index: Int, color: Color) {
    Canvas(Modifier.size(24.dp)) {
        val stroke = Stroke(width = 2.4f)
        when (index) {
            0 -> {
                val cell = size.width * 0.36f
                for (x in 0..1) for (y in 0..1) {
                    drawRoundRect(
                        color,
                        Offset(size.width * (0.1f + 0.44f * x), size.height * (0.1f + 0.44f * y)),
                        Size(cell, cell),
                        CornerRadius(5f),
                    )
                }
            }
            1 -> {
                for (row in 0..2) {
                    val y = size.height * (0.24f + 0.26f * row)
                    drawCircle(color, 2.6f, Offset(size.width * 0.16f, y))
                    drawLine(color, Offset(size.width * 0.34f, y), Offset(size.width * 0.9f, y), 2.4f)
                }
            }
            else -> {
                drawRoundRect(color, Offset(size.width * 0.2f, size.height * 0.1f), Size(size.width * 0.6f, size.height * 0.8f), CornerRadius(5f), style = stroke)
                for (row in 0..2) {
                    val y = size.height * (0.34f + 0.18f * row)
                    drawLine(color, Offset(size.width * 0.34f, y), Offset(size.width * 0.66f, y), 2.2f)
                }
            }
        }
    }
}

@Composable
private fun Tile(
    modifier: Modifier = Modifier,
    container: Color = MaterialTheme.colorScheme.surfaceContainerHigh,
    border: BorderStroke? = null,
    onClick: (() -> Unit)? = null,
    content: @Composable () -> Unit,
) {
    val colors = CardDefaults.cardColors(containerColor = container)
    val shape = RoundedCornerShape(28.dp)
    val inner: @Composable () -> Unit = {
        Column(Modifier.fillMaxWidth().padding(20.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) { content() }
    }
    if (onClick != null) {
        Card(onClick = onClick, modifier = modifier, shape = shape, colors = colors, border = border) { inner() }
    } else {
        Card(modifier = modifier, shape = shape, colors = colors, border = border) { inner() }
    }
}

@Composable
private fun Dashboard(ui: UiState) {
    val latency by AppState.delay.collectAsState()
    val traffic by AppState.traffic.collectAsState()
    val since by AppState.since.collectAsState()
    val now by produceState(System.currentTimeMillis()) {
        while (true) {
            delay(1000)
            value = System.currentTimeMillis()
        }
    }
    val scheme = MaterialTheme.colorScheme
    val lost = ui.phase == Phase.CONNECTED && ui.title.startsWith("Нет связи")
    val (container, content) = when {
        ui.phase == Phase.CONNECTED && !lost -> scheme.primaryContainer to scheme.onPrimaryContainer
        ui.phase == Phase.ERROR || lost -> scheme.errorContainer to scheme.onErrorContainer
        else -> scheme.surfaceContainerHigh to scheme.onSurface
    }
    val dot = when (ui.phase) {
        Phase.CONNECTED -> if (lost) Bad else Ok
        Phase.BUSY -> Brand
        Phase.ERROR -> Bad
        Phase.IDLE -> scheme.outline
    }
    Tile(container = container) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Box(Modifier.size(12.dp).clip(CircleShape).background(dot))
            Column {
                Text(ui.title, fontSize = 22.sp, fontWeight = FontWeight.SemiBold, color = content)
                if (ui.detail.isNotEmpty()) Text(ui.detail, fontSize = 14.sp, color = content.copy(alpha = 0.75f))
            }
        }
        ui.batch?.let { (done, total, failed) ->
            Row(Modifier.fillMaxWidth().padding(top = 8.dp), horizontalArrangement = Arrangement.SpaceBetween) {
                Text("Подбор профиля", fontSize = 14.sp, color = content.copy(alpha = 0.75f))
                Text(
                    "проверено $done из $total" + if (failed > 0) " · не прошло $failed" else "",
                    fontSize = 14.sp,
                    color = content,
                )
            }
            LinearProgressIndicator(
                progress = { done.toFloat() / total.coerceAtLeast(1) },
                modifier = Modifier.fillMaxWidth().height(8.dp).clip(RoundedCornerShape(4.dp)),
            )
        }
    }
    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Tile(Modifier.weight(1f)) {
            Text("Задержка", fontSize = 14.sp, color = scheme.onSurfaceVariant)
            Text(latency?.let { "$it мс" } ?: "—", fontSize = 28.sp, fontWeight = FontWeight.SemiBold)
        }
        Tile(Modifier.weight(1f)) {
            Text("В сети", fontSize = 14.sp, color = scheme.onSurfaceVariant)
            Text(since?.let { uptime(now - it) } ?: "—", fontSize = 28.sp, fontWeight = FontWeight.SemiBold)
        }
    }
    Tile {
        Text("Трафик через WARP и DIRECT", fontSize = 14.sp, color = scheme.onSurfaceVariant)
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Column {
                Text("↓ получено", fontSize = 13.sp, color = scheme.onSurfaceVariant)
                Text(bytes(traffic.second), fontSize = 22.sp, fontWeight = FontWeight.SemiBold)
            }
            Column(horizontalAlignment = Alignment.End) {
                Text("↑ отправлено", fontSize = 13.sp, color = scheme.onSurfaceVariant)
                Text(bytes(traffic.first), fontSize = 22.sp, fontWeight = FontWeight.SemiBold)
            }
        }
    }
    val context = LocalContext.current
    val strategies = remember { Strategies.load(context) }
    val index by AppState.strategyIndex.collectAsState()
    Tile {
        Text("Профиль WARP", fontSize = 14.sp, color = scheme.onSurfaceVariant)
        Text(strategies[index.coerceIn(strategies.indices)].name, fontSize = 18.sp, fontWeight = FontWeight.Medium)
    }
}

@Composable
private fun Profiles(ui: UiState, onAuto: () -> Unit) {
    val context = LocalContext.current
    val strategies = remember { Strategies.load(context) }
    val index by AppState.strategyIndex.collectAsState()
    val results by AppState.results.collectAsState()
    val locked = ui.phase == Phase.BUSY || ui.phase == Phase.CONNECTED
    val scheme = MaterialTheme.colorScheme
    FilledTonalButton(
        onClick = onAuto,
        enabled = !locked,
        modifier = Modifier.fillMaxWidth().height(52.dp),
        shape = RoundedCornerShape(20.dp),
    ) { Text("Подобрать автоматически", fontSize = 16.sp) }
    Text(
        "Проверка идёт без VPN, по очереди. Перед тестом отключите другой VPN.",
        fontSize = 13.sp,
        color = scheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 8.dp),
    )
    strategies.forEachIndexed { i, strategy ->
        val delay = results[i]
        val (color, note) = when {
            delay == null -> scheme.outlineVariant to ""
            delay > 0 -> Ok to "$delay мс"
            else -> Bad to "не прошёл"
        }
        val selected = i == index
        Tile(
            container = if (selected) scheme.primaryContainer else scheme.surfaceContainerHigh,
            border = if (selected) BorderStroke(2.dp, scheme.primary) else null,
            onClick = if (locked) null else ({ AppState.selectStrategy(i) }),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Box(Modifier.size(12.dp).clip(CircleShape).background(color))
                Column(Modifier.weight(1f)) {
                    Text(
                        strategy.name,
                        fontSize = 16.sp,
                        fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal,
                        color = if (selected) scheme.onPrimaryContainer else scheme.onSurface,
                    )
                    Text(
                        "${strategy.endpoint}:${strategy.port} · padding ${strategy.jc}",
                        fontSize = 13.sp,
                        color = scheme.onSurfaceVariant,
                    )
                }
                if (note.isNotEmpty()) Text(note, fontSize = 14.sp, color = color)
            }
        }
    }
}

@Composable
private fun LogPage() {
    val logs by AppState.logs.collectAsState()
    Tile {
        Text("Журнал", fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
        if (logs.isEmpty()) {
            Text("Событий пока нет", fontSize = 14.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
        } else {
            Text(
                logs.takeLast(80).joinToString("\n"),
                fontSize = 13.sp,
                fontFamily = FontFamily.Monospace,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

private fun uptime(millis: Long): String {
    val total = (millis / 1000).coerceAtLeast(0)
    return "%02d:%02d:%02d".format(total / 3600, total % 3600 / 60, total % 60)
}

private fun bytes(value: Long): String = when {
    value >= 1L shl 30 -> "%.2f ГБ".format(value / (1L shl 30).toDouble())
    value >= 1L shl 20 -> "%.1f МБ".format(value / (1L shl 20).toDouble())
    value >= 1L shl 10 -> "%.0f КБ".format(value / 1024.0)
    else -> "$value Б"
}
