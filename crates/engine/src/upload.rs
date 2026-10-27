//! Upload procedure: blobs (create-only, idempotent) → manifest (commit point)
//! (reliability §Upload procedure).

use std::collections::BTreeSet;

use futures_util::{StreamExt, TryStreamExt, stream};
use lemusync_api::TransferPhase;
use lemusync_core::manifest::{EmulatorRef, FORMAT, FileEntry, Meta, Version};
use lemusync_core::{DeviceId, PlayerId, StreamId, VersionId};

use crate::progress::Meter;
use crate::repo::Repo;
use crate::snapshot::Snapshot;
use crate::{EngineError, Failpoints, new_version_id, now_ms};

pub(crate) struct UploadMeta<'a> {
    pub stream: StreamId,
    pub parents: Vec<VersionId>,
    pub device: DeviceId,
    pub player: PlayerId,
    pub emulator: &'a str,
    pub base_files: &'a [FileEntry],
    pub note: Option<String>,
    /// The device whose stale lease this session took over.
    pub takeover_from: Option<DeviceId>,
}

/// Blob uploads in flight at once.
const PARALLEL_BLOBS: usize = 4;

/// Reads a history file and checks it still matches the snapshot (I6).
fn read_checked(repo: &Repo, snap: &Snapshot, f: &FileEntry) -> Result<Vec<u8>, EngineError> {
    let data = std::fs::read(snap.dir.join(f.path.as_str()))?;
    if u64::try_from(data.len()).unwrap_or(u64::MAX) != f.size || repo.blob_id(&data) != f.blob {
        return Err(EngineError::Data(format!(
            "history_changed:{}",
            f.path.as_str()
        )));
    }
    Ok(data)
}

/// Uploads the blobs the base version doesn't already reference (its manifest was committed
/// after them, so they're on the remote), once per blob id and 4 at a time, then writes the
/// manifest. Every history file is first checked against the snapshot's size and blob id (I6).
/// A failure before the manifest leaves only orphaned blobs, which GC collects; retrying is safe.
///
/// Errors: `Data("history_changed:<path>")` if the history copy no longer matches the snapshot;
/// `Data("blob_corrupt:<id>")` if a blob already on the remote is damaged;
/// `Data("manifest_invalid")` if the manifest would break a validation rule.
pub(crate) async fn upload(
    repo: &Repo,
    snap: &Snapshot,
    meta: UploadMeta<'_>,
    fp: &Failpoints,
    meter: &Meter,
) -> Result<Version, EngineError> {
    for f in &snap.files {
        read_checked(repo, snap, f)?;
    }
    let mut seen: BTreeSet<_> = meta.base_files.iter().map(|f| f.blob).collect();
    let new: Vec<&FileEntry> = snap.files.iter().filter(|f| seen.insert(f.blob)).collect();
    meter.begin(
        TransferPhase::Uploading,
        u32::try_from(new.len()).unwrap_or(u32::MAX),
        new.iter().map(|f| f.size).sum(),
    );
    stream::iter(new)
        .map(|f| async move {
            repo.put_blob(&read_checked(repo, snap, f)?).await?;
            Ok::<_, EngineError>(f.size)
        })
        .buffer_unordered(PARALLEL_BLOBS)
        .try_for_each(|size| {
            meter.advance(size);
            async { Ok(()) }
        })
        .await?;
    fp.hit("upload.before_manifest")?;
    commit_manifest(repo, meta, snap.files.clone(), fp).await
}

/// Writes the manifest for `files`, whose blobs are already on the remote (create-only: the
/// commit point). `changed_files` counts entries that differ from `meta.base_files`.
///
/// Errors: `Data("manifest_invalid")` if the manifest would break a validation rule.
pub(crate) async fn commit_manifest(
    repo: &Repo,
    meta: UploadMeta<'_>,
    files: Vec<FileEntry>,
    fp: &Failpoints,
) -> Result<Version, EngineError> {
    let changed = files
        .iter()
        .filter(|f| {
            !meta
                .base_files
                .iter()
                .any(|b| b.path == f.path && b.blob == f.blob)
        })
        .count();
    let v = Version {
        format: FORMAT,
        stream: meta.stream,
        id: new_version_id()?,
        parents: meta.parents,
        device: meta.device,
        player: meta.player,
        created_at: now_ms(),
        emulator: EmulatorRef {
            id: meta.emulator.to_owned(),
            version: None,
        },
        files,
        meta: Meta {
            session_secs: None,
            changed_files: u32::try_from(changed).unwrap_or(u32::MAX),
            takeover: meta.takeover_from.is_some(),
            takeover_from: meta.takeover_from,
            note: meta.note,
        },
    };
    // Never commit a manifest that readers would reject.
    v.validate(&v.stream, &v.id)
        .map_err(|_| EngineError::Data("manifest_invalid".into()))?;
    repo.put_version(&v).await?;
    fp.hit("upload.after_manifest")?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::Repo;
    use lemusync_core::{Hash32, VersionId};

    #[tokio::test]
    async fn blobs_first_manifest_last() {
        let dir = tempfile::tempdir().unwrap();
        let store = lemusync_store::Store::fs(&dir.path().join("remote")).unwrap();
        let fast = lemusync_crypto::KdfParams {
            m_kib: 19_456,
            t: 1,
            p: 1,
        };
        let (repo, _, _) = Repo::create(store, "p", fast).await.unwrap();
        let src = dir.path().join("save");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("option.sav"), b"opt").unwrap();
        let snap =
            crate::snapshot::take(&src, &dir.path().join("hist"), 1024, &|d| repo.blob_id(d))
                .unwrap()
                .unwrap();
        let stream = StreamId(Hash32([3; 32]));
        let meta = || UploadMeta {
            stream,
            parents: vec![],
            device: DeviceId(VersionId::from_parts(1, [1; 10])),
            player: PlayerId(VersionId::from_parts(2, [2; 10])),
            emulator: "cemu",
            base_files: &[],
            note: None,
            takeover_from: None,
        };
        let fp = Failpoints::default();
        fp.arm("upload.before_manifest");
        assert!(matches!(
            upload(&repo, &snap, meta(), &fp, &Meter::none()).await,
            Err(EngineError::Injected(_))
        ));
        assert!(
            repo.list_versions(&stream).await.unwrap().is_empty(),
            "no manifest, no version"
        );
        assert_eq!(
            repo.get_blob(&snap.files[0].blob).await.unwrap(),
            b"opt",
            "the blob already went up"
        );
        let v = upload(&repo, &snap, meta(), &fp, &Meter::none())
            .await
            .unwrap();
        assert_eq!(repo.list_versions(&stream).await.unwrap(), vec![v.id]);
        assert_eq!(v.meta.changed_files, 1);
    }

    #[tokio::test]
    async fn twelve_files_land_twelve_blobs_and_one_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let store = lemusync_store::Store::fs(&dir.path().join("remote")).unwrap();
        let fast = lemusync_crypto::KdfParams {
            m_kib: 19_456,
            t: 1,
            p: 1,
        };
        let (repo, _, _) = Repo::create(store, "p", fast).await.unwrap();
        let src = dir.path().join("save");
        std::fs::create_dir_all(&src).unwrap();
        for i in 0..12 {
            std::fs::write(src.join(format!("slot{i}.sav")), format!("save {i}")).unwrap();
        }
        let snap =
            crate::snapshot::take(&src, &dir.path().join("hist"), 1024, &|d| repo.blob_id(d))
                .unwrap()
                .unwrap();
        assert_eq!(snap.files.len(), 12);
        let stream = StreamId(Hash32([6; 32]));
        let meta = UploadMeta {
            stream,
            parents: vec![],
            device: DeviceId(VersionId::from_parts(1, [1; 10])),
            player: PlayerId(VersionId::from_parts(2, [2; 10])),
            emulator: "cemu",
            base_files: &[],
            note: None,
            takeover_from: None,
        };
        let v = upload(&repo, &snap, meta, &Failpoints::default(), &Meter::none())
            .await
            .unwrap();
        assert_eq!(repo.list_versions(&stream).await.unwrap(), vec![v.id]);
        for f in &snap.files {
            let i = f
                .path
                .as_str()
                .trim_start_matches("slot")
                .trim_end_matches(".sav");
            assert_eq!(
                repo.get_blob(&f.blob).await.unwrap(),
                format!("save {i}").as_bytes()
            );
        }
    }

    fn meta_on(base: &[FileEntry]) -> UploadMeta<'_> {
        UploadMeta {
            stream: StreamId(Hash32([7; 32])),
            parents: vec![],
            device: DeviceId(VersionId::from_parts(1, [1; 10])),
            player: PlayerId(VersionId::from_parts(2, [2; 10])),
            emulator: "cemu",
            base_files: base,
            note: None,
            takeover_from: None,
        }
    }

    fn blob_count(remote: &std::path::Path) -> usize {
        fn walk(d: &std::path::Path) -> usize {
            std::fs::read_dir(d).map_or(0, |rd| {
                rd.flatten()
                    .map(|e| {
                        if e.path().is_dir() {
                            walk(&e.path())
                        } else {
                            1
                        }
                    })
                    .sum()
            })
        }
        walk(&remote.join("lemusync/blobs"))
    }

    #[tokio::test]
    async fn only_blobs_missing_from_the_base_go_up_once_each() {
        use std::sync::{Arc, Mutex};
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("remote");
        let store = lemusync_store::Store::fs(&remote).unwrap();
        let fast = lemusync_crypto::KdfParams {
            m_kib: 19_456,
            t: 1,
            p: 1,
        };
        let (repo, _, _) = Repo::create(store, "p", fast).await.unwrap();
        let src = dir.path().join("save");
        std::fs::create_dir_all(&src).unwrap();
        for i in 0..10 {
            std::fs::write(src.join(format!("slot{i}.sav")), format!("save {i}")).unwrap();
        }
        let take = |n: &str| {
            crate::snapshot::take(&src, &dir.path().join(n), 1024, &|d| repo.blob_id(d))
                .unwrap()
                .unwrap()
        };
        let first = take("h1");
        let v1 = upload(
            &repo,
            &first,
            meta_on(&[]),
            &Failpoints::default(),
            &Meter::none(),
        )
        .await
        .unwrap();
        assert_eq!(blob_count(&remote), 10, "a first upload sends every blob");

        // Two new files with the same content, and one changed file.
        std::fs::write(src.join("new_a.sav"), b"same").unwrap();
        std::fs::write(src.join("new_b.sav"), b"same").unwrap();
        std::fs::write(src.join("slot0.sav"), b"changed").unwrap();
        let second = take("h2");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let sink: crate::ProgressSink = Arc::new(move |p| s2.lock().unwrap().push(p.clone()));
        let meter = Meter::new(Some(sink), "Ana", "cemu", "g");
        upload(
            &repo,
            &second,
            meta_on(&v1.files),
            &Failpoints::default(),
            &meter,
        )
        .await
        .unwrap();
        assert_eq!(blob_count(&remote), 12, "only `same` and `changed` went up");
        assert_eq!(meter.transferred(), (2, 11));
        let last = seen.lock().unwrap().last().unwrap().clone();
        assert_eq!(last.phase, lemusync_api::TransferPhase::Uploading);
        assert_eq!((last.files_done, last.files_total), (2, 2));
        assert_eq!((last.bytes_done, last.bytes_total), (11, 11));
    }

    #[tokio::test]
    async fn a_changed_history_copy_uploads_no_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let store = lemusync_store::Store::fs(&dir.path().join("remote")).unwrap();
        let fast = lemusync_crypto::KdfParams {
            m_kib: 19_456,
            t: 1,
            p: 1,
        };
        let (repo, _, _) = Repo::create(store, "p", fast).await.unwrap();
        let src = dir.path().join("save");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("option.sav"), b"opt").unwrap();
        let snap =
            crate::snapshot::take(&src, &dir.path().join("hist"), 1024, &|d| repo.blob_id(d))
                .unwrap()
                .unwrap();
        std::fs::write(snap.dir.join("option.sav"), b"bad").unwrap();
        let stream = StreamId(Hash32([4; 32]));
        let meta = UploadMeta {
            stream,
            parents: vec![],
            device: DeviceId(VersionId::from_parts(1, [1; 10])),
            player: PlayerId(VersionId::from_parts(2, [2; 10])),
            emulator: "cemu",
            base_files: &snap.files,
            note: None,
            takeover_from: None,
        };
        let result = upload(&repo, &snap, meta, &Failpoints::default(), &Meter::none()).await;
        assert!(matches!(result, Err(EngineError::Data(_))));
        assert!(
            repo.list_versions(&stream).await.unwrap().is_empty(),
            "no manifest points at a bad blob"
        );
    }
    #[tokio::test]
    async fn a_corrupt_existing_blob_blocks_the_upload() {
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("remote");
        let store = lemusync_store::Store::fs(&remote).unwrap();
        let fast = lemusync_crypto::KdfParams {
            m_kib: 19_456,
            t: 1,
            p: 1,
        };
        let (repo, _, _) = Repo::create(store, "p", fast).await.unwrap();
        let src = dir.path().join("save");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("option.sav"), b"opt").unwrap();
        let snap =
            crate::snapshot::take(&src, &dir.path().join("hist"), 1024, &|d| repo.blob_id(d))
                .unwrap()
                .unwrap();
        let raw = lemusync_store::Store::fs(&remote).unwrap();
        let path = crate::repo::blob_path(&snap.files[0].blob);
        raw.put(&path, b"torn".to_vec(), lemusync_store::PutMode::CreateOnly)
            .await
            .unwrap();
        let stream = StreamId(Hash32([5; 32]));
        let meta = UploadMeta {
            stream,
            parents: vec![],
            device: DeviceId(VersionId::from_parts(1, [1; 10])),
            player: PlayerId(VersionId::from_parts(2, [2; 10])),
            emulator: "cemu",
            base_files: &[],
            note: None,
            takeover_from: None,
        };
        let result = upload(&repo, &snap, meta, &Failpoints::default(), &Meter::none()).await;
        let expected = format!("blob_corrupt:{}", snap.files[0].blob);
        assert!(matches!(result, Err(EngineError::Data(d)) if d == expected));
        assert!(
            repo.list_versions(&stream).await.unwrap().is_empty(),
            "no manifest points at a corrupt blob"
        );
        assert_eq!(
            raw.get(&path).await.unwrap(),
            b"torn",
            "never overwritten (I2)"
        );
    }
}
