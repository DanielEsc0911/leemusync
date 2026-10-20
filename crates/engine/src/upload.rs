//! Upload procedure: blobs (create-only, idempotent) → manifest (commit point)
//! (reliability §Upload procedure).

use leemusync_core::manifest::{EmulatorRef, FORMAT, FileEntry, Meta, Version};
use leemusync_core::{DeviceId, PlayerId, StreamId, VersionId};

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
}

/// Uploads every blob from the local history copy (verified against the snapshot's size and
/// blob id first, I6), then writes the manifest. A failure before the manifest leaves only
/// orphaned blobs, which GC collects; retrying is safe.
///
/// Errors: `Data("history_changed:<path>")` if the history copy no longer matches the snapshot;
/// `Data("blob_corrupt:<id>")` if a blob already on the remote is damaged;
/// `Data("manifest_invalid")` if the manifest would break a validation rule.
pub(crate) async fn upload(
    repo: &Repo,
    snap: &Snapshot,
    meta: UploadMeta<'_>,
    fp: &Failpoints,
) -> Result<Version, EngineError> {
    for f in &snap.files {
        let data = std::fs::read(snap.dir.join(f.path.as_str()))?;
        if u64::try_from(data.len()).unwrap_or(u64::MAX) != f.size || repo.blob_id(&data) != f.blob
        {
            return Err(EngineError::Data(format!(
                "history_changed:{}",
                f.path.as_str()
            )));
        }
        repo.put_blob(&data).await?;
    }
    fp.hit("upload.before_manifest")?;
    let changed = snap
        .files
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
        files: snap.files.clone(),
        meta: Meta {
            session_secs: None,
            changed_files: u32::try_from(changed).unwrap_or(u32::MAX),
            takeover: false,
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
    use leemusync_core::{Hash32, VersionId};

    #[tokio::test]
    async fn blobs_first_manifest_last() {
        let dir = tempfile::tempdir().unwrap();
        let store = leemusync_store::Store::fs(&dir.path().join("remote")).unwrap();
        let fast = leemusync_crypto::KdfParams {
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
        };
        let fp = Failpoints::default();
        fp.arm("upload.before_manifest");
        assert!(matches!(
            upload(&repo, &snap, meta(), &fp).await,
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
        let v = upload(&repo, &snap, meta(), &fp).await.unwrap();
        assert_eq!(repo.list_versions(&stream).await.unwrap(), vec![v.id]);
        assert_eq!(v.meta.changed_files, 1);
    }

    #[tokio::test]
    async fn a_changed_history_copy_uploads_no_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let store = leemusync_store::Store::fs(&dir.path().join("remote")).unwrap();
        let fast = leemusync_crypto::KdfParams {
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
        };
        let result = upload(&repo, &snap, meta, &Failpoints::default()).await;
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
        let store = leemusync_store::Store::fs(&remote).unwrap();
        let fast = leemusync_crypto::KdfParams {
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
        let raw = leemusync_store::Store::fs(&remote).unwrap();
        let path = crate::repo::blob_path(&snap.files[0].blob);
        raw.put(
            &path,
            b"torn".to_vec(),
            leemusync_store::PutMode::CreateOnly,
        )
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
        };
        let result = upload(&repo, &snap, meta, &Failpoints::default()).await;
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
