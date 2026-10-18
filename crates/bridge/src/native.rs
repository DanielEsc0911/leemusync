//! `UniFFI` surface for Kotlin (Android) and Swift (iOS).

/// Returns `pong #<n>`, where `n` counts calls through both bindings.
#[uniffi::export]
#[must_use]
pub fn engine_ping() -> String {
    format!("pong #{}", crate::count_call())
}
