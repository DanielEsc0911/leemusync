//! Watch-mode process detection: polls which emulators run and reports each start and stop
//! (sync-model §Watch mode). The poll is the daemon's only periodic wake (reliability §Budgets).

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;

use crate::ProcessProbe;

/// Default poll interval (V-DAEMON-2 measures its idle cost).
pub const DEFAULT_POLL: Duration = Duration::from_secs(3);

/// Calls `ProcessProbe::running_set` every `poll` and sends `(emulator, running)` only when it
/// changes. Everything starts as "not running", so emulators already open are reported on the
/// first poll. Stops when dropped or when the receiver is gone.
pub struct ProcessPoller {
    task: JoinHandle<()>,
}

impl ProcessPoller {
    /// Must be called inside a tokio runtime.
    #[must_use]
    pub fn start(
        probe: Arc<dyn ProcessProbe>,
        names_by_emulator: Vec<(String, Vec<String>)>,
        poll: Duration,
        tx: Sender<(String, bool)>,
    ) -> Self {
        let names = Arc::new(names_by_emulator);
        let task = tokio::spawn(async move {
            let mut was: BTreeSet<String> = BTreeSet::new();
            loop {
                let (p, n) = (Arc::clone(&probe), Arc::clone(&names));
                // The process walk blocks (a `/proc` read on Linux).
                let Ok(now) = tokio::task::spawn_blocking(move || p.running_set(&n)).await else {
                    return;
                };
                for id in now.symmetric_difference(&was) {
                    if tx.send((id.clone(), now.contains(id))).await.is_err() {
                        return;
                    }
                }
                was = now;
                tokio::time::sleep(poll).await;
            }
        });
        Self { task }
    }
}

impl Drop for ProcessPoller {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FakeProbe;
    use tokio::time::timeout;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_poller_reports_start_and_stop_once_each() {
        let probe = Arc::new(FakeProbe::default());
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _p = ProcessPoller::start(
            probe.clone(),
            vec![("cemu".into(), vec!["Cemu".into()])],
            Duration::from_millis(20),
            tx,
        );
        let wait = Duration::from_secs(10);
        probe.set(true);
        assert_eq!(
            timeout(wait, rx.recv()).await.ok().flatten(),
            Some(("cemu".to_owned(), true))
        );
        probe.set(false);
        assert_eq!(
            timeout(wait, rx.recv()).await.ok().flatten(),
            Some(("cemu".to_owned(), false))
        );
        // Many more polls, no change: nothing more.
        assert!(
            timeout(Duration::from_millis(300), rx.recv())
                .await
                .is_err()
        );
    }
}
