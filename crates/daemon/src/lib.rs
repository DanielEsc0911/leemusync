//! `lemusyncd` internals: the run loop around `engine::Service`, logging, desktop presence
//! (tray, notifications, power) and the tray view.
//! Spec: docs/specs/architecture.md §Process model.

pub mod i18n;
pub mod logging;
pub mod notices;
pub mod platform;
pub mod run;
pub mod view;
