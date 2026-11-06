package hev.htproxy

/**
 * Обвязка над libhev-socks5-tunnel (tun -> SOCKS5). Имя пакета и класса зашиты в нативной
 * библиотеке (JNI RegisterNatives), менять их нельзя.
 */
class TProxyService {
    external fun TProxyStartService(configPath: String, fd: Int): Boolean
    external fun TProxyStopService(): Boolean
    external fun TProxyIsRunning(): Boolean
    external fun TProxyGetStats(): LongArray

    companion object {
        init {
            System.loadLibrary("hev-socks5-tunnel")
        }
    }
}
