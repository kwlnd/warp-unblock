package dev.warpunblock

import android.annotation.SuppressLint
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings

/**
 * Работа в фоне. Оболочки вроде MIUI/HyperOS убивают «лишние» процессы при блокировке экрана
 * (LockScreenClean), и вместе с ними пропадает VPN. Исключение из экономии батареи и автозапуск
 * это предотвращают; «Постоянная VPN» дополнительно поднимает сервис после убийства.
 */
object Background {
    /** Xiaomi, Redmi, POCO: у них свой экран автозапуска и агрессивная чистка. */
    val xiaomi: Boolean = Build.MANUFACTURER.lowercase() in setOf("xiaomi", "redmi", "poco")

    fun unrestricted(context: Context): Boolean =
        context.getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(context.packageName)

    /** Системный запрос «Разрешить работу в фоне?»; если оболочка его не знает — список исключений. */
    @SuppressLint("BatteryLife")
    fun requestUnrestricted(context: Context) {
        val direct = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:${context.packageName}"))
        open(context, direct, Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS))
    }

    /** Экран автозапуска MIUI; если его нет — настройки приложения. */
    fun openAutostart(context: Context) {
        val miui = Intent().setComponent(
            ComponentName("com.miui.securitycenter", "com.miui.permcenter.autostart.AutoStartManagementActivity"),
        )
        open(context, miui, details(context))
    }

    fun details(context: Context) =
        Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${context.packageName}"))

    private fun open(context: Context, vararg intents: Intent) {
        for (intent in intents) {
            if (context !is android.app.Activity) intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            if (runCatching { context.startActivity(intent) }.isSuccess) return
        }
    }
}
