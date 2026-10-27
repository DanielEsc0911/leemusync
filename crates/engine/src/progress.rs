//! Live transfer progress (architecture §Engine API, `TransferProgress`).

use std::sync::{Arc, Mutex, PoisonError};

use lemusync_api::{ConnectionEvent, TransferPhase, TransferProgress};
use lemusync_store::{RetryNotice, RetryObserver};

/// Receives every progress update. Called from the engine's task; keep it cheap.
pub type ProgressSink = Arc<dyn Fn(&TransferProgress) + Send + Sync>;

/// Receives every `ConnectionEvent`. Called from the engine's task; keep it cheap.
pub type ConnectionSink = Arc<dyn Fn(&ConnectionEvent) + Send + Sync>;

/// A store observer that forwards each `RetryNotice` to `sink` as a `ConnectionEvent`.
pub(crate) fn observer(sink: ConnectionSink) -> RetryObserver {
    Arc::new(move |n| sink(&event(&n)))
}

fn event(n: &RetryNotice) -> ConnectionEvent {
    match *n {
        RetryNotice::Retrying {
            operation,
            attempt,
            max_attempts,
            delay,
        } => ConnectionEvent::Retrying {
            operation: operation.to_owned(),
            attempt,
            max_attempts,
            delay_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
        },
        RetryNotice::Recovered => ConnectionEvent::Recovered,
    }
}

/// One stream's progress: counts blobs as they finish and tells the sink.
pub(crate) struct Meter {
    sink: Option<ProgressSink>,
    now: Mutex<Counts>,
}

struct Counts {
    progress: TransferProgress,
    /// Files and bytes moved in every phase so far.
    files: u32,
    bytes: u64,
}

impl Meter {
    /// A meter for one stream, starting in `Checking`.
    pub fn new(sink: Option<ProgressSink>, player: &str, emulator: &str, game: &str) -> Self {
        let m = Self {
            sink,
            now: Mutex::new(Counts {
                progress: TransferProgress {
                    player: player.to_owned(),
                    emulator: emulator.to_owned(),
                    game: game.to_owned(),
                    phase: TransferPhase::Checking,
                    files_done: 0,
                    files_total: 0,
                    bytes_done: 0,
                    bytes_total: 0,
                },
                files: 0,
                bytes: 0,
            }),
        };
        m.emit();
        m
    }

    /// A meter that reports nowhere.
    #[cfg(test)]
    pub fn none() -> Self {
        Self::new(None, "", "", "")
    }

    pub fn begin(&self, phase: TransferPhase, files_total: u32, bytes_total: u64) {
        {
            let mut c = self.now.lock().unwrap_or_else(PoisonError::into_inner);
            c.progress.phase = phase;
            c.progress.files_done = 0;
            c.progress.files_total = files_total;
            c.progress.bytes_done = 0;
            c.progress.bytes_total = bytes_total;
        }
        self.emit();
    }

    /// One file of `bytes` finished.
    pub fn advance(&self, bytes: u64) {
        {
            let mut c = self.now.lock().unwrap_or_else(PoisonError::into_inner);
            c.progress.files_done = c.progress.files_done.saturating_add(1);
            c.progress.bytes_done = c.progress.bytes_done.saturating_add(bytes);
            c.files = c.files.saturating_add(1);
            c.bytes = c.bytes.saturating_add(bytes);
        }
        self.emit();
    }

    /// Files and bytes moved so far, across phases.
    pub fn transferred(&self) -> (u32, u64) {
        let c = self.now.lock().unwrap_or_else(PoisonError::into_inner);
        (c.files, c.bytes)
    }

    fn emit(&self) {
        if let Some(sink) = &self.sink {
            let p = self
                .now
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .progress
                .clone();
            sink(&p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_notices_become_connection_events() {
        let n = RetryNotice::Retrying {
            operation: "put",
            attempt: 2,
            max_attempts: 5,
            delay: std::time::Duration::from_millis(1500),
        };
        assert_eq!(
            event(&n),
            ConnectionEvent::Retrying {
                operation: "put".into(),
                attempt: 2,
                max_attempts: 5,
                delay_ms: 1500
            }
        );
        assert_eq!(event(&RetryNotice::Recovered), ConnectionEvent::Recovered);
    }

    #[test]
    fn counts_each_phase_and_the_total() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let sink: ProgressSink = Arc::new(move |p| s.lock().unwrap().push(p.clone()));
        let m = Meter::new(Some(sink), "Ana", "cemu", "101c9400");
        m.begin(TransferPhase::Uploading, 2, 30);
        m.advance(10);
        m.advance(20);
        m.begin(TransferPhase::Downloading, 1, 5);
        m.advance(5);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 6);
        assert_eq!(seen[0].phase, TransferPhase::Checking);
        assert_eq!(seen[0].player, "Ana");
        assert_eq!((seen[3].files_done, seen[3].bytes_done), (2, 30));
        assert_eq!(seen[5].phase, TransferPhase::Downloading);
        assert_eq!(m.transferred(), (3, 35));
    }
}
