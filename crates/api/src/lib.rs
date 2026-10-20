//! Engine API: requests, responses, events and the engine trait. Versioned.
//! Spec: docs/specs/architecture.md (Engine API).

use serde::{Deserialize, Serialize};

/// Version of the report and request shapes in this crate. Bumped on breaking changes.
pub const API_VERSION: u32 = 1;

/// Result of `leemusync sync`: one entry per stream, plus warnings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncReport {
    /// Shape version, always `API_VERSION`.
    pub api_version: u32,
    /// True when nothing was written.
    pub dry_run: bool,
    /// One entry per stream the run looked at.
    pub streams: Vec<StreamReport>,
    /// Non-fatal findings for the whole run.
    pub warnings: Vec<Warning>,
}

/// Outcome for one player, emulator, game and account stream.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamReport {
    /// Player name as the user sees it.
    pub player: String,
    /// Emulator profile id.
    pub emulator: String,
    /// Game id (emulator-specific title id).
    pub game: String,
    /// Emulator account id, when the stream is tied to one.
    pub account: Option<String>,
    /// What the sync decided to do for this stream.
    pub action: StreamAction,
    /// Version id the stream ended on, when there is one.
    pub version: Option<String>,
    /// Current head version ids of the stream.
    pub heads: Vec<String>,
    /// True when the stream finished its work in this run.
    pub done: bool,
}

/// What a sync run decided for one stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamAction {
    /// Already in sync; nothing to do.
    Nothing,
    /// Local saves adopted as the stream's first version.
    Adopt,
    /// Local side was behind; remote version applied.
    FastForward,
    /// Local version uploaded.
    Upload,
    /// Local version uploaded, and the stream now has concurrent heads.
    UploadThenConflict,
    /// Concurrent heads exist and need a decision.
    Conflict,
    /// Local saves are missing for this stream.
    LocalMissing,
    /// Remote side is behind the local one.
    RemoteBehind,
    /// The stream needs an emulator account mapped first.
    NeedsAccount,
    /// The stream was skipped on purpose.
    Skipped,
}

/// A non-fatal finding, with a stable code and display detail.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    /// Stable machine-readable code.
    pub code: WarningCode,
    /// Display text, never translated.
    pub detail: String,
}

/// Stable codes for `Warning`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningCode {
    /// The emulator is running, so saves may change under the sync.
    EmulatorRunning,
    /// A save folder is not granted to `LeemuSync`.
    NotGranted,
    /// A shared folder that should be empty is not.
    SharedFolderNotEmpty,
    /// A save exists for an account that no player uses.
    OrphanedSave,
    /// Config from another device was merged in.
    ConfigMerged,
    /// A player was renamed during the run.
    PlayerRenamed,
    /// The remote side is behind the local one.
    RemoteBehind,
    /// A restore was rolled back after it failed.
    RestoreRolledBack,
    /// A version id could not be parsed.
    InvalidVersion,
    /// `init` created the repo, but a later step failed on this device; `detail` is the error
    /// code. `leemusync join` with the passphrase finishes the setup.
    SetupIncomplete,
    /// A save folder could not be read safely (symlink or I/O error); `detail` is its path.
    UnreadableFolder,
}

/// Result of `leemusync scan`: what was found on this machine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanReport {
    /// Shape version, always `API_VERSION`.
    pub api_version: u32,
    /// One entry per emulator install found.
    pub installs: Vec<InstallReport>,
    /// Non-fatal findings for the scan.
    pub warnings: Vec<Warning>,
}

/// One emulator install found by a scan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallReport {
    /// Emulator profile id.
    pub emulator: String,
    /// Base folder of the install.
    pub base: String,
    /// True when the base folder is granted to `LeemuSync`.
    pub granted: bool,
    /// Folders that still need a grant.
    pub needs_grant: Vec<String>,
    /// Accounts found under this install.
    pub accounts: Vec<AccountReport>,
    /// Saves found under this install.
    pub saves: Vec<SaveReport>,
}

/// One emulator account found by a scan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountReport {
    /// Account id as the emulator names it.
    pub id: String,
    /// Display name, when the emulator provides one.
    pub name: Option<String>,
    /// Player this account is mapped to, if any.
    pub player: Option<String>,
}

/// Save data found for one game and account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveReport {
    /// Game id.
    pub game: String,
    /// Account id the save belongs to.
    pub account: String,
    /// True when no player uses this account.
    pub orphaned: bool,
    /// Number of files in the save.
    pub files: u32,
    /// Total size of the save in bytes.
    pub bytes: u64,
}

/// Result of `leemusync status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusReport {
    /// Shape version, always `API_VERSION`.
    pub api_version: u32,
    /// This device's name.
    pub device: String,
    /// This device's id.
    pub device_id: String,
    /// Repo location, when a repo is set up.
    pub repo: Option<String>,
    /// Backend label, when one is configured.
    pub backend: Option<String>,
    /// Granted folders.
    pub grants: Vec<String>,
    /// Player names.
    pub players: Vec<String>,
    /// Status of each stream.
    pub streams: Vec<StreamStatus>,
}

/// Status of one stream as shown by `status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamStatus {
    /// Player name.
    pub player: String,
    /// Emulator profile id.
    pub emulator: String,
    /// Game id.
    pub game: String,
    /// Account id, when the stream is tied to one.
    pub account: Option<String>,
    /// Base folder of the local saves, when known.
    pub base: Option<String>,
}

/// Result of `leemusync history`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryReport {
    /// Shape version, always `API_VERSION`.
    pub api_version: u32,
    /// Versions of the stream, newest first.
    pub versions: Vec<HistoryEntry>,
    /// Local backups available for restore.
    pub backups: Vec<String>,
}

/// One version in a history listing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Version id.
    pub id: String,
    /// Creation time as Unix seconds.
    pub created_at: u64,
    /// Name of the device that made the version.
    pub device: String,
    /// Number of files in the version.
    pub files: u32,
    /// Total size in bytes.
    pub bytes: u64,
    /// Parent version ids.
    pub parents: Vec<String>,
    /// True when this version is a current head.
    pub head: bool,
}

/// Result of `leemusync init`. The recovery key is never included.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitReport {
    /// Shape version, always `API_VERSION`.
    pub api_version: u32,
    /// Repo location that was created.
    pub repo: String,
    /// Name of this device.
    pub device: String,
    /// Non-fatal findings, e.g. `SetupIncomplete`.
    pub warnings: Vec<Warning>,
}

/// Result of `leemusync doctor`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorReport {
    /// Shape version, always `API_VERSION`.
    pub api_version: u32,
    /// Results of each check, in run order.
    pub checks: Vec<Check>,
}

/// One named health check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    /// Check name.
    pub name: String,
    /// True when the check passed.
    pub ok: bool,
    /// Display text with the result detail.
    pub detail: String,
}

/// Error output for any command run with `--json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorReport {
    /// Shape version, always `API_VERSION`.
    pub api_version: u32,
    /// The error itself.
    pub error: ErrorBody,
}

/// Machine code and display message of an error.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    /// Stable machine-readable code.
    pub code: String,
    /// Display text, never translated.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_report_json_is_stable() {
        let r = SyncReport {
            api_version: API_VERSION,
            dry_run: true,
            streams: vec![StreamReport {
                player: "A".into(),
                emulator: "cemu".into(),
                game: "101c9400".into(),
                account: Some("80000001".into()),
                action: StreamAction::UploadThenConflict,
                version: None,
                heads: vec![],
                done: false,
            }],
            warnings: vec![Warning {
                code: WarningCode::OrphanedSave,
                detail: "80000002".into(),
            }],
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(
            json,
            r#"{"api_version":1,"dry_run":true,"streams":[{"player":"A","emulator":"cemu","game":"101c9400","account":"80000001","action":"upload_then_conflict","version":null,"heads":[],"done":false}],"warnings":[{"code":"orphaned_save","detail":"80000002"}]}"#
        );
        assert_eq!(serde_json::from_str::<SyncReport>(&json).unwrap(), r);
        // StreamAction and WarningCode are Copy: values can be taken out of borrowed reports.
        let a = r.streams[0].action;
        assert_eq!(a, StreamAction::UploadThenConflict);
        let c = r.warnings[0].code;
        assert_eq!(c, WarningCode::OrphanedSave);
    }

    #[test]
    fn init_report_carries_setup_warnings() {
        let r = InitReport {
            api_version: API_VERSION,
            repo: "0123456789abcdef0123456789abcdef".into(),
            device: "pc".into(),
            warnings: vec![Warning {
                code: WarningCode::SetupIncomplete,
                detail: "secrets".into(),
            }],
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(
            json,
            r#"{"api_version":1,"repo":"0123456789abcdef0123456789abcdef","device":"pc","warnings":[{"code":"setup_incomplete","detail":"secrets"}]}"#
        );
        assert_eq!(serde_json::from_str::<InitReport>(&json).unwrap(), r);
    }
}
