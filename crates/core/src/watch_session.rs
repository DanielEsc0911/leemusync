//! Watch-mode sessions: what to do when an emulator the user opened starts, changes saves and
//! exits. Pure: the daemon feeds process and file events and carries out the actions.
//! Spec: docs/specs/sync-model.md (Play sessions: Watch mode). I3 holds: uploads wait for the
//! emulator to exit (or for no emulator to be running) and for the quiet period.

use std::collections::{BTreeMap, BTreeSet};

use crate::StreamId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionAction {
    /// The emulator started: fetch heads only and warn if another device has a newer save.
    /// Never a restore while it runs (I3).
    CheckRemote(String),
    /// First change of a stream while its emulator runs.
    TakeLease(StreamId),
    /// Snapshot and upload once the stream's files are quiet.
    UploadAfterQuiet(StreamId),
    /// After that upload.
    ReleaseLease(StreamId),
}

#[derive(Clone, Debug, Default)]
struct Emulator {
    running: bool,
    /// Streams changed during the current run. They stay tracked even when the lease take
    /// failed: the upload still happens (leases never weaken safety, D6).
    changed: BTreeSet<StreamId>,
}

/// Sessions keyed by emulator id.
#[derive(Clone, Debug, Default)]
pub struct WatchSessions {
    emulators: BTreeMap<String, Emulator>,
}

impl WatchSessions {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The process poller saw `emulator` start or stop.
    pub fn on_running(
        &mut self,
        emulator: &str,
        running: bool,
        _now_ms: u64,
    ) -> Vec<SessionAction> {
        let e = self.emulators.entry(emulator.to_owned()).or_default();
        if e.running == running {
            return Vec::new();
        }
        e.running = running;
        if running {
            return vec![SessionAction::CheckRemote(emulator.to_owned())];
        }
        std::mem::take(&mut e.changed)
            .into_iter()
            .flat_map(|s| {
                [
                    SessionAction::UploadAfterQuiet(s),
                    SessionAction::ReleaseLease(s),
                ]
            })
            .collect()
    }

    /// A file of `stream` (a save of `emulator`) changed.
    pub fn on_change(
        &mut self,
        stream: StreamId,
        emulator: &str,
        _now_ms: u64,
    ) -> Vec<SessionAction> {
        let e = self.emulators.entry(emulator.to_owned()).or_default();
        if !e.running {
            return vec![SessionAction::UploadAfterQuiet(stream)];
        }
        if e.changed.insert(stream) {
            vec![SessionAction::TakeLease(stream)]
        } else {
            Vec::new()
        }
    }

    #[must_use]
    pub fn is_running(&self, emulator: &str) -> bool {
        self.emulators.get(emulator).is_some_and(|e| e.running)
    }
}

#[cfg(test)]
mod tests {
    use super::SessionAction::{CheckRemote, ReleaseLease, TakeLease, UploadAfterQuiet};
    use super::*;
    use crate::id::Hash32;

    fn s(n: u8) -> StreamId {
        StreamId(Hash32([n; 32]))
    }

    #[test]
    fn start_checks_the_remote_once() {
        let mut w = WatchSessions::new();
        assert_eq!(
            w.on_running("cemu", true, 0),
            vec![CheckRemote("cemu".into())]
        );
        assert_eq!(w.on_running("cemu", true, 1), vec![]);
        assert!(w.is_running("cemu"));
    }

    #[test]
    fn changes_while_running_take_one_lease_per_stream() {
        let mut w = WatchSessions::new();
        w.on_running("cemu", true, 0);
        assert_eq!(w.on_change(s(1), "cemu", 1), vec![TakeLease(s(1))]);
        assert_eq!(w.on_change(s(1), "cemu", 2), vec![]);
        assert_eq!(w.on_change(s(2), "cemu", 3), vec![TakeLease(s(2))]);
    }

    #[test]
    fn exit_uploads_every_changed_stream_then_releases() {
        let mut w = WatchSessions::new();
        w.on_running("cemu", true, 0);
        w.on_change(s(1), "cemu", 1);
        w.on_change(s(2), "cemu", 2);
        assert_eq!(
            w.on_running("cemu", false, 3),
            vec![
                UploadAfterQuiet(s(1)),
                ReleaseLease(s(1)),
                UploadAfterQuiet(s(2)),
                ReleaseLease(s(2)),
            ]
        );
        assert_eq!(w.on_running("cemu", false, 4), vec![]);
        // A new run starts clean.
        w.on_running("cemu", true, 5);
        assert_eq!(w.on_running("cemu", false, 6), vec![]);
    }

    #[test]
    fn a_change_while_not_running_uploads_without_a_lease() {
        let mut w = WatchSessions::new();
        assert_eq!(w.on_change(s(1), "cemu", 0), vec![UploadAfterQuiet(s(1))]);
        assert_eq!(
            w.on_running("cemu", true, 1),
            vec![CheckRemote("cemu".into())]
        );
        assert_eq!(w.on_running("cemu", false, 2), vec![]);
    }

    #[test]
    fn emulators_are_tracked_independently() {
        let mut w = WatchSessions::new();
        w.on_running("cemu", true, 0);
        assert_eq!(w.on_change(s(1), "cemu", 1), vec![TakeLease(s(1))]);
        assert_eq!(
            w.on_change(s(2), "dolphin", 1),
            vec![UploadAfterQuiet(s(2))]
        );
        assert_eq!(
            w.on_running("dolphin", true, 2),
            vec![CheckRemote("dolphin".into())]
        );
        assert_eq!(w.on_running("dolphin", false, 3), vec![]);
        assert!(w.is_running("cemu"));
        assert_eq!(
            w.on_running("cemu", false, 4),
            vec![UploadAfterQuiet(s(1)), ReleaseLease(s(1))]
        );
    }
}
