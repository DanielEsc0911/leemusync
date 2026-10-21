//! Live transfer progress (architecture §Engine API, `TransferProgress`).

use std::sync::{Arc, Mutex, PoisonError};

use leemusync_api::{TransferPhase, TransferProgress};

/// Receives every progress update. Called from the engine's task; keep it cheap.
pub type ProgressSink = Arc<dyn Fn(&TransferProgress) + Send + Sync>;

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
