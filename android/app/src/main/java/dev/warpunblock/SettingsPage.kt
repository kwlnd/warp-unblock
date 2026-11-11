package dev.warpunblock

import android.content.Intent
import android.net.Uri
import android.provider.Settings
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Редактор доменов — отдельный раздел, как «Списки доменов» в Windows-версии. */
@Composable
internal fun ListsPage(locked: Boolean) {
    val p = LocalPalette.current
    val context = LocalContext.current
    var tab by remember { mutableIntStateOf(0) }
    var proxy by remember { mutableStateOf(DomainLists.read(context, "proxy-domains.txt")) }
    var direct by remember { mutableStateOf(DomainLists.read(context, "direct-domains.txt")) }
    var message by remember { mutableStateOf("") }
    SectionTitle("Списки доменов", "Один домен на строку, +.example.com включает поддомены. «Напрямую» главнее правил обхода.")
    WarpCard {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            listOf("Через WARP", "Напрямую").forEachIndexed { index, title ->
                FilterChip(
                    selected = tab == index,
                    onClick = { tab = index },
                    label = { Text(title) },
                    colors = FilterChipDefaults.filterChipColors(selectedContainerColor = p.surfaceHover, selectedLabelColor = p.text),
                    border = FilterChipDefaults.filterChipBorder(
                        enabled = true, selected = tab == index,
                        borderColor = p.border, selectedBorderColor = p.accent,
                    ),
                )
            }
        }
        if (locked) Text("Для сохранения отключите соединение.", fontSize = 13.sp, color = p.warn)
        if (tab == 0) {
            OutlinedTextField(proxy, { proxy = it }, minLines = 10, maxLines = 18, enabled = !locked, modifier = Modifier.fillMaxWidth())
        } else {
            OutlinedTextField(direct, { direct = it }, minLines = 10, maxLines = 18, enabled = !locked, modifier = Modifier.fillMaxWidth())
        }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Button(onClick = {
                message = runCatching { DomainLists.save(context, proxy, direct); "Списки сохранены. Применятся при подключении." }
                    .getOrElse { it.message ?: "Не удалось сохранить" }
            }, enabled = !locked) { Text("Сохранить списки") }
            Text("Строк: ${(if (tab == 0) proxy else direct).lines().count { it.isNotBlank() }}", fontSize = 13.sp, color = p.muted)
        }
        if (message.isNotEmpty()) Text(message, fontSize = 13.sp, color = p.muted)
    }
}

@Composable
internal fun SettingsPage(locked: Boolean) {
    val context = LocalContext.current
    val prefs = remember { Preferences(context) }
    var boot by remember { mutableStateOf(prefs.boot) }
    var auto by remember { mutableStateOf(prefs.autoConnect) }
    var mode by remember { mutableStateOf(prefs.appMode) }
    var selected by remember { mutableStateOf(prefs.apps) }
    var message by remember { mutableStateOf("") }
    var checkUpdates by remember { mutableStateOf(prefs.checkUpdates) }
    var autoSwitch by remember { mutableStateOf(prefs.autoSwitch) }
    val checking by UpdateState.checking.collectAsState()
    val available by UpdateState.available.collectAsState()
    val updateMessage by UpdateState.message.collectAsState()
    var showApps by remember { mutableStateOf(false) }
    var search by remember { mutableStateOf("") }
    val apps by produceState<List<Pair<String, String>>>(emptyList()) {
        value = withContext(Dispatchers.IO) {
            val intent = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
            @Suppress("DEPRECATION")
            context.packageManager.queryIntentActivities(intent, 0).map {
                it.activityInfo.packageName to it.loadLabel(context.packageManager).toString()
            }.filter { it.first != context.packageName }.distinctBy { it.first }.sortedBy { it.second.lowercase() }
        }
    }
    SectionTitle("Настройки", "Подключение, приложения в VPN и обновления.")
    WarpCard {
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Подключение", fontSize = 16.sp, fontWeight = FontWeight.Bold)
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Text("При открытии приложения", Modifier.weight(1f))
                Switch(auto, { auto = it; prefs.autoConnect = it }, enabled = !locked)
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Text("После загрузки телефона", Modifier.weight(1f))
                Switch(boot, { boot = it; prefs.boot = it }, enabled = !locked)
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Text("Сам менять профиль, если WARP перестал отвечать", Modifier.weight(1f))
                Switch(autoSwitch, { autoSwitch = it; prefs.autoSwitch = it })
            }
            Text("Сначала подключитесь вручную и разрешите VPN. Некоторые оболочки требуют разрешения автозапуска и отключения экономии батареи.")
            TextButton(onClick = { context.startActivity(Intent(Settings.ACTION_VPN_SETTINGS)) }) { Text("Системные настройки VPN") }
            Text("В настройках VPN выберите WARP Unblock → «Постоянная VPN» (Always-on). «Блокировать соединения без VPN» — системный kill switch. При раздельном туннелировании исключённые приложения могут потерять сеть.")
        }
    }
    // Статус перечитывается при возврате из системных настроек.
    var unrestricted by remember { mutableStateOf(Background.unrestricted(context)) }
    val lifecycle = androidx.lifecycle.compose.LocalLifecycleOwner.current
    DisposableEffect(lifecycle) {
        val observer = androidx.lifecycle.LifecycleEventObserver { _, event ->
            if (event == androidx.lifecycle.Lifecycle.Event.ON_RESUME) unrestricted = Background.unrestricted(context)
        }
        lifecycle.lifecycle.addObserver(observer)
        onDispose { lifecycle.lifecycle.removeObserver(observer) }
    }
    WarpCard {
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Работа в фоне", fontSize = 16.sp, fontWeight = FontWeight.Bold)
            Text(
                if (unrestricted) "Экономия батареи VPN не ограничивает."
                else "Экономия батареи может закрыть VPN, когда экран погаснет.",
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (!unrestricted) {
                    FilledTonalButton(onClick = { Background.requestUnrestricted(context) }) { Text("Снять ограничение") }
                }
                if (Background.xiaomi) {
                    FilledTonalButton(onClick = { Background.openAutostart(context) }) { Text("Автозапуск") }
                }
            }
            if (Background.xiaomi) {
                Text("На Xiaomi включите автозапуск, а в настройках приложения → «Контроль активности» выберите «Нет ограничений». В недавних приложениях карточку можно закрепить замком.", style = MaterialTheme.typography.bodySmall)
            }
        }
    }
    WarpCard {
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Приложения в VPN", fontSize = 16.sp, fontWeight = FontWeight.Bold)
            listOf("all" to "Все приложения", "allow" to "Только выбранные", "exclude" to "Кроме выбранных").forEach { (key, label) ->
                TextButton(onClick = { mode = key; prefs.appMode = key }, enabled = !locked) { Text((if (mode == key) "● " else "○ ") + label) }
            }
            Text("Доменные правила действуют внутри VPN. Сам WARP Unblock всегда исключён для защиты от петли. Изменения применятся после подключения.")
            TextButton(onClick = { showApps = !showApps }, enabled = !locked && mode != "all") { Text("Выбрано ${selected.size} · изменить") }
            if (showApps && mode != "all") {
                OutlinedTextField(search, { search = it }, label = { Text("Поиск приложения") }, modifier = Modifier.fillMaxWidth())
                LazyColumn(Modifier.fillMaxWidth().heightIn(min = 80.dp, max = 320.dp)) {
                items(apps.filter { search.isBlank() || it.first.contains(search, true) || it.second.contains(search, true) }, key = { it.first }) { (pkg, label) ->
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                        Column(Modifier.weight(1f)) { Text(label); Text(pkg, style = MaterialTheme.typography.bodySmall) }
                        Checkbox(pkg in selected, { checked -> selected = if (checked) selected + pkg else selected - pkg; prefs.apps = selected }, enabled = !locked)
                    }
                }
                }
            }
        }
    }
    WarpCard {
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Оформление", fontSize = 16.sp, fontWeight = FontWeight.Bold)
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
                Text("Ренат мод", Modifier.weight(1f))
                Switch(Mog.on, { Mog.on = it; prefs.renat = it })
            }
            Text("Радужная кнопка, фото на фоне и особые надписи.", style = MaterialTheme.typography.bodySmall)
        }
    }
    WarpCard {
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Версия ${BuildConfig.VERSION_NAME}", fontSize = 16.sp, fontWeight = FontWeight.Bold)
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Text("Проверять обновления при запуске", Modifier.weight(1f))
                Switch(checkUpdates, { checkUpdates = it; prefs.checkUpdates = it })
            }
            Text("Если вышла новая версия, появится окно: скачать APK, проверить SHA-256 и открыть установщик.")
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                FilledTonalButton(onClick = { UpdateState.check(context, manual = true) }, enabled = !checking) {
                    Text(if (checking) "Проверяю…" else "Проверить обновления")
                }
                available?.let { release ->
                    Button(onClick = { UpdateState.dialog.value = UpdateState.Dialog.Available(release) }) { Text("Установить ${release.version}") }
                }
            }
            if (updateMessage.isNotEmpty()) Text(updateMessage, style = MaterialTheme.typography.bodySmall)
            TextButton(onClick = { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://github.com/kwlnd/warp-unblock/releases"))) }) { Text("Открыть релизы") }
        }
    }
    if (message.isNotEmpty()) Text(message, Modifier.padding(8.dp))
}
