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
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import kotlinx.coroutines.delay

class MainActivity : ComponentActivity() {
    private val vpnPermission = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        if (it.resultCode == RESULT_OK) startVpn()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        connectIfRequested()
    }

    /** Плитка в шторке просит подключиться, а разрешение на VPN ещё не выдано. */
    private fun connectIfRequested() {
        if (intent.getBooleanExtra(EXTRA_CONNECT, false)) {
            intent.removeExtra(EXTRA_CONNECT)
            val phase = AppState.ui.value.phase
            if (phase != Phase.CONNECTED && phase != Phase.BUSY) toggle()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        AppState.init(this)
        connectIfRequested()
        if (Preferences(this).autoConnect && VpnService.prepare(this) == null && AppState.ui.value.phase == Phase.IDLE) startVpn()
        UpdateState.checkOnStart(this)
        setContent {
            WarpTheme {
                Screen(
                    onToggle = ::toggle,
                    onAuto = { Selector.start(this) },
                )
                UpdateDialog()
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

    companion object {
        const val EXTRA_CONNECT = "dev.warpunblock.CONNECT"
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

/** Разделы — те же, что в боковом меню Windows-версии. */
private val Tabs = listOf(
    "Связь" to R.drawable.ic_lucide_power,
    "Профили" to R.drawable.ic_lucide_route,
    "Списки" to R.drawable.ic_lucide_list_filter,
    "Настройки" to R.drawable.ic_lucide_settings_2,
    "Журнал" to R.drawable.ic_lucide_scroll_text,
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun Screen(onToggle: () -> Unit, onAuto: () -> Unit, initialTab: Int = 0) {
    val p = LocalPalette.current
    val ui by AppState.ui.collectAsState()
    var tab by remember { mutableIntStateOf(initialTab) }
    val active = ui.phase == Phase.CONNECTED || ui.phase == Phase.BUSY
    val snackbar = remember { SnackbarHostState() }
    val color = stateColor(ui)
    // Всплывашки с итогом, как в Windows-версии: подключено, отключено, ошибка.
    var previous by remember { mutableStateOf(ui.phase) }
    LaunchedEffect(ui.phase, ui.title) {
        val message = when {
            ui.phase == previous -> null
            ui.phase == Phase.CONNECTED -> "Подключено"
            ui.phase == Phase.ERROR -> ui.title
            ui.phase == Phase.IDLE && previous == Phase.CONNECTED -> "Отключено"
            else -> null
        }
        previous = ui.phase
        if (message != null) {
            snackbar.currentSnackbarData?.dismiss()
            snackbar.showSnackbar(message)
        }
    }
    Scaffold(
        containerColor = p.background,
        topBar = {
            TopAppBar(
                colors = TopAppBarDefaults.topAppBarColors(containerColor = p.sidebar),
                title = {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Lucide(R.drawable.ic_lucide_shield_check, p.accent, 20.dp)
                        Text("WARP Unblock", fontSize = 16.sp, fontWeight = FontWeight.Bold, color = p.text)
                    }
                },
                actions = {
                    Row(Modifier.padding(end = 12.dp)) {
                        StatusPill(
                            when (ui.phase) {
                                Phase.CONNECTED -> if (ui.title.startsWith("Нет связи")) "Нет связи" else "Работает"
                                Phase.BUSY -> "Подключаю…"
                                Phase.ERROR -> "Ошибка"
                                Phase.IDLE -> "Отключено"
                            },
                            color,
                            emphasized = ui.phase != Phase.IDLE,
                        )
                    }
                },
            )
        },
        bottomBar = {
            NavigationBar(containerColor = p.sidebar) {
                Tabs.forEachIndexed { index, (title, icon) ->
                    NavigationBarItem(
                        selected = tab == index,
                        onClick = { tab = index },
                        icon = { Lucide(icon, if (tab == index) p.accent else p.muted, 22.dp) },
                        label = {
                            Text(
                                title,
                                fontSize = 11.sp,
                                fontWeight = if (tab == index) FontWeight.SemiBold else FontWeight.Normal,
                                maxLines = 1,
                            )
                        },
                        colors = NavigationBarItemDefaults.colors(
                            indicatorColor = p.surfaceHover,
                            selectedTextColor = p.text,
                            unselectedTextColor = p.muted,
                        ),
                    )
                }
            }
        },
        snackbarHost = {
            SnackbarHost(snackbar) { data ->
                Row(
                    Modifier
                        .padding(horizontal = 16.dp, vertical = 8.dp)
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(12.dp))
                        .background(p.surface)
                        .border(1.dp, p.border, RoundedCornerShape(12.dp))
                        .padding(horizontal = 14.dp, vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Dot(color, 9.dp)
                    Text(data.visuals.message, fontSize = 14.sp, color = p.text)
                }
            }
        },
    ) { padding ->
        // Страница появляется с коротким сдвигом снизу, как в Windows-версии.
        AnimatedContent(
            targetState = tab,
            transitionSpec = {
                (fadeIn(tween(180)) + slideInVertically(tween(180)) { it / 40 }) togetherWith fadeOut(tween(90))
            },
            label = "page",
            modifier = Modifier.padding(padding),
        ) { page ->
            Column(
                Modifier
                    .fillMaxSize()
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = 16.dp, vertical = 12.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                when (page) {
                    0 -> Home(ui, onToggle)
                    1 -> Profiles(ui, onAuto)
                    2 -> ListsPage(active)
                    3 -> SettingsPage(active)
                    else -> LogPage()
                }
            }
        }
    }
}

@Composable
private fun Home(ui: UiState, onToggle: () -> Unit) {
    val p = LocalPalette.current
    val latency by AppState.delay.collectAsState()
    val traffic by AppState.traffic.collectAsState()
    val since by AppState.since.collectAsState()
    val now by produceState(System.currentTimeMillis()) {
        while (true) {
            delay(1000)
            value = System.currentTimeMillis()
        }
    }
    val context = LocalContext.current
    val strategies = remember { Strategies.load(context) }
    val index by AppState.strategyIndex.collectAsState()
    val hint = when {
        ui.detail.isNotEmpty() -> ui.detail
        ui.phase == Phase.BUSY -> "Нажмите, чтобы отменить"
        ui.phase == Phase.CONNECTED -> "Нажмите, чтобы отключиться"
        else -> "Нажмите, чтобы подключиться"
    }
    // Светящаяся сетка за кнопкой: цвет состояния, при подключении пятна двигаются быстрее.
    val healthy = ui.phase == Phase.CONNECTED && !ui.title.startsWith("Нет связи")
    val glowColor = when {
        healthy -> p.accent
        ui.phase == Phase.BUSY -> androidx.compose.ui.graphics.lerp(p.accent, p.warn, 0.45f)
        ui.phase == Phase.CONNECTED || ui.phase == Phase.ERROR -> p.error
        else -> p.accent
    }
    WarpCard(
        padding = 22.dp,
        backdrop = {
            GlowGrid(
                mode = when {
                    healthy -> GlowMode.ON
                    ui.phase == Phase.BUSY -> GlowMode.WAVES
                    ui.phase == Phase.CONNECTED || ui.phase == Phase.ERROR -> GlowMode.ERROR
                    else -> GlowMode.IDLE
                },
                color = glowColor,
                modifier = Modifier.matchParentSize(),
            )
        },
    ) {
        Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
            PowerButton(ui, onToggle)
            Text(ui.title, fontSize = 22.sp, fontWeight = FontWeight.Bold, color = p.text, textAlign = TextAlign.Center, modifier = Modifier.padding(top = 6.dp))
            Text(hint, fontSize = 13.sp, color = p.muted, textAlign = TextAlign.Center)
        }
        ui.batch?.let { (done, total, failed) ->
            Row(Modifier.fillMaxWidth().padding(top = 6.dp), horizontalArrangement = Arrangement.SpaceBetween) {
                Text("Подбор профиля", fontSize = 13.sp, color = p.muted)
                Text("проверено $done из $total" + if (failed > 0) " · не прошло $failed" else "", fontSize = 13.sp, color = p.text)
            }
            LinearProgressIndicator(
                progress = { done.toFloat() / total.coerceAtLeast(1) },
                modifier = Modifier.fillMaxWidth().height(8.dp).clip(RoundedCornerShape(4.dp)),
                color = p.accent,
                trackColor = p.surfaceHover,
            )
        }
        Row(Modifier.fillMaxWidth().padding(top = 6.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Metric("Время работы", since?.let { uptime(now - it) } ?: "—", R.drawable.ic_lucide_clock, Modifier.weight(1f))
            Metric("Пинг WARP", latency?.let { "$it мс" } ?: "—", R.drawable.ic_lucide_activity, Modifier.weight(1f))
        }
        Metric("Профиль", strategies[index.coerceIn(strategies.indices)].name.replace("WARP · ", ""), R.drawable.ic_lucide_route, Modifier.fillMaxWidth())
    }
    WarpCard {
        Text("Трафик через VPN", fontSize = 13.sp, color = p.muted)
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Lucide(R.drawable.ic_lucide_arrow_down, p.ok, 18.dp)
                Text(bytes(traffic.second), fontSize = 20.sp, fontWeight = FontWeight.SemiBold, color = p.text)
            }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Lucide(R.drawable.ic_lucide_arrow_up, p.accent, 18.dp)
                Text(bytes(traffic.first), fontSize = 20.sp, fontWeight = FontWeight.SemiBold, color = p.text)
            }
        }
    }
}

@Composable
private fun Profiles(ui: UiState, onAuto: () -> Unit) {
    val p = LocalPalette.current
    val context = LocalContext.current
    val strategies = remember { Strategies.load(context) }
    val index by AppState.strategyIndex.collectAsState()
    val results by AppState.results.collectAsState()
    val locked = ui.phase == Phase.BUSY || ui.phase == Phase.CONNECTED
    var hasAccount by remember { mutableStateOf(WarpAccount.load(context.filesDir) != null) }
    val scanner = rememberLauncherForActivityResult(ScanContract()) { result ->
        result.contents?.let { text ->
            runCatching { WarpAccount.parseWgcf(text).save(context.filesDir) }
                .onSuccess { hasAccount = true; AppState.log("Профиль WARP импортирован из QR"); AppState.set(UiState(Phase.IDLE, "Профиль импортирован")) }
                .onFailure { AppState.set(UiState(Phase.ERROR, "QR не содержит профиль WARP", it.message ?: "")) }
        }
    }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) {
            runCatching {
                val text = context.contentResolver.openInputStream(uri)!!.bufferedReader().use { it.readText() }
                WarpAccount.parseWgcf(text).save(context.filesDir)
            }.onSuccess {
                hasAccount = true
                AppState.log("Профиль WARP импортирован")
                AppState.set(UiState(Phase.IDLE, "Профиль импортирован", "Можно подключаться"))
            }.onFailure {
                AppState.log("Импорт не удался: ${it.message}")
                AppState.set(UiState(Phase.ERROR, "Импорт не удался", it.message ?: ""))
            }
        }
    }
    SectionTitle("Профили WARP", "Как WireGuard доезжает до WARP. Проверка идёт без VPN — перед тестом отключите другой VPN.")
    WarpCard {
        Text("Аккаунт WARP", fontSize = 13.sp, color = p.muted)
        Text(
            if (hasAccount) "Есть, готов к подключению" else "Нет — зарегистрируется при подключении",
            fontSize = 16.sp,
            fontWeight = FontWeight.SemiBold,
            color = p.text,
        )
        Text(
            "Если регистрация не проходит, импортируйте wgcf-profile.conf или QR из Windows-версии.",
            fontSize = 13.sp,
            color = p.muted,
        )
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            FilledTonalButton(onClick = { picker.launch(arrayOf("*/*")) }, enabled = !locked) { Text("Файл") }
            FilledTonalButton(
                onClick = { scanner.launch(ScanOptions().setDesiredBarcodeFormats(ScanOptions.QR_CODE).setPrompt("Профиль WARP с компьютера").setBeepEnabled(false)) },
                enabled = !locked,
            ) { Text("QR") }
            if (hasAccount) {
                TextButton(onClick = { WarpAccount.delete(context.filesDir); hasAccount = false }, enabled = !locked) { Text("Сбросить") }
            }
        }
    }
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        FilledTonalButton(onClick = { Selector.start(context, best = true) }, enabled = !locked, modifier = Modifier.weight(1f)) { Text("Выбрать лучший") }
        androidx.compose.material3.Button(onClick = onAuto, enabled = !locked, modifier = Modifier.weight(1f)) { Text("Подобрать") }
    }
    if (locked) Text("Менять профиль можно при отключённом соединении.", fontSize = 13.sp, color = p.muted)
    ProfileStats.load(context)[index]?.let { stats ->
        Text("Текущий: успешно ${stats.successes}/${stats.attempts} · средняя задержка ${stats.totalMs / stats.successes.coerceAtLeast(1)} мс", fontSize = 13.sp, color = p.muted)
    }
    strategies.forEachIndexed { i, strategy ->
        val delay = results[i]
        val (color, note) = when {
            delay == null -> p.border to ""
            delay > 0 -> p.ok to "$delay мс"
            else -> p.error to "не прошёл"
        }
        val selected = i == index
        WarpCard(
            border = if (selected) p.accent else null,
            onClick = if (locked) null else ({ AppState.selectStrategy(i) }),
            padding = 14.dp,
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Dot(color, 9.dp)
                Column(Modifier.weight(1f)) {
                    Text(strategy.name, fontSize = 15.sp, fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal, color = p.text)
                    Text("${strategy.endpoint}:${strategy.port} · padding ${strategy.jc}", fontSize = 12.sp, color = p.muted)
                }
                if (note.isNotEmpty()) Text(note, fontSize = 13.sp, color = color)
            }
        }
    }
}

@Composable
private fun LogPage() {
    val p = LocalPalette.current
    val logs by AppState.logs.collectAsState()
    val context = LocalContext.current
    SectionTitle("Журнал", "Журнал может содержать домены и IP. Профиль WARP и QR с ключом не публикуйте.")
    FilledTonalButton(onClick = {
        val text = "WARP Unblock Android ${BuildConfig.VERSION_NAME}\n" + logs.joinToString("\n") + "\n\n" + Core(context).logTail(120)
        context.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"; putExtra(Intent.EXTRA_SUBJECT, "Журнал WARP Unblock"); putExtra(Intent.EXTRA_TEXT, text)
        }, "Поделиться журналом"))
    }) {
        Lucide(R.drawable.ic_lucide_share_2, p.text, 18.dp)
        Spacer(Modifier.width(8.dp))
        Text("Поделиться журналом")
    }
    WarpCard {
        Text("События (новые сверху)", fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = p.text)
        if (logs.isEmpty()) {
            Text("Событий пока нет", fontSize = 14.sp, color = p.muted)
        } else {
            Text(logs.takeLast(80).reversed().joinToString("\n"), fontSize = 12.sp, fontFamily = FontFamily.Monospace, color = p.muted)
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
