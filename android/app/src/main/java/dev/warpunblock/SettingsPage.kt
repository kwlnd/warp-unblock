package dev.warpunblock

import android.content.Intent
import android.net.Uri
import android.provider.Settings
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import java.net.HttpURLConnection
import java.net.URL

internal fun stableVersion(tag: String): List<Int>? {
    if (!Regex("[0-9]+\\.[0-9]+\\.[0-9]+").matches(tag)) return null
    return tag.split('.').map { it.toIntOrNull() ?: return null }
}
private fun compareVersion(a: List<Int>, b: List<Int>): Int {
    for (i in 0..2) if (a[i] != b[i]) return a[i].compareTo(b[i])
    return 0
}
private fun checkUpdates(): String {
    var newest: List<Int>? = null
    for (page in 1..10) {
        val connection = URL("https://api.github.com/repos/kwlnd/warp-unblock/releases?per_page=100&page=$page").openConnection() as HttpURLConnection
        connection.connectTimeout = 10_000
        connection.readTimeout = 15_000
        connection.setRequestProperty("User-Agent", "warp-unblock-android")
        val releases = try {
            check(connection.responseCode == 200) { "GitHub: HTTP ${connection.responseCode} (для приватного репозитория проверка недоступна)" }
            JSONArray(connection.inputStream.bufferedReader().use { it.readText() })
        } finally { connection.disconnect() }
        for (i in 0 until releases.length()) {
            val release = releases.getJSONObject(i)
            if (release.optBoolean("draft") || release.optBoolean("prerelease")) continue
            val tag = release.optString("tag_name")
            if (!tag.startsWith("android-v")) continue
            val version = stableVersion(tag.removePrefix("android-v")) ?: continue
            if (newest == null || compareVersion(version, newest!!) > 0) newest = version
        }
        if (releases.length() < 100) break
    }
    val latest = newest ?: error("Нет стабильных Android-релизов")
    val current = stableVersion(BuildConfig.VERSION_NAME) ?: error("Неизвестная версия приложения")
    return if (compareVersion(latest, current) > 0) "Доступна Android v${latest.joinToString(".")}. Откройте релизы для загрузки." else "Версия ${BuildConfig.VERSION_NAME} актуальна"
}

@Composable
internal fun SettingsPage(locked: Boolean) {
    val context = LocalContext.current
    val prefs = remember { Preferences(context) }
    val scope = rememberCoroutineScope()
    var boot by remember { mutableStateOf(prefs.boot) }
    var auto by remember { mutableStateOf(prefs.autoConnect) }
    var mode by remember { mutableStateOf(prefs.appMode) }
    var selected by remember { mutableStateOf(prefs.apps) }
    var proxy by remember { mutableStateOf(DomainLists.read(context, "proxy-domains.txt")) }
    var direct by remember { mutableStateOf(DomainLists.read(context, "direct-domains.txt")) }
    var message by remember { mutableStateOf("") }
    var checking by remember { mutableStateOf(false) }
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
    Card {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Подключение", style = MaterialTheme.typography.titleLarge)
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Text("При открытии приложения", Modifier.weight(1f))
                Switch(auto, { auto = it; prefs.autoConnect = it }, enabled = !locked)
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Text("После загрузки телефона", Modifier.weight(1f))
                Switch(boot, { boot = it; prefs.boot = it }, enabled = !locked)
            }
            Text("Сначала подключитесь вручную и разрешите VPN. Некоторые оболочки требуют разрешения автозапуска и отключения экономии батареи.")
            TextButton(onClick = { context.startActivity(Intent(Settings.ACTION_VPN_SETTINGS)) }) { Text("Системные настройки VPN") }
            Text("В настройках VPN выберите WARP Unblock → «Постоянная VPN» (Always-on). «Блокировать соединения без VPN» — системный kill switch. При раздельном туннелировании исключённые приложения могут потерять сеть.")
        }
    }
    Card {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Приложения в VPN", style = MaterialTheme.typography.titleLarge)
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
    Card {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Домены", style = MaterialTheme.typography.titleLarge)
            Text("Один домен на строку. +.example.com включает поддомены, # — комментарий. Кириллица — в punycode. «Напрямую» имеет приоритет.")
            OutlinedTextField(proxy, { proxy = it }, label = { Text("В обход через WARP") }, minLines = 3, maxLines = 8, enabled = !locked, modifier = Modifier.fillMaxWidth())
            OutlinedTextField(direct, { direct = it }, label = { Text("Напрямую") }, minLines = 3, maxLines = 8, enabled = !locked, modifier = Modifier.fillMaxWidth())
            FilledTonalButton(onClick = {
                message = runCatching { DomainLists.save(context, proxy, direct); "Списки сохранены. Применятся при подключении." }.getOrElse { it.message ?: "Не удалось сохранить" }
            }, enabled = !locked) { Text("Сохранить домены") }
        }
    }
    Card {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Версия ${BuildConfig.VERSION_NAME}", style = MaterialTheme.typography.titleLarge)
            FilledTonalButton(onClick = {
                checking = true; message = "Проверяю обновления…"
                scope.launch {
                    message = withContext(Dispatchers.IO) { runCatching { checkUpdates() }.getOrElse { "Ошибка: ${it.message}" } }
                    checking = false
                }
            }, enabled = !checking) { Text(if (checking) "Проверяю…" else "Проверить обновления") }
            TextButton(onClick = { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://github.com/kwlnd/warp-unblock/releases"))) }) { Text("Открыть релизы") }
        }
    }
    if (message.isNotEmpty()) Text(message, Modifier.padding(8.dp))
}
