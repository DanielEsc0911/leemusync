//! Staged, journaled, atomic restore with a backup first (reliability §Restore procedure).
//!
//! Order: confine → stage and verify (I6) → emulator closed (I3) → durable backup (I1) →
//! journal (I5) → atomic swap (I4) → done. A crash after the journal entry is rolled back to
//! the backup by `recover`, so a save folder is never left half old, half new.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use futures_util::{StreamExt, TryStreamExt, stream};
use lemusync_api::{TransferPhase, Warning, WarningCode};
use lemusync_core::manifest::Version;
use lemusync_core::{RelPath, StreamId};
use serde::{Deserialize, Serialize};

use crate::progress::Meter;
use crate::repo::Repo;
use crate::{
    EngineError, Failpoints, JournalEntry, ProcessProbe, State, fsx, grants, history_dir,
    new_version_id, now_ms, snapshot,
};

const KIND: &str = "restore";

/// Blob downloads in flight at once.
const PARALLEL_BLOBS: usize = 4;

/// What to restore: a version from the repo, or a local-history folder.
pub(crate) enum Source<'a> {
    Remote(&'a Version),
    Local(&'a Path),
}

pub(crate) struct Ctx<'a> {
    pub repo: Option<&'a Repo>,
    pub state: &'a State,
    pub data_dir: &'a Path,
    pub grants: &'a [PathBuf],
    pub probe: &'a dyn ProcessProbe,
    pub process_names: &'a [String],
    /// The profile's per-file limit (security §File-system confinement).
    pub max_file_bytes: u64,
    pub fp: &'a Failpoints,
    pub meter: &'a Meter,
}

/// The journal entry's data: everything `recover` needs without the engine.
#[derive(Serialize, Deserialize)]
struct SwapData {
    target: PathBuf,
    grant: PathBuf,
    backup: Option<PathBuf>,
    /// The stream's local history, for the pre-rollback backup.
    history: PathBuf,
    staging: PathBuf,
    process_names: Vec<String>,
}

fn too_large(rel: &str) -> EngineError {
    EngineError::Data(format!("file_too_large:{rel}"))
}

/// Step 1: every file into `staging`, verified (I6) and size-checked, before the target is touched.
async fn stage(
    ctx: &Ctx<'_>,
    source: &Source<'_>,
    staging: &Path,
) -> Result<Vec<String>, EngineError> {
    std::fs::create_dir_all(staging)?;
    match source {
        Source::Remote(v) => {
            let repo = ctx.repo.ok_or(EngineError::NotSetUp("repo"))?;
            if let Some(f) = v.files.iter().find(|f| f.size > ctx.max_file_bytes) {
                return Err(too_large(f.path.as_str()));
            }
            ctx.meter.begin(
                TransferPhase::Downloading,
                u32::try_from(v.files.len()).unwrap_or(u32::MAX),
                v.files.iter().map(|f| f.size).sum(),
            );
            stream::iter(&v.files)
                .map(|f| async move {
                    let rel = f.path.as_str();
                    // `get_blob` checks the AEAD tag and the keyed content hash.
                    let data = repo.get_blob(&f.blob).await?;
                    if u64::try_from(data.len()).ok() != Some(f.size) {
                        return Err(EngineError::Data(format!("wrong_size:{rel}")));
                    }
                    fsx::write_atomic(&staging.join(rel), &data)?;
                    Ok(f.size)
                })
                .buffer_unordered(PARALLEL_BLOBS)
                .try_for_each(|size| {
                    ctx.meter.advance(size);
                    async { Ok(()) }
                })
                .await?;
            Ok(v.files.iter().map(|f| f.path.as_str().to_owned()).collect())
        }
        Source::Local(dir) => {
            let mut files = Vec::new();
            for (rel, meta) in fsx::list_files(dir)? {
                RelPath::parse(&rel)
                    .map_err(|_| EngineError::Data(format!("file_name_invalid:{rel}")))?;
                if meta.len() > ctx.max_file_bytes {
                    return Err(too_large(&rel));
                }
                let bytes = std::fs::read(dir.join(&rel))?;
                if !u64::try_from(bytes.len()).is_ok_and(|n| n <= ctx.max_file_bytes) {
                    return Err(too_large(&rel));
                }
                fsx::write_atomic(&staging.join(&rel), &bytes)?;
                files.push(rel);
            }
            Ok(files)
        }
    }
}

/// fsyncs `dir` and its ancestors while they stay inside `stop` (unix; V-REL-6 on Windows).
fn sync_up(dir: &Path, stop: &Path) -> Result<(), EngineError> {
    for d in dir.ancestors().take_while(|d| d.starts_with(stop)) {
        fsx::sync_dir(d)?;
    }
    Ok(())
}

/// Copies every file of `src` into `dest` (inside `history`), consistently (`Busy` if a file
/// changes meanwhile) and durably: built in a temp sibling, every file and folder fsynced,
/// then renamed into place. A failure leaves nothing that looks like a backup.
fn backup_to(src: &Path, dest: &Path, history: &Path) -> Result<(), EngineError> {
    std::fs::create_dir_all(history)?;
    let tmp = fsx::temp_sibling(dest)?;
    let result = (|| {
        let listed = fsx::list_files(src)?;
        std::fs::create_dir_all(&tmp)?;
        for (rel, _) in &listed {
            fsx::write_atomic(&tmp.join(rel), &std::fs::read(src.join(rel))?)?;
        }
        if let Some(rel) = snapshot::first_change(&listed, &fsx::list_files(src)?) {
            return Err(EngineError::Busy(rel));
        }
        fsx::sync_tree(&tmp)?;
        fsx::rename_dir(&tmp, dest)?;
        // `history/<stream>`, `history` and the data folder may be new too.
        let stop = history.ancestors().nth(2).unwrap_or(history);
        sync_up(history, stop)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    result
}

/// Steps 1–3: stage, check I3, back up. Nothing here writes to the target.
async fn prepare(
    ctx: &Ctx<'_>,
    source: &Source<'_>,
    target: &Path,
    staging: &Path,
    history: &Path,
    reason: &str,
) -> Result<(Vec<String>, Option<PathBuf>), EngineError> {
    let files = stage(ctx, source, staging).await?;
    if ctx.probe.running(ctx.process_names) {
        return Err(EngineError::EmulatorRunning(ctx.process_names.join(", ")));
    }
    let backup = if target.is_dir() && !fsx::list_files(target)?.is_empty() {
        let dir = history.join(format!("{}-{reason}", now_ms()));
        backup_to(target, &dir, history)?;
        Some(dir)
    } else {
        None
    };
    ctx.fp.hit("restore.after_backup")?;
    Ok((files, backup))
}

/// Step 5: each file atomically (I4), then drop the files the version doesn't have (they are
/// in the backup), then make the folder durable.
fn swap(
    ctx: &Ctx<'_>,
    target: &Path,
    grant: &Path,
    staging: &Path,
    files: &[String],
) -> Result<(), EngineError> {
    std::fs::create_dir_all(target)?;
    for (i, rel) in files.iter().enumerate() {
        let dest = target.join(rel);
        fsx::ensure_no_symlinks(grant, &dest)?;
        fsx::write_atomic(&dest, &std::fs::read(staging.join(rel))?)?;
        if i == 0 {
            ctx.fp.hit("restore.mid_swap")?;
        }
    }
    let keep: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    for (rel, _) in fsx::list_files(target)? {
        if !keep.contains(rel.as_str()) {
            let path = target.join(&rel);
            fsx::ensure_no_symlinks(grant, &path)?;
            std::fs::remove_file(path)?;
        }
    }
    fsx::sync_tree(target)?;
    sync_up(target, grant)?;
    ctx.fp.hit("restore.before_done")
}

/// Restores `source` into `target` (a save folder inside a grant) and returns the backup of
/// what was there, or `None` when the target was empty. `reason` names the backup
/// (`<ms>-<reason>`) and must be a plain word (`a-z`, `0-9`, `-`).
///
/// Errors: `Usage("restore_reason_invalid:…")`, `NotGranted`, `EmulatorRunning`,
/// `Data("file_too_large:…" | "wrong_size:…" | "file_name_invalid:…")`, `Busy`, repo and I/O
/// errors. A failure before the swap leaves the target untouched; a failure during it is rolled
/// back at once, or by `recover` if the process dies.
pub(crate) async fn restore(
    ctx: &Ctx<'_>,
    stream: &StreamId,
    target: &Path,
    source: Source<'_>,
    reason: &str,
) -> Result<Option<PathBuf>, EngineError> {
    if reason.is_empty()
        || !reason
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(EngineError::Usage(format!(
            "restore_reason_invalid:{reason}"
        )));
    }
    // Confinement first: nothing is downloaded for a folder we may not write.
    let grant = grants::require(ctx.grants, target)?;
    fsx::ensure_no_symlinks(&grant, target)?;
    let op = new_version_id()?.to_string().to_lowercase();
    let staging = ctx.data_dir.join("staging").join(&op);
    let history = history_dir(ctx.data_dir, stream);
    let (files, backup) = match prepare(ctx, &source, target, &staging, &history, reason).await {
        Ok(p) => p,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    // Step 4: journal "swap started" (I5), durable before the first live write.
    let data = SwapData {
        target: target.to_path_buf(),
        grant: grant.clone(),
        backup: backup.clone(),
        history,
        staging: staging.clone(),
        process_names: ctx.process_names.to_vec(),
    };
    let entry = JournalEntry {
        op: op.clone(),
        stream: *stream,
        kind: KIND.into(),
        step: "swap".into(),
        data: serde_json::to_string(&data).map_err(|e| EngineError::Io(e.to_string()))?,
    };
    if let Err(e) = ctx.state.journal_put(&entry) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    match swap(ctx, target, &grant, &staging, &files) {
        Ok(()) => {}
        // A failpoint stands for a crash: the journal entry stays for `recover`.
        Err(e @ EngineError::Injected(_)) => return Err(e),
        Err(e) => {
            // A real error: roll back now rather than leave a half-new folder until the next
            // start. If that fails too, the entry stays and `recover` retries.
            if roll_back(&data).is_ok() {
                let _ = finish(ctx.state, &op, &staging);
            }
            return Err(e);
        }
    }
    // Step 6: done.
    finish(ctx.state, &op, &staging)?;
    Ok(backup)
}

fn finish(state: &State, op: &str, staging: &Path) -> Result<(), EngineError> {
    state.journal_delete(op)?;
    let _ = std::fs::remove_dir_all(staging);
    Ok(())
}

/// Same names, same bytes.
fn same_tree(a: &Path, a_names: &[String], b: &Path, b_names: &[String]) -> bool {
    a_names == b_names
        && a_names.iter().all(|rel| {
            matches!(
                (std::fs::read(a.join(rel)), std::fs::read(b.join(rel))),
                (Ok(x), Ok(y)) if x == y
            )
        })
}

/// Puts the target back exactly as the backup holds it. What it would overwrite or remove
/// goes to local history first (I1). Idempotent: a target that already matches is left alone.
fn roll_back(d: &SwapData) -> Result<(), EngineError> {
    fsx::ensure_no_symlinks(&d.grant, &d.target)?;
    let old: Vec<String> = match &d.backup {
        Some(b) => fsx::list_files(b)?.into_iter().map(|(n, _)| n).collect(),
        None => Vec::new(),
    };
    let current: Vec<String> = if d.target.is_dir() {
        fsx::list_files(&d.target)?
            .into_iter()
            .map(|(n, _)| n)
            .collect()
    } else {
        Vec::new()
    };
    let empty = PathBuf::new();
    if same_tree(
        &d.target,
        &current,
        d.backup.as_ref().unwrap_or(&empty),
        &old,
    ) {
        return Ok(());
    }
    if !current.is_empty() {
        let keep = d.history.join(format!("{}-pre-rollback", now_ms()));
        backup_to(&d.target, &keep, &d.history)?;
    }
    for rel in current.iter().filter(|rel| !old.contains(rel)) {
        let path = d.target.join(rel);
        fsx::ensure_no_symlinks(&d.grant, &path)?;
        std::fs::remove_file(path)?;
    }
    if let Some(b) = &d.backup {
        for rel in &old {
            let path = d.target.join(rel);
            fsx::ensure_no_symlinks(&d.grant, &path)?;
            let bytes = std::fs::read(b.join(rel))?;
            if std::fs::read(&path).ok().as_ref() != Some(&bytes) {
                fsx::write_atomic(&path, &bytes)?;
            }
        }
    }
    if d.target.is_dir() {
        fsx::sync_tree(&d.target)?;
    }
    Ok(())
}

/// Rolls every unfinished restore back to its backup (reliability §Restore procedure).
/// `EmulatorRunning` (entry kept) while that emulator runs.
pub(crate) fn recover(
    state: &State,
    probe: &dyn ProcessProbe,
) -> Result<Vec<Warning>, EngineError> {
    let mut warnings = Vec::new();
    for entry in state.journal_all()?.into_iter().filter(|e| e.kind == KIND) {
        let data: SwapData = serde_json::from_str(&entry.data)
            .map_err(|e| EngineError::Io(format!("journal entry {}: {e}", entry.op)))?;
        if probe.running(&data.process_names) {
            return Err(EngineError::EmulatorRunning(data.process_names.join(", ")));
        }
        roll_back(&data)?;
        finish(state, &entry.op, &data.staging)?;
        warnings.push(Warning {
            code: WarningCode::RestoreRolledBack,
            detail: data.target.display().to_string(),
        });
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FakeProbe, State};
    use lemusync_core::{DeviceId, Hash32, PlayerId, VersionId};

    struct World {
        _dir: tempfile::TempDir,
        root: PathBuf,
        repo: Repo,
        state: State,
        version: Version,
        target: PathBuf,
    }

    async fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let store = lemusync_store::Store::fs(&root.join("remote")).unwrap();
        let fast = lemusync_crypto::KdfParams {
            m_kib: 19_456,
            t: 1,
            p: 1,
        };
        let (repo, _, _) = Repo::create(store, "p", fast).await.unwrap();
        let state = State::open(&root.join("data")).unwrap();
        let new = root.join("new");
        std::fs::create_dir_all(new.join("0")).unwrap();
        std::fs::write(new.join("0/game_data.sav"), b"NEW GAME").unwrap();
        std::fs::write(new.join("option.sav"), b"NEW OPT").unwrap();
        let snap = crate::snapshot::take(&new, &root.join("hist"), 1 << 20, &|d| repo.blob_id(d))
            .unwrap()
            .unwrap();
        let meta = crate::upload::UploadMeta {
            stream: StreamId(Hash32([9; 32])),
            parents: vec![],
            device: DeviceId(VersionId::from_parts(1, [1; 10])),
            player: PlayerId(VersionId::from_parts(2, [2; 10])),
            emulator: "cemu",
            base_files: &[],
            note: None,
            takeover_from: None,
        };
        let version = crate::upload::upload(&repo, &snap, meta, &Failpoints::default(), &NO_METER)
            .await
            .unwrap();
        let target = root.join("grant/user/80000001");
        std::fs::create_dir_all(target.join("0")).unwrap();
        std::fs::write(target.join("0/game_data.sav"), b"OLD GAME").unwrap();
        std::fs::write(target.join("stale.sav"), b"OLD ONLY").unwrap();
        World {
            _dir: dir,
            root,
            repo,
            state,
            version,
            target,
        }
    }

    static NO_METER: std::sync::LazyLock<Meter> = std::sync::LazyLock::new(Meter::none);

    fn read_tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
        fsx::list_files(dir)
            .unwrap()
            .into_iter()
            .map(|(n, _)| (n.clone(), std::fs::read(dir.join(&n)).unwrap()))
            .collect()
    }

    fn ctx<'a>(
        w: &'a World,
        grants: &'a [PathBuf],
        probe: &'a FakeProbe,
        fp: &'a Failpoints,
        data: &'a Path,
    ) -> Ctx<'a> {
        Ctx {
            repo: Some(&w.repo),
            state: &w.state,
            data_dir: data,
            grants,
            probe,
            process_names: &[],
            max_file_bytes: 1 << 20,
            fp,
            meter: &NO_METER,
        }
    }

    fn history_names(w: &World) -> Vec<String> {
        let dir = history_dir(&w.root.join("data"), &w.version.stream);
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|d| {
                d.map(|e| e.unwrap().file_name().into_string().unwrap())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[tokio::test]
    async fn restore_backs_up_then_replaces_exactly() {
        let w = world().await;
        let grants = vec![w.root.join("grant").canonicalize().unwrap()];
        let (probe, fp, data) = (
            FakeProbe::default(),
            Failpoints::default(),
            w.root.join("data"),
        );
        let before = read_tree(&w.target);
        let backup = restore(
            &ctx(&w, &grants, &probe, &fp, &data),
            &w.version.stream,
            &w.target,
            Source::Remote(&w.version),
            "pre-restore",
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            read_tree(&backup),
            before,
            "I1: the old files are in local history"
        );
        assert_eq!(
            read_tree(&w.target),
            vec![
                ("0/game_data.sav".into(), b"NEW GAME".to_vec()),
                ("option.sav".into(), b"NEW OPT".to_vec())
            ]
        );
        assert_eq!(w.state.journal_all().unwrap(), Vec::<JournalEntry>::new());
        assert!(
            !data.join("staging").exists()
                || std::fs::read_dir(data.join("staging")).unwrap().count() == 0,
            "staging is cleaned up"
        );
    }

    #[tokio::test]
    async fn refuses_outside_grants_and_while_running() {
        let w = world().await;
        let (probe, fp, data) = (
            FakeProbe::default(),
            Failpoints::default(),
            w.root.join("data"),
        );
        let none: Vec<PathBuf> = vec![];
        let r = restore(
            &ctx(&w, &none, &probe, &fp, &data),
            &w.version.stream,
            &w.target,
            Source::Remote(&w.version),
            "x",
        )
        .await;
        assert!(matches!(r, Err(EngineError::NotGranted(_))));
        let grants = vec![w.root.join("grant").canonicalize().unwrap()];
        probe.set(true);
        let r = restore(
            &ctx(&w, &grants, &probe, &fp, &data),
            &w.version.stream,
            &w.target,
            Source::Remote(&w.version),
            "x",
        )
        .await;
        assert!(matches!(r, Err(EngineError::EmulatorRunning(_))));
        assert_eq!(
            std::fs::read(w.target.join("stale.sav")).unwrap(),
            b"OLD ONLY",
            "nothing touched"
        );
    }

    #[tokio::test]
    async fn a_crash_at_any_step_rolls_back_to_the_old_files() {
        for point in [
            "restore.after_backup",
            "restore.mid_swap",
            "restore.before_done",
        ] {
            let w = world().await;
            let grants = vec![w.root.join("grant").canonicalize().unwrap()];
            let (probe, fp, data) = (
                FakeProbe::default(),
                Failpoints::default(),
                w.root.join("data"),
            );
            let before = read_tree(&w.target);
            fp.arm(point);
            let r = restore(
                &ctx(&w, &grants, &probe, &fp, &data),
                &w.version.stream,
                &w.target,
                Source::Remote(&w.version),
                "x",
            )
            .await;
            assert!(matches!(r, Err(EngineError::Injected(_))), "{point}");
            recover(&w.state, &probe).unwrap();
            assert_eq!(
                read_tree(&w.target),
                before,
                "{point}: never half old, half new"
            );
            assert!(w.state.journal_all().unwrap().is_empty(), "{point}");
            recover(&w.state, &probe).unwrap(); // idempotent
        }
    }

    #[tokio::test]
    async fn rolling_back_keeps_the_files_it_replaces_in_history() {
        let w = world().await;
        let grants = vec![w.root.join("grant").canonicalize().unwrap()];
        let (probe, fp, data) = (
            FakeProbe::default(),
            Failpoints::default(),
            w.root.join("data"),
        );
        fp.arm("restore.before_done");
        let r = restore(
            &ctx(&w, &grants, &probe, &fp, &data),
            &w.version.stream,
            &w.target,
            Source::Remote(&w.version),
            "pre-restore",
        )
        .await;
        assert!(matches!(r, Err(EngineError::Injected(_))));
        let warnings = recover(&w.state, &probe).unwrap();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, WarningCode::RestoreRolledBack);
        let names = history_names(&w);
        let rollback = names
            .iter()
            .find(|n| n.ends_with("-pre-rollback"))
            .expect("I1 also holds for the rollback");
        let kept = read_tree(&history_dir(&data, &w.version.stream).join(rollback));
        assert!(kept.contains(&("0/game_data.sav".into(), b"NEW GAME".to_vec())));
        assert!(names.iter().any(|n| n.ends_with("-pre-restore")));
    }

    #[tokio::test]
    async fn recovery_waits_while_the_emulator_runs() {
        let w = world().await;
        let grants = vec![w.root.join("grant").canonicalize().unwrap()];
        let (probe, fp, data) = (
            FakeProbe::default(),
            Failpoints::default(),
            w.root.join("data"),
        );
        fp.arm("restore.mid_swap");
        let _ = restore(
            &ctx(&w, &grants, &probe, &fp, &data),
            &w.version.stream,
            &w.target,
            Source::Remote(&w.version),
            "x",
        )
        .await;
        probe.set(true);
        assert!(matches!(
            recover(&w.state, &probe),
            Err(EngineError::EmulatorRunning(_))
        ));
        assert_eq!(w.state.journal_all().unwrap().len(), 1, "entry kept");
        probe.set(false);
        recover(&w.state, &probe).unwrap();
        assert_eq!(
            std::fs::read(w.target.join("0/game_data.sav")).unwrap(),
            b"OLD GAME"
        );
    }

    #[tokio::test]
    async fn a_real_error_mid_swap_rolls_back_at_once() {
        let w = world().await;
        // A folder where the version has a file: the second swap step fails.
        std::fs::create_dir_all(w.target.join("option.sav")).unwrap();
        std::fs::write(w.target.join("option.sav/inner"), b"IN").unwrap();
        let before = read_tree(&w.target);
        let grants = vec![w.root.join("grant").canonicalize().unwrap()];
        let (probe, fp, data) = (
            FakeProbe::default(),
            Failpoints::default(),
            w.root.join("data"),
        );
        let r = restore(
            &ctx(&w, &grants, &probe, &fp, &data),
            &w.version.stream,
            &w.target,
            Source::Remote(&w.version),
            "x",
        )
        .await;
        assert!(matches!(r, Err(EngineError::Io(_))), "{r:?}");
        assert_eq!(read_tree(&w.target), before);
        assert_eq!(w.state.journal_all().unwrap(), Vec::<JournalEntry>::new());
    }

    #[tokio::test]
    async fn a_blob_over_the_size_limit_is_refused_before_any_write() {
        let w = world().await;
        let grants = vec![w.root.join("grant").canonicalize().unwrap()];
        let (probe, fp, data) = (
            FakeProbe::default(),
            Failpoints::default(),
            w.root.join("data"),
        );
        let before = read_tree(&w.target);
        let mut c = ctx(&w, &grants, &probe, &fp, &data);
        c.max_file_bytes = 4;
        let r = restore(
            &c,
            &w.version.stream,
            &w.target,
            Source::Remote(&w.version),
            "x",
        )
        .await;
        assert!(matches!(r, Err(EngineError::Data(_))), "{r:?}");
        assert_eq!(read_tree(&w.target), before);
        assert!(history_names(&w).is_empty(), "no backup either");
    }

    #[tokio::test]
    async fn restores_a_local_history_folder() {
        let w = world().await;
        let grants = vec![w.root.join("grant").canonicalize().unwrap()];
        let (probe, fp, data) = (
            FakeProbe::default(),
            Failpoints::default(),
            w.root.join("data"),
        );
        let old = w.root.join("hist");
        let backup = restore(
            &ctx(&w, &grants, &probe, &fp, &data),
            &w.version.stream,
            &w.target,
            Source::Local(&old),
            "pre-restore",
        )
        .await
        .unwrap();
        assert!(backup.is_some());
        assert_eq!(read_tree(&w.target), read_tree(&old));
    }

    #[tokio::test]
    async fn a_reason_that_is_not_a_plain_word_is_refused() {
        let w = world().await;
        let grants = vec![w.root.join("grant").canonicalize().unwrap()];
        let (probe, fp, data) = (
            FakeProbe::default(),
            Failpoints::default(),
            w.root.join("data"),
        );
        let r = restore(
            &ctx(&w, &grants, &probe, &fp, &data),
            &w.version.stream,
            &w.target,
            Source::Remote(&w.version),
            "../x",
        )
        .await;
        assert!(matches!(r, Err(EngineError::Usage(_))));
    }

    #[tokio::test]
    async fn opening_the_engine_rolls_back_an_unfinished_restore() {
        let w = world().await;
        let grants = vec![w.root.join("grant").canonicalize().unwrap()];
        let probe = FakeProbe::default();
        let fp = Failpoints::default();
        let env_root = w.root.join("env");
        let data = crate::Locations::under(&env_root).data_dir;
        let before = read_tree(&w.target);
        {
            let state = State::open(&data).unwrap();
            let c = Ctx {
                repo: Some(&w.repo),
                state: &state,
                data_dir: &data,
                grants: &grants,
                probe: &probe,
                process_names: &[],
                max_file_bytes: 1 << 20,
                fp: &fp,
                meter: &NO_METER,
            };
            fp.arm("restore.mid_swap");
            let r = restore(
                &c,
                &w.version.stream,
                &w.target,
                Source::Remote(&w.version),
                "x",
            )
            .await;
            assert!(matches!(r, Err(EngineError::Injected(_))));
        }
        let engine = crate::Engine::open(crate::tests::test_env(&env_root)).unwrap();
        assert_eq!(read_tree(&w.target), before);
        assert_eq!(
            engine.state.journal_all().unwrap(),
            Vec::<JournalEntry>::new()
        );
    }
}
