# Keep rules for release builds (R8). Without them the app crashes at the
# first UniFFI call (docs/specs/architecture.md, Bridge).

# JNA finds these by reflection from native code; UniFFI's Kotlin bindings
# use JNA structures and callbacks.
-keep class com.sun.jna.** { *; }
-keep class * extends com.sun.jna.** { *; }
-keep class uniffi.** { *; }
-dontwarn java.awt.**

# rustls-platform-verifier calls its Kotlin half from Rust through JNI.
-keep, includedescriptorclasses class org.rustls.platformverifier.** { *; }
