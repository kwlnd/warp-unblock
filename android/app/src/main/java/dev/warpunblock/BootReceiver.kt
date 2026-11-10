package dev.warpunblock

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.net.VpnService
import androidx.core.content.ContextCompat

class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED && intent.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
        if (!Preferences(context).boot || VpnService.prepare(context) != null) return
        runCatching {
            ContextCompat.startForegroundService(context,
                Intent(context, WarpVpnService::class.java).setAction(WarpVpnService.ACTION_START))
        }.onFailure { AppState.log("Автозапуск не удался: ${it.message}. Откройте приложение или используйте Always-on VPN.") }
    }
}
