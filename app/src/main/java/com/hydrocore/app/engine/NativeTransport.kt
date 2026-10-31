package com.hydrocore.app.engine

/** One JSON request in, one JSON response out. Swappable so tests need no native library. */
fun interface Transport {
    fun dispatch(request: String): String?
}

/**
 * The JNI link to `libhydrocore.so`. The library is loaded lazily the first time this
 * object is touched, so JVM unit tests that use a fake [Transport] never load it.
 */
object NativeTransport : Transport {
    init {
        System.loadLibrary("hydrocore")
    }

    /** Implemented in Rust: `Java_com_hydrocore_app_engine_NativeTransport_nativeDispatch`. */
    external fun nativeDispatch(request: String): String?

    override fun dispatch(request: String): String? = nativeDispatch(request)
}
