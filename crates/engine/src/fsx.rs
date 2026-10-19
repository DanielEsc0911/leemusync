//! File helpers that keep invariant I4 (atomic writes) and never follow symlinks.

use std::fmt::Write as _;
use std::fs::{self, Metadata, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

const TEMP_PREFIX: &str = ".leemusync-";
const TEMP_SUFFIX: &str = ".tmp";

/// `write_atomic`'s temp names. A crash can orphan one; it is never snapshotted or restored.
fn is_temp_name(name: &str) -> bool {
    name.len() > TEMP_PREFIX.len() + TEMP_SUFFIX.len()
        && name.starts_with(TEMP_PREFIX)
        && name.ends_with(TEMP_SUFFIX)
}

/// Temp file in the same folder (`create_new`, so an existing name or symlink is never opened),
/// write, fsync, rename over the target (replaces the name, never follows it), fsync the folder (unix).
///
/// # Errors
/// Any I/O error; the target is then unchanged.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("path has no parent"))?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(temp_name()?);
    let result = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        sync_dir(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// A fresh random temp name (`.leemusync-<hex>.tmp`) that `list_files` skips.
fn temp_name() -> io::Result<String> {
    let random = leemusync_crypto::random_bytes::<8>().map_err(io::Error::other)?;
    let name = random.iter().fold(String::new(), |mut acc, b| {
        let _ = write!(acc, "{b:02x}");
        acc
    });
    Ok(format!("{TEMP_PREFIX}{name}{TEMP_SUFFIX}"))
}

/// A not-yet-created temp path next to `path` (same folder, so a rename stays on one file system).
///
/// # Errors
/// `path` has no parent, or the OS random source fails.
pub fn temp_sibling(path: &Path) -> io::Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("path has no parent"))?;
    Ok(parent.join(temp_name()?))
}

/// Renames a finished folder into place, then fsyncs the parent (unix) so the rename is durable.
///
/// # Errors
/// Any I/O error; on unix a non-empty `to` is never replaced.
pub fn rename_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)?;
    match to.parent() {
        Some(parent) => sync_dir(parent),
        None => Ok(()),
    }
}

/// Makes the rename durable. Not done on Windows; see reliability V-REL-6.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> io::Result<()> {
    fs::File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)] // same signature as the unix version
fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

/// Fails if `path` is outside `root` or any existing component between them is a symlink.
///
/// The root is matched against `path`'s ancestors (shortest first) lexically or by canonical
/// form, so `/var` → `/private/var` (macOS) and `\\?\` prefixes (Windows) aren't false
/// rejections. Below the matched ancestor every component is checked with `symlink_metadata`,
/// never followed.
///
/// # Errors
/// `PermissionDenied` naming the problem.
pub fn ensure_no_symlinks(root: &Path, path: &Path) -> io::Result<()> {
    let base = matching_ancestor(root, path).ok_or_else(|| {
        io::Error::new(io::ErrorKind::PermissionDenied, "path is outside its root")
    })?;
    let rel = path
        .strip_prefix(base)
        .map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied, "path is outside its root"))?;
    let mut current = base.to_path_buf();
    for c in rel.components() {
        let Component::Normal(part) = c else {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "path has `..` or a prefix",
            ));
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "symlink inside a save folder",
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// The shortest ancestor of `path` (itself included) that is `root`, lexically or canonically.
fn matching_ancestor<'a>(root: &Path, path: &'a Path) -> Option<&'a Path> {
    let mut ancestors: Vec<&Path> = path.ancestors().collect();
    ancestors.reverse();
    if let Some(found) = ancestors.iter().find(|a| **a == root) {
        return Some(found);
    }
    let canonical_root = fs::canonicalize(root).ok()?;
    ancestors
        .into_iter()
        .find(|a| fs::canonicalize(a).is_ok_and(|c| c == canonical_root))
}

/// Every regular file under `dir`, as `/`-separated relative paths, sorted. Refuses symlinks.
/// Skips (never deletes) `write_atomic` temp files.
///
/// # Errors
/// I/O errors, or `PermissionDenied` for a symlink or a non-UTF-8 name.
pub fn list_files(dir: &Path) -> io::Result<Vec<(String, Metadata)>> {
    let mut out = Vec::new();
    walk(dir, "", &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, Metadata)>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let meta = fs::symlink_metadata(entry.path())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied, "non-UTF-8 file name"))?;
        if is_temp_name(&name) {
            continue;
        }
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if meta.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("symlink at {rel}"),
            ));
        } else if meta.is_dir() {
            walk(&entry.path(), &rel, out)?;
        } else if meta.is_file() {
            out.push((rel, meta));
        }
    }
    Ok(())
}

/// Copies every file under `src` into `dst` (created), each written atomically.
///
/// # Errors
/// Any I/O error from listing, reading or writing.
pub fn copy_tree(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for (rel, _) in list_files(src)? {
        write_atomic(&dst.join(&rel), &fs::read(src.join(&rel))?)?;
    }
    Ok(())
}

#[must_use]
pub fn mtime_ms(meta: &Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_replaces_and_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.sav");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinks_inside_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        assert!(ensure_no_symlinks(dir.path(), &dir.path().join("link/x.sav")).is_err());
        assert!(ensure_no_symlinks(dir.path(), &dir.path().join("real/x.sav")).is_ok());
        assert!(
            ensure_no_symlinks(dir.path(), outside.path()).is_err(),
            "outside the root"
        );
        std::fs::write(dir.path().join("f"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.path().join("f"), dir.path().join("g")).unwrap();
        assert!(
            list_files(dir.path()).is_err(),
            "listing refuses symlinks too"
        );
    }

    #[test]
    fn copy_tree_and_list_files_are_recursive_and_sorted() {
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("0")).unwrap();
        std::fs::write(src.path().join("0/game_data.sav"), b"g").unwrap();
        std::fs::write(src.path().join("option.sav"), b"o").unwrap();
        let dst = tempfile::tempdir().unwrap();
        copy_tree(src.path(), &dst.path().join("copy")).unwrap();
        let names: Vec<String> = list_files(&dst.path().join("copy"))
            .unwrap()
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(names, ["0/game_data.sav", "option.sav"]);
    }

    #[test]
    fn a_canonical_path_matches_a_non_canonical_root() {
        // macOS tempdirs live under /var → /private/var; Windows canonical paths gain `\\?\`.
        let dir = tempfile::tempdir().unwrap();
        let canonical = std::fs::canonicalize(dir.path()).unwrap();
        assert!(ensure_no_symlinks(dir.path(), &canonical.join("0/x.sav")).is_ok());
        assert!(ensure_no_symlinks(&canonical, &dir.path().join("0/x.sav")).is_ok());
        assert!(ensure_no_symlinks(dir.path(), &dir.path().join("../x.sav")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_ancestor_of_the_root_is_fine_but_links_below_it_are_not() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir_all(real.join("sub")).unwrap();
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        assert!(ensure_no_symlinks(&alias, &real.join("sub/x.sav")).is_ok());
        assert!(ensure_no_symlinks(&real, &alias.join("sub/x.sav")).is_ok());
        std::os::unix::fs::symlink(real.join("sub"), real.join("inner")).unwrap();
        assert!(
            ensure_no_symlinks(&real, &real.join("inner/x.sav")).is_err(),
            "even pointing inside"
        );
    }

    #[test]
    fn list_files_skips_orphaned_temp_files_without_deleting_them() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("0")).unwrap();
        std::fs::write(
            dir.path().join("0/.leemusync-0011223344556677.tmp"),
            b"half",
        )
        .unwrap();
        std::fs::write(dir.path().join(".leemusync-aa.tmp"), b"half").unwrap();
        std::fs::write(dir.path().join("0/a.sav"), b"a").unwrap();
        std::fs::write(dir.path().join(".leemusync-notes.txt"), b"kept").unwrap();
        let names: Vec<String> = list_files(dir.path())
            .unwrap()
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(names, [".leemusync-notes.txt", "0/a.sav"]);
        assert!(
            dir.path()
                .join("0/.leemusync-0011223344556677.tmp")
                .exists()
        );
    }
}
