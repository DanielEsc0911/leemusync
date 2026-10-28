//! Linux: `ksni` tray, `notify-rust` notifications, logind and XFCE power hooks over `zbus`.
//! Evidence: docs/specs/platforms/desktop.md §Linux results. Each `start_*` must be called
//! inside a tokio runtime; D-Bus work runs on spawned tasks so the trait calls never block.

mod notify;
mod power;
mod tray;

pub use notify::start_notifier;
pub use power::start_power;
pub use tray::start_tray;
