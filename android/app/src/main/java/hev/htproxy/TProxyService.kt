package hev.htproxy

/**
 * Обвязка над libhev-socks5-tunnel (tun -> SOCKS5). Имя пакета и класса зашиты в нативной
 * библиотеке (JNI RegisterNatives), менять их нельзя.
 */
class TProxyService {
    // Сигнатуры как в hev-socks5-tunnel 2.9.3 (void, void, long[]) — иначе RegisterNatives не найдёт методы.
    external fun TProxyStartService(configPath: String, fd: Int)
    external fun TProxyStopService()
    external fun TProxyGetStats(): LongArray

    companion object {
        init {
            System.loadLibrary("hev-socks5-tunnel")
        }
    }
}
