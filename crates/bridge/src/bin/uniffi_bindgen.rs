//! Generates the Kotlin and Swift bindings: `cargo run -p lemusync-bridge
//! --features bindgen --bin uniffi-bindgen -- generate --library <lib> …`.

fn main() {
    uniffi::uniffi_bindgen_main();
}
