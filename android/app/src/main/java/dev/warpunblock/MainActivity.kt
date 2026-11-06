package dev.warpunblock

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat

private val Background = Color(0xFF16171A)
private val Surface = Color(0xFF212328)
private val SurfaceHover = Color(0xFF30333A)
private val Border = Color(0xFF3A3E47)
private val TextColor = Color(0xFFF0F2F5)
private val Muted = Color(0xFFB1B7C2)
private val Accent = Color(0xFFF6821F)
private val OnAccent = Color(0xFF1C1004)
private val Ok = Color(0xFF4CC38A)
private val Warn = Color(0xFFF0B429)
private val Error = Color(0xFFF2666D)

class MainActivity : ComponentActivity() {
    private val vpnPermission = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        if (it.resultCode == RESULT_OK) startVpn()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        AppState.init(this)
        setContent {
            MaterialTheme(
                colorScheme = darkColorScheme(primary = Accent, background = Background, surface = Surface),
            ) {
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

@Composable
private fun Screen(onToggle: () -> Unit, onAuto: () -> Unit) {
    val ui by AppState.ui.collectAsState()
    val logs by AppState.logs.collectAsState()
    Column(
        Modifier
            .fillMaxSize()
            .background(Background)
            .padding(horizontal = 20.dp, vertical = 16.dp),
    ) {
        Column(
            Modifier
                .weight(1f)
                .verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            StatusCard(ui)
            ProfileCard(ui, onAuto)
            Card {
                Text("Журнал", fontWeight = FontWeight.SemiBold, fontSize = 16.sp, color = TextColor)
                if (logs.isEmpty()) {
                    Text("Событий пока нет", fontSize = 14.sp, color = Muted)
                } else {
                    Text(
                        logs.takeLast(30).joinToString("\n"),
                        fontSize = 13.sp,
                        fontFamily = FontFamily.Monospace,
                        color = Muted,
                    )
                }
            }
        }
        Spacer16()
        val label = when {
            ui.phase == Phase.CONNECTED -> "Отключить"
            ui.phase == Phase.BUSY -> "Отменить"
            else -> "Подключить"
        }
        val active = ui.phase == Phase.CONNECTED || ui.phase == Phase.BUSY
        Button(
            onClick = onToggle,
            modifier = Modifier
                .fillMaxWidth()
                .height(58.dp),
            shape = RoundedCornerShape(12.dp),
            colors = ButtonDefaults.buttonColors(
                containerColor = if (active) SurfaceHover else Accent,
                contentColor = if (active) TextColor else OnAccent,
            ),
            border = BorderStroke(1.dp, if (active) Border else Accent),
        ) {
            Text(label, fontSize = 19.sp, fontWeight = FontWeight.Bold)
        }
    }
}

@Composable
private fun Spacer16() {
    Box(Modifier.height(16.dp))
}

@Composable
private fun Card(content: @Composable () -> Unit) {
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .background(Surface)
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        content()
    }
}

@Composable
private fun StatusCard(ui: UiState) {
    val color = when (ui.phase) {
        Phase.CONNECTED -> if (ui.title.startsWith("Нет связи")) Error else Ok
        Phase.BUSY -> Warn
        Phase.ERROR -> Error
        Phase.IDLE -> Muted
    }
    Card {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Box(Modifier.size(12.dp).clip(CircleShape).background(color))
            Column {
                Text(ui.title, fontSize = 22.sp, fontWeight = FontWeight.SemiBold, color = TextColor)
                if (ui.detail.isNotEmpty()) Text(ui.detail, fontSize = 14.sp, color = Muted)
            }
        }
        ui.batch?.let { (done, total, failed) ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Text("Подбор профиля", fontSize = 14.sp, color = Muted)
                Text(
                    "проверено $done из $total" + if (failed > 0) " · не прошло $failed" else "",
                    fontSize = 14.sp,
                    color = TextColor,
                )
            }
            LinearProgressIndicator(
                progress = { done.toFloat() / total.coerceAtLeast(1) },
                modifier = Modifier.fillMaxWidth().height(8.dp).clip(RoundedCornerShape(4.dp)),
                color = Accent,
                trackColor = Background,
            )
        }
    }
}

@Composable
private fun ProfileCard(ui: UiState, onAuto: () -> Unit) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val strategies = remember { Strategies.load(context) }
    val index by AppState.strategyIndex.collectAsState()
    val results by AppState.results.collectAsState()
    var open by remember { mutableStateOf(false) }
    val locked = ui.phase == Phase.BUSY || ui.phase == Phase.CONNECTED
    Card {
        Text("Профиль WARP", fontWeight = FontWeight.SemiBold, fontSize = 16.sp, color = TextColor)
        Box {
            OutlinedButton(
                onClick = { open = true },
                enabled = !locked,
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(10.dp),
            ) {
                ProfileRow(strategies[index.coerceIn(strategies.indices)], results[index])
            }
            DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                strategies.forEachIndexed { i, strategy ->
                    DropdownMenuItem(
                        text = { ProfileRow(strategy, results[i]) },
                        onClick = {
                            AppState.selectStrategy(i)
                            open = false
                        },
                    )
                }
            }
        }
        Button(
            onClick = onAuto,
            enabled = !locked,
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(10.dp),
            colors = ButtonDefaults.buttonColors(containerColor = SurfaceHover, contentColor = TextColor),
        ) {
            Text("Подобрать автоматически")
        }
        Text("Проверка без VPN. Перед тестом отключите другой VPN.", fontSize = 14.sp, color = Muted)
    }
}

@Composable
private fun ProfileRow(strategy: Strategy, delay: Int?) {
    val (color, note) = when {
        delay == null -> Border to ""
        delay > 0 -> Ok to " · $delay мс"
        else -> Error to " · не прошёл"
    }
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Box(Modifier.size(10.dp).clip(CircleShape).background(color))
        Text(strategy.name + note, fontSize = 15.sp, color = TextColor)
    }
}
