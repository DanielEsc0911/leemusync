//! Wire types of the engine API: handshake, requests, responses and events
//! (architecture §Engine API). The same shapes run in-process and over IPC.

use serde::{Deserialize, Serialize};

use crate::status::{Session, Status};
use crate::{
    ConflictInfo, ConnectionEvent, DoctorReport, ErrorBody, GamesReport, HistoryReport, LeaseInfo,
    StatusReport, SyncReport, TransferProgress,
};

/// First frame a client sends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// The client's `API_VERSION`.
    pub api_version: u32,
    /// Client name, for logs (e.g. `lemusync-cli`).
    pub client: String,
}

/// The daemon's answer to a matching [`Hello`]. A mismatch gets `Response::Error` instead.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloReply {
    /// The daemon's `API_VERSION`.
    pub api_version: u32,
    /// The daemon's crate version.
    pub daemon_version: String,
}

/// A command for the engine. JSON: `{"cmd":"<name>", …fields}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    /// `lemusync status`.
    Status,
    /// Sync now.
    Sync {
        /// Write nothing.
        dry_run: bool,
    },
    /// Pause sync.
    Pause {
        /// End of the pause, Unix milliseconds; `None` pauses until `Resume`.
        until_ms: Option<u64>,
    },
    /// End a pause.
    Resume,
    /// List conflicted streams.
    Conflicts,
    /// List installed games.
    Games,
    /// Versions of one stream.
    History {
        /// Game reference.
        game: String,
        /// Emulator profile id, when the game is ambiguous.
        emu: Option<String>,
        /// Player name; the default player when `None`.
        player: Option<String>,
    },
    /// List leases.
    LeaseStatus,
    /// Run the health checks.
    Doctor,
    /// Turn this connection into an event stream.
    Subscribe,
    /// Stop the daemon.
    Shutdown,
}

/// The answer to one [`Request`]. JSON: `{"ok":"<name>","data":…}` (`data` absent for unit
/// variants).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ok", content = "data", rename_all = "snake_case")]
pub enum Response {
    /// For `Status`.
    Status(StatusReport),
    /// For `Sync`.
    Sync(SyncReport),
    /// For `Pause`.
    Paused,
    /// For `Resume`.
    Resumed,
    /// For `Conflicts`.
    Conflicts(Vec<ConflictInfo>),
    /// For `Games`.
    Games(GamesReport),
    /// For `History`.
    History(HistoryReport),
    /// For `LeaseStatus`.
    LeaseStatus(Vec<LeaseInfo>),
    /// For `Doctor`.
    Doctor(DoctorReport),
    /// For `Subscribe`; events follow on this connection.
    Subscribed,
    /// For `Shutdown`.
    ShuttingDown,
    /// Any request (or the handshake) failed.
    Error(ErrorBody),
}

/// A pushed event. JSON: `{"event":"<name>","data":…}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// The status model changed.
    StatusChanged(Status),
    /// A play session started (`Some`) or ended (`None`).
    SessionChanged(Option<Session>),
    /// A stream got a second head.
    ConflictDetected(ConflictInfo),
    /// Another device holds a live lease.
    LeaseWarning(LeaseInfo),
    /// Transfer progress of one stream.
    TransferProgress(TransferProgress),
    /// Storage connection trouble or recovery.
    Connection(ConnectionEvent),
    /// A background error.
    Error(ErrorBody),
    /// Watch mode: an emulator started while another device has a newer save of one of its
    /// streams. Nothing is restored while it runs (I3); the update waits for it to close.
    RemoteNewer(RemoteNewer),
}

/// The `RemoteNewer` event: a stream's newest head was written by another device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteNewer {
    /// Player name.
    pub player: String,
    /// Emulator profile id.
    pub emulator: String,
    /// Game id.
    pub game: String,
    /// The game's name on this device, when known. Not synced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Name of the device that wrote the newer save (its id when unknown).
    pub device: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::StatusFacts;
    use crate::{API_VERSION, TransferPhase};

    fn golden<T>(v: &T, want: &str)
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(v).unwrap();
        assert_eq!(json, want);
        assert_eq!(&serde_json::from_str::<T>(&json).unwrap(), v);
    }

    fn round<T>(v: &T)
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(v).unwrap();
        assert_eq!(&serde_json::from_str::<T>(&json).unwrap(), v, "{json}");
    }

    fn err() -> ErrorBody {
        ErrorBody {
            code: "auth".into(),
            message: "m".into(),
            detail: None,
        }
    }

    fn lease() -> LeaseInfo {
        LeaseInfo {
            player: "A".into(),
            emulator: "cemu".into(),
            game: "g".into(),
            device_id: "d1".into(),
            device: "pc".into(),
            renewed_at_ms: 5,
            live: true,
            ours: false,
        }
    }

    fn conflict() -> ConflictInfo {
        ConflictInfo {
            player: "A".into(),
            emulator: "cemu".into(),
            game: "g".into(),
            name: None,
            heads: vec![],
        }
    }

    #[test]
    fn handshake_json_is_stable() {
        golden(
            &Hello {
                api_version: API_VERSION,
                client: "cli".into(),
            },
            r#"{"api_version":1,"client":"cli"}"#,
        );
        golden(
            &HelloReply {
                api_version: API_VERSION,
                daemon_version: "0.1.0".into(),
            },
            r#"{"api_version":1,"daemon_version":"0.1.0"}"#,
        );
    }

    #[test]
    fn request_json_is_stable() {
        golden(&Request::Status, r#"{"cmd":"status"}"#);
        golden(
            &Request::Sync { dry_run: true },
            r#"{"cmd":"sync","dry_run":true}"#,
        );
        golden(
            &Request::Pause { until_ms: Some(9) },
            r#"{"cmd":"pause","until_ms":9}"#,
        );
        golden(&Request::Resume, r#"{"cmd":"resume"}"#);
        golden(&Request::Conflicts, r#"{"cmd":"conflicts"}"#);
        golden(&Request::Games, r#"{"cmd":"games"}"#);
        golden(
            &Request::History {
                game: "botw".into(),
                emu: None,
                player: Some("A".into()),
            },
            r#"{"cmd":"history","game":"botw","emu":null,"player":"A"}"#,
        );
        golden(&Request::LeaseStatus, r#"{"cmd":"lease_status"}"#);
        golden(&Request::Doctor, r#"{"cmd":"doctor"}"#);
        golden(&Request::Subscribe, r#"{"cmd":"subscribe"}"#);
        golden(&Request::Shutdown, r#"{"cmd":"shutdown"}"#);
        round(&Request::Pause { until_ms: None });
    }

    #[test]
    fn response_json_is_stable() {
        golden(&Response::Paused, r#"{"ok":"paused"}"#);
        golden(&Response::Resumed, r#"{"ok":"resumed"}"#);
        golden(&Response::Subscribed, r#"{"ok":"subscribed"}"#);
        golden(&Response::ShuttingDown, r#"{"ok":"shutting_down"}"#);
        golden(
            &Response::Error(err()),
            r#"{"ok":"error","data":{"code":"auth","message":"m"}}"#,
        );
        golden(
            &Response::Doctor(DoctorReport {
                api_version: API_VERSION,
                checks: vec![],
            }),
            r#"{"ok":"doctor","data":{"api_version":1,"checks":[]}}"#,
        );
        golden(
            &Response::Conflicts(vec![]),
            r#"{"ok":"conflicts","data":[]}"#,
        );
        golden(
            &Response::LeaseStatus(vec![]),
            r#"{"ok":"lease_status","data":[]}"#,
        );
        golden(
            &Response::Games(GamesReport {
                api_version: API_VERSION,
                games: vec![],
            }),
            r#"{"ok":"games","data":{"api_version":1,"games":[]}}"#,
        );
        golden(
            &Response::History(HistoryReport {
                api_version: API_VERSION,
                name: None,
                versions: vec![],
                backups: vec![],
            }),
            r#"{"ok":"history","data":{"api_version":1,"versions":[],"backups":[]}}"#,
        );
        golden(
            &Response::Sync(SyncReport {
                api_version: API_VERSION,
                dry_run: false,
                streams: vec![],
                warnings: vec![],
            }),
            r#"{"ok":"sync","data":{"api_version":1,"dry_run":false,"streams":[],"warnings":[]}}"#,
        );
        golden(
            &Response::Status(StatusReport {
                api_version: API_VERSION,
                device: "pc".into(),
                device_id: "d".into(),
                repo: None,
                backend: None,
                grants: vec![],
                players: vec![],
                streams: vec![],
            }),
            r#"{"ok":"status","data":{"api_version":1,"device":"pc","device_id":"d","repo":null,"backend":null,"grants":[],"players":[],"streams":[]}}"#,
        );
    }

    #[test]
    fn event_json_is_stable() {
        golden(
            &Event::StatusChanged(Status::from_facts(&StatusFacts::default())),
            r#"{"event":"status_changed","data":{"state":"synced","last_sync_ms":null,"uploads":0,"downloads":0,"bytes":0,"paused_until_ms":null,"queued":0,"conflicts":0,"foreign_leases":0,"error":null}}"#,
        );
        golden(
            &Event::SessionChanged(None),
            r#"{"event":"session_changed","data":null}"#,
        );
        golden(
            &Event::SessionChanged(Some(Session {
                game: "g".into(),
                name: Some("BotW".into()),
                emulator: "cemu".into(),
                player: "A".into(),
                device: "pc".into(),
                started_ms: 1,
            })),
            r#"{"event":"session_changed","data":{"game":"g","name":"BotW","emulator":"cemu","player":"A","device":"pc","started_ms":1}}"#,
        );
        golden(
            &Event::ConflictDetected(conflict()),
            r#"{"event":"conflict_detected","data":{"player":"A","emulator":"cemu","game":"g","heads":[]}}"#,
        );
        golden(
            &Event::LeaseWarning(lease()),
            r#"{"event":"lease_warning","data":{"player":"A","emulator":"cemu","game":"g","device_id":"d1","device":"pc","renewed_at_ms":5,"live":true,"ours":false}}"#,
        );
        golden(
            &Event::TransferProgress(TransferProgress {
                player: "A".into(),
                emulator: "cemu".into(),
                game: "g".into(),
                phase: TransferPhase::Uploading,
                files_done: 1,
                files_total: 2,
                bytes_done: 3,
                bytes_total: 4,
            }),
            r#"{"event":"transfer_progress","data":{"player":"A","emulator":"cemu","game":"g","phase":"uploading","files_done":1,"files_total":2,"bytes_done":3,"bytes_total":4}}"#,
        );
        golden(
            &Event::Connection(ConnectionEvent::Recovered),
            r#"{"event":"connection","data":{"kind":"recovered"}}"#,
        );
        golden(
            &Event::Error(err()),
            r#"{"event":"error","data":{"code":"auth","message":"m"}}"#,
        );
        golden(
            &Event::RemoteNewer(RemoteNewer {
                player: "A".into(),
                emulator: "cemu".into(),
                game: "g".into(),
                name: None,
                device: "deck".into(),
            }),
            r#"{"event":"remote_newer","data":{"player":"A","emulator":"cemu","game":"g","device":"deck"}}"#,
        );
    }

    #[test]
    fn unknown_command_is_an_error() {
        assert!(serde_json::from_str::<Request>(r#"{"cmd":"rm_rf"}"#).is_err());
        assert!(serde_json::from_str::<Request>(r#"{"cmd":"sync"}"#).is_err());
    }
}
