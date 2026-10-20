//! Local SQLite state DB and journal (D12, reliability §Restore procedure).

use std::fs::File;
use std::path::Path;

use leemusync_core::manifest::FileEntry;
use leemusync_core::{PlayerId, StreamId, VersionId};
use rusqlite::{Connection, OptionalExtension, params};

use crate::EngineError;

const SCHEMA: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE streams (
  stream_id TEXT PRIMARY KEY,
  player_id TEXT NOT NULL,
  emulator TEXT NOT NULL,
  game TEXT NOT NULL,
  base_version TEXT,
  base_files TEXT NOT NULL,
  highest_seen TEXT,
  replace_local INTEGER NOT NULL
);
CREATE TABLE manifests (stream_id TEXT NOT NULL, version_id TEXT NOT NULL, body BLOB NOT NULL,
  PRIMARY KEY (stream_id, version_id));
CREATE TABLE config_versions (version_id TEXT PRIMARY KEY, body BLOB NOT NULL);
CREATE TABLE journal (op_id TEXT PRIMARY KEY, stream_id TEXT NOT NULL, kind TEXT NOT NULL,
  step TEXT NOT NULL, data TEXT NOT NULL);
PRAGMA user_version = 1;
";

/// The `PRAGMA user_version` this build writes and reads.
const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamRow {
    pub stream: StreamId,
    pub player: PlayerId,
    pub emulator: String,
    pub game: String,
    pub base: Option<VersionId>,
    pub base_files: Vec<FileEntry>,
    pub highest_seen: Option<VersionId>,
    pub replace_local: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalEntry {
    pub op: String,
    pub stream: StreamId,
    pub kind: String,
    pub step: String,
    pub data: String,
}

pub struct State {
    conn: Connection,
    _lock: File,
}

fn parse<T: std::str::FromStr>(text: &str) -> Result<T, EngineError> {
    text.parse()
        .map_err(|_| EngineError::Io("state database holds an invalid id".into()))
}

/// 0 → create the schema and stamp 1 in one transaction; 1 → as is; newer → refuse.
fn migrate(conn: &mut Connection) -> Result<(), EngineError> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let version: u32 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    match version {
        0 => {
            tx.execute_batch(SCHEMA)?;
            tx.commit()?;
            Ok(())
        }
        SCHEMA_VERSION => Ok(()),
        newer => Err(EngineError::Data(format!("state.db user_version {newer}"))),
    }
}

impl State {
    /// Opens (and creates) `<data_dir>/state.db` and holds `<data_dir>/lock` until dropped.
    ///
    /// # Errors
    /// `Locked` when another engine holds the lock; `Io` otherwise.
    pub fn open(data_dir: &Path) -> Result<Self, EngineError> {
        std::fs::create_dir_all(data_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(data_dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let lock = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(data_dir.join("lock"))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(EngineError::Locked(data_dir.display().to_string()));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
        }
        let mut conn = Connection::open(data_dir.join("state.db"))?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;")?;
        migrate(&mut conn)?;
        Ok(Self { conn, _lock: lock })
    }

    /// # Errors
    /// `Io` for database errors or corrupt rows.
    pub fn stream(&self, id: &StreamId) -> Result<Option<StreamRow>, EngineError> {
        let row = self
            .conn
            .query_row(
                "SELECT player_id, emulator, game, base_version, base_files, highest_seen, replace_local
                 FROM streams WHERE stream_id = ?1",
                params![id.to_string()],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, Option<String>>(5)?,
                        r.get::<_, bool>(6)?,
                    ))
                },
            )
            .optional()?;
        let Some((player, emulator, game, base, files, highest, replace)) = row else {
            return Ok(None);
        };
        Ok(Some(StreamRow {
            stream: *id,
            player: PlayerId(parse(&player)?),
            emulator,
            game,
            base: base.as_deref().map(parse).transpose()?,
            base_files: serde_json::from_str(&files).map_err(|e| EngineError::Io(e.to_string()))?,
            highest_seen: highest.as_deref().map(parse).transpose()?,
            replace_local: replace,
        }))
    }

    /// # Errors
    /// `Io` for database errors.
    pub fn put_stream(&self, row: &StreamRow) -> Result<(), EngineError> {
        let files =
            serde_json::to_string(&row.base_files).map_err(|e| EngineError::Io(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO streams (stream_id, player_id, emulator, game, base_version, base_files, highest_seen, replace_local)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(stream_id) DO UPDATE SET player_id = ?2, emulator = ?3, game = ?4, base_version = ?5,
               base_files = ?6, highest_seen = ?7, replace_local = ?8",
            params![
                row.stream.to_string(),
                row.player.to_string(),
                row.emulator,
                row.game,
                row.base.map(|v| v.to_string()),
                files,
                row.highest_seen.map(|v| v.to_string()),
                row.replace_local,
            ],
        )?;
        Ok(())
    }

    /// # Errors
    /// `Io` for database errors.
    pub fn cached_manifest(
        &self,
        stream: &StreamId,
        id: &VersionId,
    ) -> Result<Option<Vec<u8>>, EngineError> {
        Ok(self
            .conn
            .query_row(
                "SELECT body FROM manifests WHERE stream_id = ?1 AND version_id = ?2",
                params![stream.to_string(), id.to_string()],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// # Errors
    /// `Io` for database errors.
    pub fn cache_manifest(
        &self,
        stream: &StreamId,
        id: &VersionId,
        json: &[u8],
    ) -> Result<(), EngineError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO manifests (stream_id, version_id, body) VALUES (?1, ?2, ?3)",
            params![stream.to_string(), id.to_string(), json],
        )?;
        Ok(())
    }

    /// # Errors
    /// `Io` for database errors.
    pub fn cached_config(&self, id: &VersionId) -> Result<Option<Vec<u8>>, EngineError> {
        Ok(self
            .conn
            .query_row(
                "SELECT body FROM config_versions WHERE version_id = ?1",
                params![id.to_string()],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// # Errors
    /// `Io` for database errors.
    pub fn cache_config(&self, id: &VersionId, json: &[u8]) -> Result<(), EngineError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO config_versions (version_id, body) VALUES (?1, ?2)",
            params![id.to_string(), json],
        )?;
        Ok(())
    }

    /// # Errors
    /// `Io` for database errors.
    pub fn journal_put(&self, e: &JournalEntry) -> Result<(), EngineError> {
        self.conn.execute(
            "INSERT INTO journal (op_id, stream_id, kind, step, data) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(op_id) DO UPDATE SET step = ?4, data = ?5",
            params![e.op, e.stream.to_string(), e.kind, e.step, e.data],
        )?;
        Ok(())
    }

    /// # Errors
    /// `Io` for database errors or corrupt rows.
    pub fn journal_all(&self) -> Result<Vec<JournalEntry>, EngineError> {
        let mut stmt = self
            .conn
            .prepare("SELECT op_id, stream_id, kind, step, data FROM journal ORDER BY op_id")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
            ))
        })?;
        rows.map(|row| {
            let (op, stream, kind, step, data) = row?;
            let hash = leemusync_core::Hash32::from_hex(&stream)
                .map_err(|e| EngineError::Io(e.to_string()))?;
            Ok(JournalEntry {
                op,
                stream: StreamId(hash),
                kind,
                step,
                data,
            })
        })
        .collect()
    }

    /// # Errors
    /// `Io` for database errors.
    pub fn journal_delete(&self, op: &str) -> Result<(), EngineError> {
        self.conn
            .execute("DELETE FROM journal WHERE op_id = ?1", params![op])?;
        Ok(())
    }

    /// # Errors
    /// `Io` for database errors.
    pub fn meta(&self, key: &str) -> Result<Option<String>, EngineError> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// # Errors
    /// `Io` for database errors.
    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), EngineError> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leemusync_core::{Hash32, VersionId};

    fn row() -> StreamRow {
        StreamRow {
            stream: StreamId(Hash32([1; 32])),
            player: PlayerId(VersionId::from_parts(1, [1; 10])),
            emulator: "cemu".into(),
            game: "101c9400".into(),
            base: Some(VersionId::from_parts(2, [2; 10])),
            base_files: vec![],
            highest_seen: None,
            replace_local: true,
        }
    }

    #[test]
    fn streams_journal_and_meta_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::open(dir.path()).unwrap();
        state.put_stream(&row()).unwrap();
        assert_eq!(state.stream(&row().stream).unwrap(), Some(row()));
        let e = JournalEntry {
            op: "op1".into(),
            stream: row().stream,
            kind: "restore".into(),
            step: "swap".into(),
            data: "{}".into(),
        };
        state.journal_put(&e).unwrap();
        assert_eq!(state.journal_all().unwrap(), vec![e]);
        state.journal_delete("op1").unwrap();
        assert_eq!(state.journal_all().unwrap(), Vec::<JournalEntry>::new());
        state.set_meta("k", "v").unwrap();
        assert_eq!(state.meta("k").unwrap().as_deref(), Some("v"));
    }

    #[test]
    fn a_second_engine_on_the_same_data_dir_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let _first = State::open(dir.path()).unwrap();
        assert!(matches!(
            State::open(dir.path()),
            Err(EngineError::Locked(_))
        ));
    }

    #[test]
    fn a_newer_state_db_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        {
            let state = State::open(dir.path()).unwrap();
            assert_eq!(state.meta("x").unwrap(), None);
        }
        {
            let conn = Connection::open(dir.path().join("state.db")).unwrap();
            conn.execute_batch("PRAGMA user_version = 2;").unwrap();
        }
        match State::open(dir.path()) {
            Err(EngineError::Data(v)) => assert!(v.contains('2'), "{v}"),
            Err(e) => panic!("wrong error {e}"),
            Ok(_) => panic!("opened a newer state DB"),
        }
    }

    #[test]
    fn reopening_keeps_rows_and_version_1() {
        let dir = tempfile::tempdir().unwrap();
        State::open(dir.path()).unwrap().set_meta("k", "v").unwrap();
        let state = State::open(dir.path()).unwrap();
        assert_eq!(state.meta("k").unwrap().as_deref(), Some("v"));
        let v: u32 = state
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1);
    }
}
