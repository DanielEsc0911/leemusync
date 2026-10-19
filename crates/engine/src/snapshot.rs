//! Consistent snapshots: read → hash → re-stat, copied into local history first
//! (sync-model §Snapshot rules, reliability §Upload procedure step 1).
//! Only reads the save folder; every write goes to local history (I1, I4).

use std::io::Read as _;
use std::path::{Path, PathBuf};

use leemusync_core::manifest::FileEntry;
use leemusync_core::{BlobId, RelPath};

use crate::{EngineError, fsx};

#[allow(dead_code)] // used by Task 15
pub(crate) enum Quick {
    Missing,
    SameAsBase,
    Changed,
}

#[allow(dead_code)] // used by Task 15
pub(crate) struct Snapshot {
    pub dir: PathBuf,
    pub files: Vec<FileEntry>,
}

/// Names, sizes and mtimes only; never reads file contents.
#[allow(dead_code)] // used by Task 15
pub(crate) fn quick(src: &Path, base_files: &[FileEntry]) -> Result<Quick, EngineError> {
    if !src.is_dir() {
        return Ok(Quick::Missing);
    }
    let files = fsx::list_files(src)?;
    if files.is_empty() {
        return Ok(Quick::Missing);
    }
    let same = files.len() == base_files.len()
        && files.iter().zip(base_files).all(|((name, meta), base)| {
            name == base.path.as_str()
                && meta.len() == base.size
                && fsx::mtime_ms(meta) == base.mtime
        });
    Ok(if same {
        Quick::SameAsBase
    } else {
        Quick::Changed
    })
}

/// Same paths and same blobs (mtimes and sizes don't matter).
#[allow(dead_code)] // used by Task 15
pub(crate) fn same_files(a: &[FileEntry], b: &[FileEntry]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.path == y.path && x.blob == y.blob)
}

/// Copies every file of `src` into `dest` and returns the entries, sorted by path.
/// `None` when `src` has no files.
///
/// Errors: `Busy(path)` if a file changed during the read; `Usage("file_too_large:<path>")`
/// or `Usage("file_name_invalid:<path>")`; `Io` for symlinks and I/O failures.
#[allow(dead_code)] // used by Task 15
pub(crate) fn take(
    src: &Path,
    dest: &Path,
    max_file_bytes: u64,
    blob_id: &dyn Fn(&[u8]) -> BlobId,
) -> Result<Option<Snapshot>, EngineError> {
    if !src.is_dir() {
        return Ok(None);
    }
    let listed = fsx::list_files(src)?;
    if listed.is_empty() {
        return Ok(None);
    }
    let mut files = Vec::with_capacity(listed.len());
    for (rel, before) in listed {
        let path = RelPath::parse(&rel)
            .map_err(|_| EngineError::Usage(format!("file_name_invalid:{rel}")))?;
        if before.len() > max_file_bytes {
            return Err(EngineError::Usage(format!("file_too_large:{rel}")));
        }
        let full = src.join(&rel);
        // Bounded read: a file that grows past the limit mid-read is caught as Busy below.
        let mut bytes = Vec::new();
        std::fs::File::open(&full)?
            .take(max_file_bytes.saturating_add(1))
            .read_to_end(&mut bytes)?;
        let after = std::fs::symlink_metadata(&full)?;
        if !after.is_file()
            || after.len() != before.len()
            || fsx::mtime_ms(&after) != fsx::mtime_ms(&before)
            || u64::try_from(bytes.len()).unwrap_or(u64::MAX) != before.len()
        {
            return Err(EngineError::Busy(rel));
        }
        fsx::write_atomic(&dest.join(&rel), &bytes)?;
        files.push(FileEntry {
            path,
            size: before.len(),
            mtime: fsx::mtime_ms(&before),
            blob: blob_id(&bytes),
        });
    }
    Ok(Some(Snapshot {
        dir: dest.to_path_buf(),
        files,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use leemusync_core::Hash32;

    fn fake_id(d: &[u8]) -> BlobId {
        let mut h = [0u8; 32];
        for (i, b) in d.iter().enumerate() {
            h[i % 32] ^= *b;
        }
        BlobId(Hash32(h))
    }

    #[test]
    fn takes_a_copy_and_lists_entries() {
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("0")).unwrap();
        std::fs::write(src.path().join("0/game_data.sav"), b"game").unwrap();
        std::fs::write(src.path().join("option.sav"), b"opt").unwrap();
        let hist = tempfile::tempdir().unwrap();
        let snap = take(src.path(), &hist.path().join("s1"), 1024, &fake_id)
            .unwrap()
            .unwrap();
        let paths: Vec<&str> = snap.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["0/game_data.sav", "option.sav"]);
        assert_eq!(
            std::fs::read(snap.dir.join("0/game_data.sav")).unwrap(),
            b"game"
        );
        assert_eq!(snap.files[1].size, 3);
        assert!(matches!(
            quick(src.path(), &snap.files).unwrap(),
            Quick::SameAsBase
        ));
        std::fs::write(src.path().join("option.sav"), b"changed").unwrap();
        assert!(matches!(
            quick(src.path(), &snap.files).unwrap(),
            Quick::Changed
        ));
    }

    #[test]
    fn empty_or_missing_folders_have_no_snapshot() {
        let src = tempfile::tempdir().unwrap();
        let hist = tempfile::tempdir().unwrap();
        assert!(
            take(src.path(), &hist.path().join("s"), 1024, &fake_id)
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            quick(&src.path().join("nope"), &[]).unwrap(),
            Quick::Missing
        ));
    }

    #[test]
    fn refuses_oversized_files() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("big.sav"), vec![0u8; 2048]).unwrap();
        let hist = tempfile::tempdir().unwrap();
        assert!(matches!(
            take(src.path(), &hist.path().join("s"), 1024, &fake_id),
            Err(EngineError::Usage(_))
        ));
    }

    #[test]
    fn same_files_compares_paths_and_blobs_only() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("a.sav"), b"a").unwrap();
        let hist = tempfile::tempdir().unwrap();
        let one = take(src.path(), &hist.path().join("1"), 1024, &fake_id)
            .unwrap()
            .unwrap();
        let mut two = one.files.clone();
        two[0].mtime += 5;
        assert!(same_files(&one.files, &two), "mtime alone is not a change");
        two[0].blob = fake_id(b"b");
        assert!(!same_files(&one.files, &two));
        assert!(!same_files(&one.files, &[]));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinks_and_leaves_the_save_folder_alone() {
        let src = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), b"x").unwrap();
        std::fs::write(src.path().join("a.sav"), b"a").unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret"), src.path().join("b.sav"))
            .unwrap();
        let hist = tempfile::tempdir().unwrap();
        assert!(take(src.path(), &hist.path().join("s"), 1024, &fake_id).is_err());
        assert!(
            !hist.path().join("s/b.sav").exists(),
            "nothing behind a link is copied"
        );
        assert_eq!(std::fs::read(src.path().join("a.sav")).unwrap(), b"a");
    }
}
