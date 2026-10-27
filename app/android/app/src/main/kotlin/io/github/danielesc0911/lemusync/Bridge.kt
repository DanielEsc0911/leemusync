package io.github.danielesc0911.lemusync

import android.content.Context

/**
 * Loads `liblemusync_bridge.so`, the same library Dart loads through
 * flutter_rust_bridge, so Kotlin and Dart share one engine
 * (docs/specs/architecture.md, Bridge). The UniFFI calls live in
 * `uniffi.lemusync`.
 */
object Bridge {
    init {
        System.loadLibrary("lemusync_bridge")
    }

    /** Gives rustls-platform-verifier the app context. Call once, before any TLS. */
    @JvmStatic external fun initTls(context: Context)
}
