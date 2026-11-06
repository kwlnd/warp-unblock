package dev.warpunblock

import java.io.File

/** Process helpers that work on Android API 24 and later. */
internal fun Process.runningCompat(): Boolean =
    try {
        exitValue()
        false
    } catch (_: IllegalThreadStateException) {
        true
    }

internal fun Process.stopCompat() {
    destroy()
}

internal fun ProcessBuilder.logged(file: File): Process {
    val process = redirectErrorStream(true).start()
    Thread({
        runCatching {
            process.inputStream.use { input -> file.outputStream().use { output -> input.copyTo(output) } }
        }
    }, "${file.nameWithoutExtension}-log").apply {
        isDaemon = true
        start()
    }
    return process
}
