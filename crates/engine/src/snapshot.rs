//! Consistent snapshots: read → hash → re-stat, copied into local history first
//! (sync-model §Snapshot rules, reliability §Upload procedure step 1).
//! Only reads the save folder; every write goes to local history (I1, I4).

use std::fs::Metadata;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use lemusync_core::manifest::FileEntry;
use lemusync_core::{BlobId, RelPath};

use crate::{EngineError, fsx};

pub(crate) enum Quick {
    Missing,
    SameAsBase,
    Changed,
}

pub(crate) struct Snapshot {
    pub dir: PathBuf,
    pub files: Vec<FileEntry>,
}

/// Names, sizes and mtimes only; never reads file contents.
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
pub(crate) fn same_files(a: &[FileEntry], b: &[FileEntry]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.path == y.path && x.blob == y.blob)
}

/// Copies every file of `src` into `dest` and returns the entries, sorted by path.
/// `None` when `src` has no files. The copy is built in a temp sibling of `dest` and renamed
/// into place only when complete, so a failed snapshot leaves nothing that looks like one.
///
/// Errors: `Busy(path)` if a file changed, appeared or vanished during the read;
/// `Usage("file_too_large:<path>")` or `Usage("file_name_invalid:<path>")`; `Io` for symlinks
/// and I/O failures.
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
    take_listed(src, dest, &listed, max_file_bytes, blob_id).map(Some)
}

/// `take` after the listing: reads every listed file, then re-lists `src` and requires the
/// same paths, sizes and mtimes, so the set is never a mix of old and new files.
fn take_listed(
    src: &Path,
    dest: &Path,
    listed: &[(String, Metadata)],
    max_file_bytes: u64,
    blob_id: &dyn Fn(&[u8]) -> BlobId,
) -> Result<Snapshot, EngineError> {
    let tmp = fsx::temp_sibling(dest)?;
    let result = copy_listed(src, &tmp, listed, max_file_bytes, blob_id).and_then(|files| {
        fsx::rename_dir(&tmp, dest)?;
        Ok(Snapshot {
            dir: dest.to_path_buf(),
            files,
        })
    });
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    result
}

fn copy_listed(
    src: &Path,
    tmp: &Path,
    listed: &[(String, Metadata)],
    max_file_bytes: u64,
    blob_id: &dyn Fn(&[u8]) -> BlobId,
) -> Result<Vec<FileEntry>, EngineError> {
    std::fs::create_dir_all(tmp)?;
    let mut files = Vec::with_capacity(listed.len());
    for (rel, before) in listed {
        let path = RelPath::parse(rel)
            .map_err(|_| EngineError::Usage(format!("file_name_invalid:{rel}")))?;
        if before.len() > max_file_bytes {
            return Err(EngineError::Usage(format!("file_too_large:{rel}")));
        }
        let bytes = read_consistent(&src.join(rel), rel, before, max_file_bytes)?;
        fsx::write_atomic(&tmp.join(rel), &bytes)?;
        files.push(FileEntry {
            path,
            size: before.len(),
            mtime: fsx::mtime_ms(before),
            blob: blob_id(&bytes),
        });
    }
    if let Some(rel) = first_change(listed, &fsx::list_files(src)?) {
        return Err(EngineError::Busy(rel));
    }
    Ok(files)
}

/// The first path that was added, removed or changed (size or mtime) between two sorted listings.
pub(crate) fn first_change(
    before: &[(String, Metadata)],
    after: &[(String, Metadata)],
) -> Option<String> {
    for i in 0..before.len().max(after.len()) {
        match (before.get(i), after.get(i)) {
            (Some((a, am)), Some((b, bm))) if a == b => {
                if am.len() != bm.len() || fsx::mtime_ms(am) != fsx::mtime_ms(bm) {
                    return Some(a.clone());
                }
            }
            (Some((a, _)), Some((b, _))) => return Some(a.min(b).clone()),
            (Some((a, _)), None) | (None, Some((a, _))) => return Some(a.clone()),
            (None, None) => break,
        }
    }
    None
}

/// Reads one file (at most `max + 1` bytes) and re-stats it. `Busy(rel)` if it vanished, is no
/// longer a regular file, or its size or mtime differs from `before`.
fn read_consistent(
    full: &Path,
    rel: &str,
    before: &Metadata,
    max: u64,
) -> Result<Vec<u8>, EngineError> {
    let busy_if_gone = |e: std::io::Error| {
        if e.kind() == std::io::ErrorKind::NotFound {
            EngineError::Busy(rel.to_owned())
        } else {
            e.into()
        }
    };
    let mut bytes = Vec::new();
    std::fs::File::open(full)
        .map_err(busy_if_gone)?
        .take(max.saturating_add(1))
        .read_to_end(&mut bytes)?;
    let after = std::fs::symlink_metadata(full).map_err(busy_if_gone)?;
    if !after.is_file()
        || after.len() != before.len()
        || fsx::mtime_ms(&after) != fsx::mtime_ms(before)
        || u64::try_from(bytes.len()).unwrap_or(u64::MAX) != before.len()
    {
        return Err(EngineError::Busy(rel.to_owned()));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lemusync_core::Hash32;

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
    #[test]
    fn a_file_added_after_the_listing_is_busy() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("a.sav"), b"a").unwrap();
        let listed = fsx::list_files(src.path()).unwrap();
        std::fs::write(src.path().join("b.sav"), b"b").unwrap();
        let hist = tempfile::tempdir().unwrap();
        let dest = hist.path().join("s");
        let r = take_listed(src.path(), &dest, &listed, 1024, &fake_id);
        assert!(matches!(r, Err(EngineError::Busy(_))));
        assert!(!dest.exists(), "no partial snapshot");
    }

    #[test]
    fn a_file_removed_after_the_listing_is_busy() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("a.sav"), b"a").unwrap();
        std::fs::write(src.path().join("b.sav"), b"b").unwrap();
        let listed = fsx::list_files(src.path()).unwrap();
        std::fs::remove_file(src.path().join("b.sav")).unwrap();
        let hist = tempfile::tempdir().unwrap();
        let dest = hist.path().join("s");
        let r = take_listed(src.path(), &dest, &listed, 1024, &fake_id);
        assert!(matches!(r, Err(EngineError::Busy(p)) if p == "b.sav"));
        assert!(!dest.exists(), "no partial snapshot");
    }

    #[test]
    fn read_consistent_catches_rewrites_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.sav");
        std::fs::write(&f, b"ab").unwrap();
        let before = std::fs::symlink_metadata(&f).unwrap();
        assert_eq!(read_consistent(&f, "a.sav", &before, 1024).unwrap(), b"ab");
        std::fs::write(&f, b"abcd").unwrap();
        assert!(matches!(
            read_consistent(&f, "a.sav", &before, 1024),
            Err(EngineError::Busy(_))
        ));
        std::fs::remove_file(&f).unwrap();
        assert!(matches!(
            read_consistent(&f, "a.sav", &before, 1024),
            Err(EngineError::Busy(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn read_consistent_refuses_a_file_swapped_for_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.sav");
        std::fs::write(&f, b"ab").unwrap();
        let before = std::fs::symlink_metadata(&f).unwrap();
        let other = dir.path().join("other");
        std::fs::write(&other, b"ab").unwrap();
        std::fs::remove_file(&f).unwrap();
        std::os::unix::fs::symlink(&other, &f).unwrap();
        assert!(matches!(
            read_consistent(&f, "a.sav", &before, 1024),
            Err(EngineError::Busy(_))
        ));
    }

    #[test]
    fn a_failed_take_leaves_nothing_in_history() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("a.sav"), b"a").unwrap();
        std::fs::write(src.path().join("big.sav"), vec![0u8; 2048]).unwrap();
        let hist = tempfile::tempdir().unwrap();
        let r = take(src.path(), &hist.path().join("s"), 1024, &fake_id);
        assert!(matches!(r, Err(EngineError::Usage(_))));
        assert_eq!(
            std::fs::read_dir(hist.path()).unwrap().count(),
            0,
            "no dest, no temp dir"
        );
    }
}
