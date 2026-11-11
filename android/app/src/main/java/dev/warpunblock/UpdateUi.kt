package dev.warpunblock

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

/** Состояние проверки и установки обновлений; переживает пересоздание Activity. */
object UpdateState {
    sealed interface Dialog {
        data class Available(val release: AndroidRelease) : Dialog
        data class Downloading(val release: AndroidRelease, val fraction: Float, val label: String) : Dialog
        data class NeedPermission(val release: AndroidRelease, val apk: File) : Dialog
        data class Failed(val message: String) : Dialog
    }

    val dialog = MutableStateFlow<Dialog?>(null)
    val message = MutableStateFlow("")
    val available = MutableStateFlow<AndroidRelease?>(null)
    val checking = MutableStateFlow(false)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private var checkedOnStart = false

    /** Проверка при запуске: один раз за процесс, с учётом «пропустить версию». */
    fun checkOnStart(context: Context) {
        if (checkedOnStart || !Preferences(context).checkUpdates) return
        checkedOnStart = true
        check(context, manual = false)
    }

    fun check(context: Context, manual: Boolean) {
        if (checking.value) return
        checking.value = true
        if (manual) message.value = "Проверяю обновления…"
        val prefs = Preferences(context.applicationContext)
        scope.launch {
            val result = withContext(Dispatchers.IO) { runCatching { Updater.check() } }
            checking.value = false
            result.onSuccess { release ->
                available.value = release
                if (release == null) {
                    message.value = "Версия ${BuildConfig.VERSION_NAME} актуальна"
                } else {
                    message.value = "Доступна Android v${release.version}"
                    if ((manual || prefs.skipVersion != release.tag) && dialog.value == null) {
                        dialog.value = Dialog.Available(release)
                    }
                }
            }.onFailure { if (manual) message.value = "Не удалось проверить: ${it.message}" }
        }
    }

    fun skip(context: Context, release: AndroidRelease) {
        Preferences(context).skipVersion = release.tag
        dialog.value = null
    }

    fun install(context: Context, release: AndroidRelease) {
        val app = context.applicationContext
        dialog.value = Dialog.Downloading(release, 0f, "Скачиваю…")
        scope.launch {
            val result = withContext(Dispatchers.IO) {
                runCatching {
                    Updater.download(app, release) { done, total ->
                        val fraction = if (total > 0) done.toFloat() / total else 0f
                        dialog.value = Dialog.Downloading(
                            release, fraction,
                            "Скачиваю %.1f из %.1f МБ".format(done / 1048576.0, total / 1048576.0),
                        )
                    }
                }
            }
            result.onSuccess { apk -> launchInstaller(app, release, apk) }
                .onFailure { dialog.value = Dialog.Failed(it.message ?: "Не удалось скачать обновление") }
        }
    }

    fun launchInstaller(context: Context, release: AndroidRelease, apk: File) {
        if (Updater.needsInstallPermission(context)) {
            dialog.value = Dialog.NeedPermission(release, apk)
            return
        }
        dialog.value = null
        runCatching { Updater.install(context, apk) }
            .onFailure { dialog.value = Dialog.Failed(it.message ?: "Не удалось открыть установщик") }
    }
}

@Composable
fun UpdateDialog() {
    val context = LocalContext.current
    val state by UpdateState.dialog.collectAsState()
    val dismiss = { UpdateState.dialog.value = null }
    when (val dialog = state) {
        null -> Unit
        is UpdateState.Dialog.Available -> {
            val release = dialog.release
            AlertDialog(
                onDismissRequest = dismiss,
                title = { Text("Доступна версия ${release.version}") },
                text = {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text(
                            "У вас ${BuildConfig.VERSION_NAME}. APK %.1f МБ проверяется по SHA-256. Настройки и аккаунт сохранятся; VPN переподключится после обновления, если включено автоподключение."
                                .format(release.size / 1048576.0),
                            style = MaterialTheme.typography.bodySmall,
                        )
                        Text(
                            release.notes.ifBlank { "Описание изменений не указано." },
                            Modifier.fillMaxWidth().heightIn(max = 280.dp).verticalScroll(rememberScrollState()),
                            style = MaterialTheme.typography.bodyMedium,
                        )
                        ReleaseLinks(
                            onSkip = { UpdateState.skip(context, release) },
                            onPage = { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(release.page))) },
                        )
                    }
                },
                confirmButton = { Button(onClick = { UpdateState.install(context, release) }) { Text("Обновить") } },
                dismissButton = { TextButton(onClick = dismiss) { Text("Позже") } },
            )
        }
        is UpdateState.Dialog.Downloading -> AlertDialog(
            onDismissRequest = {},
            title = { Text("Обновление до ${dialog.release.version}") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(dialog.label)
                    LinearProgressIndicator(progress = { dialog.fraction }, modifier = Modifier.fillMaxWidth())
                }
            },
            confirmButton = {},
        )
        is UpdateState.Dialog.NeedPermission -> AlertDialog(
            onDismissRequest = dismiss,
            title = { Text("Разрешите установку") },
            text = { Text("Android просит разрешить WARP Unblock устанавливать приложения. Включите переключатель, вернитесь и нажмите «Продолжить».") },
            confirmButton = {
                Button(onClick = { UpdateState.launchInstaller(context, dialog.release, dialog.apk) }) { Text("Продолжить") }
            },
            dismissButton = { TextButton(onClick = { Updater.openInstallPermission(context) }) { Text("Открыть настройки") } },
        )
        is UpdateState.Dialog.Failed -> AlertDialog(
            onDismissRequest = dismiss,
            title = { Text("Обновление не установлено") },
            text = { Text(dialog.message) },
            confirmButton = { TextButton(onClick = dismiss) { Text("Закрыть") } },
        )
    }
}

@Composable
private fun ReleaseLinks(onSkip: () -> Unit, onPage: () -> Unit) {
    androidx.compose.foundation.layout.Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        TextButton(onClick = onSkip) { Text("Пропустить версию") }
        TextButton(onClick = onPage) { Text("Что нового") }
    }
}
