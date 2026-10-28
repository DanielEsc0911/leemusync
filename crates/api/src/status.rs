//! The status model every surface shows (status-and-notifications §Status model).

use serde::{Deserialize, Serialize};

/// The one state a surface shows, picked from [`StatusFacts`] by priority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusState {
    /// Nothing to do; every save is synced.
    Synced,
    /// Looking for local or remote changes.
    Scanning,
    /// Uploading or downloading.
    Syncing,
    /// The user paused sync.
    Paused,
    /// The storage can't be reached; changes wait in the queue.
    Offline,
    /// Conflicts or foreign leases need the user.
    Attention,
    /// Sync can't run until the user fixes something.
    Error,
}

/// Why sync is in the `error` state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Storage sign-in expired or was refused.
    Auth,
    /// The storage is full.
    Quota,
    /// A folder or object can't be read or written.
    Permission,
    /// The config is invalid.
    Config,
}

/// What every surface shows: the `StatusChanged` event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// The state picked by priority.
    pub state: StatusState,
    /// Last finished sync, Unix milliseconds.
    pub last_sync_ms: Option<u64>,
    /// Uploads in flight.
    pub uploads: u32,
    /// Downloads in flight.
    pub downloads: u32,
    /// Bytes the current transfers move.
    pub bytes: u64,
    /// When a pause ends, Unix milliseconds; `None` while paused means until resume.
    pub paused_until_ms: Option<u64>,
    /// Changes waiting to sync.
    pub queued: u32,
    /// Streams with more than one head.
    pub conflicts: u32,
    /// Live leases held by other devices.
    pub foreign_leases: u32,
    /// Set in the `error` state.
    pub error: Option<ErrorKind>,
}

/// The raw facts the engine knows; [`Status::from_facts`] turns them into a [`Status`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts, one flag each"
)]
pub struct StatusFacts {
    /// A blocking error, if any.
    pub error: Option<ErrorKind>,
    /// Streams with more than one head.
    pub conflicts: u32,
    /// Live leases held by other devices.
    pub foreign_leases: u32,
    /// A transfer is running.
    pub syncing: bool,
    /// Uploads in flight.
    pub uploads: u32,
    /// Downloads in flight.
    pub downloads: u32,
    /// Bytes the current transfers move.
    pub bytes: u64,
    /// The storage can't be reached.
    pub offline: bool,
    /// Changes waiting to sync.
    pub queued: u32,
    /// The user paused sync.
    pub paused: bool,
    /// When the pause ends; `None` means until resume.
    pub paused_until_ms: Option<u64>,
    /// A scan is running.
    pub scanning: bool,
    /// Last finished sync, Unix milliseconds.
    pub last_sync_ms: Option<u64>,
}

impl Status {
    /// Picks the state by the spec's priority:
    /// `error` > `attention` > `syncing` > `offline` > `paused` > `scanning` > `synced`.
    #[must_use]
    pub fn from_facts(f: &StatusFacts) -> Status {
        let state = if f.error.is_some() {
            StatusState::Error
        } else if f.conflicts > 0 || f.foreign_leases > 0 {
            StatusState::Attention
        } else if f.syncing {
            StatusState::Syncing
        } else if f.offline {
            StatusState::Offline
        } else if f.paused {
            StatusState::Paused
        } else if f.scanning {
            StatusState::Scanning
        } else {
            StatusState::Synced
        };
        Status {
            state,
            last_sync_ms: f.last_sync_ms,
            uploads: f.uploads,
            downloads: f.downloads,
            bytes: f.bytes,
            paused_until_ms: if f.paused { f.paused_until_ms } else { None },
            queued: f.queued,
            conflicts: f.conflicts,
            foreign_leases: f.foreign_leases,
            error: f.error,
        }
    }
}

/// A running play session: the overlay the `SessionChanged` event carries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// Game id.
    pub game: String,
    /// The game's name on this device, when known.
    pub name: Option<String>,
    /// Emulator profile id.
    pub emulator: String,
    /// Player name.
    pub player: String,
    /// Name of the device playing.
    pub device: String,
    /// Session start, Unix milliseconds.
    pub started_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Facts from space-separated flag names.
    fn facts(flags: &str) -> StatusFacts {
        let mut f = StatusFacts::default();
        for flag in flags.split_whitespace() {
            match flag {
                "error" => f.error = Some(ErrorKind::Auth),
                "conflicts" => f.conflicts = 1,
                "leases" => f.foreign_leases = 1,
                "syncing" => f.syncing = true,
                "offline" => f.offline = true,
                "paused" => f.paused = true,
                "scanning" => f.scanning = true,
                other => panic!("unknown flag {other}"),
            }
        }
        f
    }

    #[test]
    fn priority_table() {
        use StatusState as S;
        let table = [
            ("", S::Synced),
            ("scanning", S::Scanning),
            ("paused", S::Paused),
            ("offline", S::Offline),
            ("syncing", S::Syncing),
            ("conflicts", S::Attention),
            ("leases", S::Attention),
            ("error", S::Error),
            (
                "error conflicts leases syncing offline paused scanning",
                S::Error,
            ),
            ("error conflicts", S::Error),
            ("error syncing", S::Error),
            ("conflicts syncing", S::Attention),
            ("leases offline", S::Attention),
            ("conflicts scanning", S::Attention),
            ("syncing offline", S::Syncing),
            ("syncing paused", S::Syncing),
            ("syncing scanning", S::Syncing),
            ("paused offline", S::Offline),
            ("offline scanning", S::Offline),
            ("paused scanning", S::Paused),
        ];
        for (flags, want) in table {
            assert_eq!(Status::from_facts(&facts(flags)).state, want, "{flags}");
        }
    }

    #[test]
    fn from_facts_copies_data() {
        let s = Status::from_facts(&StatusFacts {
            uploads: 3,
            bytes: 9,
            syncing: true,
            paused_until_ms: Some(5),
            last_sync_ms: Some(7),
            ..Default::default()
        });
        assert_eq!((s.uploads, s.bytes, s.last_sync_ms), (3, 9, Some(7)));
        // A pause deadline only shows while paused.
        assert_eq!(s.paused_until_ms, None);
    }

    #[test]
    fn status_json_is_stable() {
        let s = Status::from_facts(&StatusFacts {
            error: Some(ErrorKind::Permission),
            ..Default::default()
        });
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(
            json,
            r#"{"state":"error","last_sync_ms":null,"uploads":0,"downloads":0,"bytes":0,"paused_until_ms":null,"queued":0,"conflicts":0,"foreign_leases":0,"error":"permission"}"#
        );
        assert_eq!(serde_json::from_str::<Status>(&json).unwrap(), s);
    }
}
