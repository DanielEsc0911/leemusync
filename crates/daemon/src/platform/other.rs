//! No-op tray, notifications and power for targets without an implementation yet
//! (Windows and macOS: Phase 2 Task 11).

use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::mpsc;

use super::{IdleGuard, Notice, NoticeId, Notifier, Power, SleepEdge, Tray, TrayAction, TrayView};

struct NoTray;

impl Tray for NoTray {
    fn show(&self, _view: &TrayView) {}
}

#[must_use]
pub fn start_tray(_tx: mpsc::Sender<TrayAction>) -> Box<dyn Tray> {
    Box::new(NoTray)
}

struct NoNotifier(AtomicU64);

impl Notifier for NoNotifier {
    fn notify(&self, _n: Notice) -> NoticeId {
        NoticeId(self.0.fetch_add(1, Ordering::Relaxed) + 1)
    }
}

#[must_use]
pub fn start_notifier(_tx: mpsc::Sender<(NoticeId, String)>) -> Box<dyn Notifier> {
    Box::new(NoNotifier(AtomicU64::new(0)))
}

struct NoPower;

impl Power for NoPower {
    fn block_idle(&self, _reason: &str) -> IdleGuard {
        IdleGuard::noop()
    }

    fn sleep_events(&self) -> mpsc::Receiver<SleepEdge> {
        mpsc::channel(1).1
    }

    fn ack_suspend(&self) {}
}

#[must_use]
pub fn start_power() -> Box<dyn Power> {
    Box::new(NoPower)
}
