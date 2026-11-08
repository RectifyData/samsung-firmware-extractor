package com.rectify.fwextract

/**
 * JNI bridge to the Rust core (rust/src/jni.rs). The library is built by cargo-ndk as libfwx.so.
 *
 * File access goes through file descriptors, so the same code works with Storage Access Framework
 * URIs: Kotlin opens the URI, detaches the descriptor, and Rust takes ownership of it.
 */
object Native {
    init {
        System.loadLibrary("fwx")
    }

    /** Returns one line per image: "<size or -1>\t<path>". Throws RuntimeException on failure. */
    @JvmStatic
    external fun scan(fd: Int, name: String): String

    /** Decodes the newline-separated [selected] paths into [workDir], reporting through [callback]. */
    @JvmStatic
    external fun extract(fd: Int, name: String, selected: String, workDir: String, callback: Callback)

    /** Stops a running scan or extraction at its next read. */
    @JvmStatic
    external fun cancel()

    interface Callback {
        fun onLog(message: String)

        /** A raw image is ready at [path]. It is deleted after this returns, so copy it out. */
        fun onImage(name: String, path: String)
    }
}
