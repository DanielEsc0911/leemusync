//! Fluent strings for the tray and notifications. The loader is the CLI's file, included as
//! a module so both binaries share one copy without a new crate (plan Task 6).

#[path = "../../cli/src/i18n.rs"]
#[allow(dead_code, clippy::must_use_candidate)]
mod shared;

pub use shared::{I18n, s};
