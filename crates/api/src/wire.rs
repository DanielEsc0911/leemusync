//! Wire types of the engine API: handshake, requests, responses and events
//! (architecture §Engine API). The same shapes run in-process and over IPC.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::status::{Session, Status};
use crate::{
    ConflictInfo, ConnectionEvent, CreatedAccount, DoctorReport, ErrorBody, GamesReport,
    HistoryReport, InitReport, LeaseInfo, PlayEvent, PlayReport, ScanReport, Secret, StatusReport,
    StreamReport, SyncReport, TransferProgress, Warning,
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

/// A command for the engine. JSON: `{"cmd":"<name>", …fields}`. Every CLI command but
/// `service`, `logs` and the Google Drive login has one (D35); secrets travel as [`Secret`]s,
/// which `Debug` never shows.
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
        /// Player name; the only player when `None`.
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
    /// `init`: create the repo.
    Init {
        /// The new passphrase.
        passphrase: Secret,
    },
    /// `join`: unlock the repo on this device.
    Join {
        /// Passphrase or recovery key.
        secret: JoinWire,
    },
    /// `backend add fs <path>`.
    BackendAddFs {
        /// Folder path.
        path: String,
    },
    /// `backend add s3`.
    BackendAddS3 {
        /// `https://` endpoint.
        endpoint: String,
        /// Bucket name.
        bucket: String,
        /// Region.
        region: String,
        /// Access key id.
        access_key_id: String,
        /// Secret access key (stored in the keystore).
        secret: Secret,
    },
    /// `grant add <path>`.
    GrantAdd {
        /// Folder path.
        path: String,
    },
    /// `grant list`.
    GrantList,
    /// `players add <name>`.
    PlayersAdd {
        /// Player name.
        name: String,
    },
    /// `players list`.
    PlayersList,
    /// `scan`.
    Scan,
    /// `accounts map`.
    AccountsMap {
        /// Player name.
        player: String,
        /// Emulator profile id.
        emulator: String,
        /// Emulator account id.
        account: String,
        /// What to do with files on both sides.
        choice: MapChoiceWire,
    },
    /// `accounts create`.
    AccountsCreate {
        /// Player name.
        player: String,
        /// Emulator profile id.
        emulator: String,
        /// Account display name.
        name: Option<String>,
    },
    /// `accounts recover`.
    AccountsRecover {
        /// Emulator profile id.
        emulator: String,
        /// Emulator account id.
        account: String,
        /// Player name.
        player: String,
    },
    /// The user confirmed the first sync on this device.
    ConfirmFirstSync,
    /// Resolve a game reference to `(emulator, game)`.
    ResolveGame {
        /// Game reference.
        game: String,
        /// Emulator profile id, when ambiguous.
        emu: Option<String>,
    },
    /// The game names known on this device.
    GameNames,
    /// `restore`.
    Restore {
        /// Player name.
        player: String,
        /// Emulator profile id.
        emulator: String,
        /// Game id.
        game: String,
        /// Version id or backup name.
        what: String,
    },
    /// `resolve`.
    Resolve {
        /// Player name.
        player: String,
        /// Emulator profile id.
        emulator: String,
        /// Game id.
        game: String,
        /// The head to keep.
        keep: String,
    },
    /// `lease release --force <game>`.
    LeaseRelease {
        /// Game id.
        game: String,
    },
    /// `games alias`.
    AliasAdd {
        /// Game reference.
        game: String,
        /// Emulator profile id, when ambiguous.
        emu: Option<String>,
        /// New alias.
        alias: String,
    },
    /// `games unalias`.
    AliasRemove {
        /// Alias.
        alias: String,
    },
    /// `config …`.
    Config(ConfigOp),
    /// `play`: `play_event` and `prompt` frames follow, then `play` or `error`. The client
    /// answers each `prompt` with an `answer` request.
    Play {
        /// Player name.
        player: String,
        /// Emulator profile id.
        emulator: String,
        /// Game id.
        game: String,
        /// Extra emulator arguments.
        args: Vec<String>,
    },
    /// The answer to a `prompt` frame during `play`.
    Answer(PlayAnswer),
}

/// `join`'s secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum JoinWire {
    /// The repo passphrase.
    Passphrase(Secret),
    /// The recovery key.
    Recovery(Secret),
}

/// `accounts map`'s choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapChoiceWire {
    /// Ask when both sides have files.
    Ask,
    /// Keep the player's versions.
    Keep,
    /// Replace them with the account's files.
    Replace,
}

/// What `accounts map` did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum MapResult {
    /// Mapped.
    Mapped,
    /// Both sides have files: keep or replace?
    NeedsChoice {
        /// The games on both sides.
        games: Vec<String>,
    },
}

/// One `config` subcommand.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ConfigOp {
    /// `config path`.
    Path,
    /// `config list`.
    List,
    /// `config get`.
    Get {
        /// Key.
        key: String,
    },
    /// `config set`.
    Set {
        /// Key.
        key: String,
        /// Values.
        values: Vec<String>,
    },
    /// `config unset`.
    Unset {
        /// Key.
        key: String,
    },
}

/// A question `play` asks the player through the client.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ask", rename_all = "snake_case")]
pub enum PlayQuestion {
    /// Another device holds the lease.
    Lease {
        /// Holder's device name.
        by: String,
        /// Seconds since its last renewal.
        ago_s: u64,
        /// The lease expired.
        stale: bool,
    },
    /// The stream has a conflict.
    Conflict {
        /// The conflict.
        conflict: ConflictInfo,
    },
}

/// The player's answer to a [`PlayQuestion`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum PlayAnswer {
    /// For `Lease`.
    Lease {
        /// The choice.
        choice: LeaseAnswer,
    },
    /// For `Conflict`: the head to keep, or `None` to play this device's copy.
    Keep {
        /// Version id.
        version: Option<String>,
    },
    /// No answer: the non-interactive outcome (exit 5 for a lease, 4 for a conflict). A
    /// dropped connection means the same.
    Refuse,
}

/// The lease choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseAnswer {
    /// Wait until the lease is free.
    Wait,
    /// Play without the lease.
    PlayAnyway,
    /// Take over a stale lease.
    TakeOver,
}

/// `(emulator, game)`, from `resolve_game` and `alias_add`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameRef {
    /// Emulator profile id.
    pub emulator: String,
    /// Game id.
    pub game: String,
}

/// One known game name on this device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameName {
    /// Emulator profile id.
    pub emulator: String,
    /// Game key, lowercase.
    pub key: String,
    /// Display name.
    pub name: String,
}

/// A result with its non-fatal warnings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WithWarnings<T> {
    /// The result.
    pub value: T,
    /// Non-fatal findings.
    pub warnings: Vec<Warning>,
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
    Conflicts(WithWarnings<Vec<ConflictInfo>>),
    /// For `Games`.
    Games(GamesReport),
    /// For `History`.
    History(HistoryReport),
    /// For `LeaseStatus`.
    LeaseStatus(WithWarnings<Vec<LeaseInfo>>),
    /// For `Doctor`.
    Doctor(DoctorReport),
    /// For `Subscribe`; events follow on this connection.
    Subscribed,
    /// For `Shutdown`.
    ShuttingDown,
    /// For requests with nothing to report.
    Done,
    /// For `Init`. The recovery key is shown once, by the client.
    Init {
        /// The report.
        report: InitReport,
        /// The recovery key.
        recovery: Secret,
    },
    /// For `Join`.
    Join(InitReport),
    /// For `GrantAdd` and `ConfigOp::Path`.
    Path(String),
    /// For `GrantList` and `PlayersList`.
    Names(Vec<String>),
    /// For `PlayersAdd`.
    Warnings(Vec<Warning>),
    /// For `Scan`.
    Scan(ScanReport),
    /// For `AccountsMap`.
    Map(MapResult),
    /// For `AccountsCreate`.
    Created(CreatedAccount),
    /// For `AccountsRecover` and `Restore`.
    Stream(StreamReport),
    /// For `ResolveGame` and `AliasAdd`.
    Game(GameRef),
    /// For `GameNames`.
    GameNames(Vec<GameName>),
    /// For `Resolve`.
    Resolved(WithWarnings<StreamReport>),
    /// For `LeaseRelease`: how many leases were deleted.
    Released(u32),
    /// For `ConfigOp::List`.
    ConfigValues(BTreeMap<String, serde_json::Value>),
    /// For `ConfigOp::Get`.
    ConfigValue(serde_json::Value),
    /// During `Play`: a step.
    PlayEvent(PlayEvent),
    /// During `Play`: a question; answer with `Request::Answer`.
    Prompt(PlayQuestion),
    /// For `Play`, once it ends.
    Play(PlayReport),
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
            &Response::Conflicts(WithWarnings {
                value: vec![],
                warnings: vec![],
            }),
            r#"{"ok":"conflicts","data":{"value":[],"warnings":[]}}"#,
        );
        golden(
            &Response::LeaseStatus(WithWarnings {
                value: vec![],
                warnings: vec![],
            }),
            r#"{"ok":"lease_status","data":{"value":[],"warnings":[]}}"#,
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
                daemon: None,
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
            &Event::StatusChanged(Status::from_facts(&StatusFacts {
                error: Some(crate::ErrorKind::NotSetUp),
                conflicts: 1,
                ..StatusFacts::default()
            })),
            r#"{"event":"status_changed","data":{"state":"error","last_sync_ms":null,"uploads":0,"downloads":0,"bytes":0,"paused_until_ms":null,"queued":0,"conflicts":1,"foreign_leases":0,"error":"not_set_up"}}"#,
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

    fn secret(s: &str) -> Secret {
        Secret::new(zeroize::Zeroizing::new(s.into()))
    }

    #[test]
    fn new_requests_json_is_stable() {
        golden(
            &Request::Init {
                passphrase: secret("p"),
            },
            r#"{"cmd":"init","passphrase":"p"}"#,
        );
        golden(
            &Request::Join {
                secret: JoinWire::Recovery(secret("r")),
            },
            r#"{"cmd":"join","secret":{"kind":"recovery","value":"r"}}"#,
        );
        golden(
            &Request::Config(ConfigOp::Get { key: "k".into() }),
            r#"{"cmd":"config","op":"get","key":"k"}"#,
        );
        golden(
            &Request::Answer(PlayAnswer::Lease {
                choice: LeaseAnswer::PlayAnyway,
            }),
            r#"{"cmd":"answer","answer":"lease","choice":"play_anyway"}"#,
        );
        golden(
            &Request::Answer(PlayAnswer::Refuse),
            r#"{"cmd":"answer","answer":"refuse"}"#,
        );
        golden(
            &Response::Prompt(PlayQuestion::Lease {
                by: "Deck".into(),
                ago_s: 3,
                stale: false,
            }),
            r#"{"ok":"prompt","data":{"ask":"lease","by":"Deck","ago_s":3,"stale":false}}"#,
        );
        golden(&Response::Done, r#"{"ok":"done"}"#);
        golden(
            &Response::Map(MapResult::NeedsChoice {
                games: vec!["g".into()],
            }),
            r#"{"ok":"map","data":{"outcome":"needs_choice","games":["g"]}}"#,
        );
        for r in [
            Request::BackendAddS3 {
                endpoint: "https://e".into(),
                bucket: "b".into(),
                region: "auto".into(),
                access_key_id: "AK".into(),
                secret: secret("s3cr3t-value"),
            },
            Request::Init {
                passphrase: secret("s3cr3t-value"),
            },
            Request::Join {
                secret: JoinWire::Passphrase(secret("s3cr3t-value")),
            },
        ] {
            round(&r);
            assert!(!format!("{r:?}").contains("s3cr3t"), "{r:?}");
        }
        let init = Response::Init {
            report: InitReport {
                api_version: API_VERSION,
                repo: "r".into(),
                device: "d".into(),
                warnings: vec![],
            },
            recovery: secret("s3cr3t-value"),
        };
        round(&init);
        assert!(!format!("{init:?}").contains("s3cr3t"));
    }

    #[test]
    fn unknown_command_is_an_error() {
        assert!(serde_json::from_str::<Request>(r#"{"cmd":"rm_rf"}"#).is_err());
        assert!(serde_json::from_str::<Request>(r#"{"cmd":"sync"}"#).is_err());
    }
}
