package dev.warpunblock

import android.util.Base64
import org.bouncycastle.crypto.params.X25519PrivateKeyParameters
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.SecureRandom
import java.time.Instant

/** Аккаунт Cloudflare WARP (то же, что делает wgcf register). Хранится в filesDir/warp.json. */
data class WarpAccount(
    val privateKey: String,
    val publicKey: String,
    val ipv4: String,
    val ipv6: String,
    val reserved: List<Int>,
) {
    fun save(dir: File) {
        dir.mkdirs()
        File(dir, "warp.json").writeText(
            JSONObject()
                .put("private_key", privateKey)
                .put("public_key", publicKey)
                .put("ipv4", ipv4)
                .put("ipv6", ipv6)
                .put("reserved", org.json.JSONArray(reserved))
                .toString(),
        )
    }

    companion object {
        fun delete(dir: File) {
            File(dir, "warp.json").delete()
        }

        /** Разбирает wgcf-profile.conf (с ПК). Необязательная строка `Reserved = a, b, c` поддерживается. */
        fun parseWgcf(text: String): WarpAccount {
            fun field(name: String): String =
                Regex("(?im)^\\s*$name\\s*=\\s*(.+?)\\s*$").find(text)?.groupValues?.get(1)
                    ?: error("В профиле нет поля $name")
            val addresses = field("Address").split(",").map { it.trim().substringBefore("/") }
            val reserved = Regex("(?i)reserved\\s*[=:]\\s*\\[?\\s*(\\d+)\\s*,\\s*(\\d+)\\s*,\\s*(\\d+)")
                .find(text)?.groupValues?.drop(1)?.map { it.toInt() } ?: emptyList()
            return WarpAccount(
                privateKey = field("PrivateKey"),
                publicKey = field("PublicKey"),
                ipv4 = addresses.firstOrNull { !it.contains(":") } ?: error("В профиле нет IPv4-адреса"),
                ipv6 = addresses.firstOrNull { it.contains(":") } ?: error("В профиле нет IPv6-адреса"),
                reserved = reserved,
            )
        }

        fun load(dir: File): WarpAccount? {
            val file = File(dir, "warp.json")
            if (!file.isFile) return null
            return runCatching {
                val json = JSONObject(file.readText())
                val reserved = json.getJSONArray("reserved")
                WarpAccount(
                    json.getString("private_key"),
                    json.getString("public_key"),
                    json.getString("ipv4"),
                    json.getString("ipv6"),
                    (0 until reserved.length()).map { reserved.getInt(it) },
                )
            }.getOrNull()
        }

        /** Регистрирует новый аккаунт. Нужен доступ к api.cloudflareclient.com (VPN при этом выключен). */
        fun register(dir: File): WarpAccount {
            val private = X25519PrivateKeyParameters(SecureRandom())
            val privateB64 = Base64.encodeToString(private.encoded, Base64.NO_WRAP)
            val publicB64 = Base64.encodeToString(private.generatePublicKey().encoded, Base64.NO_WRAP)
            val body = JSONObject()
                .put("install_id", "")
                .put("fcm_token", "")
                .put("tos", Instant.now().toString())
                .put("key", publicB64)
                .put("type", "Android")
                .put("model", "PC")
                .put("locale", "en_US")
            val connection = URL("https://api.cloudflareclient.com/v0a1922/reg")
                .openConnection() as HttpURLConnection
            try {
                connection.requestMethod = "POST"
                connection.connectTimeout = 15_000
                connection.readTimeout = 20_000
                connection.doOutput = true
                connection.setRequestProperty("Content-Type", "application/json")
                connection.setRequestProperty("User-Agent", "okhttp/3.12.1")
                connection.setRequestProperty("CF-Client-Version", "a-6.3-1922")
                connection.outputStream.use { it.write(body.toString().toByteArray()) }
                if (connection.responseCode !in 200..299) {
                    error("Cloudflare ответил ${connection.responseCode}")
                }
                val response = JSONObject(connection.inputStream.bufferedReader().use { it.readText() })
                val config = response.getJSONObject("config")
                val peer = config.getJSONArray("peers").getJSONObject(0)
                val addresses = config.getJSONObject("interface").getJSONObject("addresses")
                val clientId = Base64.decode(config.getString("client_id"), Base64.DEFAULT)
                val account = WarpAccount(
                    privateKey = privateB64,
                    publicKey = peer.getString("public_key"),
                    ipv4 = addresses.getString("v4"),
                    ipv6 = addresses.getString("v6"),
                    reserved = clientId.take(3).map { it.toInt() and 0xFF },
                )
                account.save(dir)
                return account
            } catch (error: java.io.IOException) {
                throw IllegalStateException(
                    "Cloudflare не отвечает (${error.message}). Скорее всего оператор блокирует регистрацию WARP — " +
                        "импортируйте wgcf-профиль с компьютера на вкладке «Профили».",
                    error,
                )
            } finally {
                connection.disconnect()
            }
        }
    }
}
