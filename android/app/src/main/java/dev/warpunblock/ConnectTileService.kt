package dev.warpunblock

import android.app.PendingIntent
import android.content.Intent
import android.graphics.drawable.Icon
import android.net.VpnService
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.launch

/** Плитка в шторке (в MIUI/HyperOS: пункт «Изменить» в центре управления): включает и выключает обход. */
class ConnectTileService : TileService() {
    private var scope: CoroutineScope? = null
    private var job: Job? = null

    override fun onStartListening() {
        AppState.init(this)
        val scope = CoroutineScope(Dispatchers.Main)
        this.scope = scope
        job = scope.launch { AppState.ui.collect { render(it) } }
    }

    override fun onStopListening() {
        job?.cancel()
        scope?.cancel()
        scope = null
    }

    override fun onClick() {
        val state = AppState.ui.value
        when {
            state.selecting -> return
            state.phase == Phase.CONNECTED || state.phase == Phase.BUSY -> {
                startService(Intent(this, WarpVpnService::class.java).setAction(WarpVpnService.ACTION_STOP))
            }
            VpnService.prepare(this) != null -> openApp()
            else -> try {
                ContextCompat.startForegroundService(
                    this,
                    Intent(this, WarpVpnService::class.java).setAction(WarpVpnService.ACTION_START),
                )
            } catch (error: Exception) {
                // Система не дала запустить сервис из фона — откроем приложение, оно подключится само.
                openApp()
            }
        }
    }

    private fun openApp() {
        val intent = Intent(this, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            .putExtra(MainActivity.EXTRA_CONNECT, true)
        if (Build.VERSION.SDK_INT >= 34) {
            startActivityAndCollapse(
                PendingIntent.getActivity(this, 0, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT),
            )
        } else {
            @Suppress("DEPRECATION")
            startActivityAndCollapse(intent)
        }
    }

    private fun render(ui: UiState) {
        val tile = qsTile ?: return
        tile.icon = Icon.createWithResource(this, R.drawable.ic_tile)
        tile.label = "WARP"
        when (ui.phase) {
            Phase.CONNECTED -> {
                tile.state = Tile.STATE_ACTIVE
                tile.subtitleCompat("Подключено")
            }
            Phase.BUSY -> {
                tile.state = Tile.STATE_ACTIVE
                tile.subtitleCompat(if (ui.selecting) "Подбор…" else "Подключаю…")
            }
            else -> {
                tile.state = Tile.STATE_INACTIVE
                tile.subtitleCompat("Выключено")
            }
        }
        tile.updateTile()
    }

    private fun Tile.subtitleCompat(text: String) {
        if (Build.VERSION.SDK_INT >= 29) subtitle = text
    }
}
