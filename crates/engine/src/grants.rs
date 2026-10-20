//! The engine reads and writes only inside folders the user granted (security §File-system confinement).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::EngineError;

/// The canonical form of `path`, or of its nearest existing ancestor plus the missing tail.
/// `None` when the tail would contain `..` (it can't be resolved without the folders existing).
fn canonical_or_nearest(path: &Path) -> Option<PathBuf> {
    let mut probe = path.to_path_buf();
    let mut rest = Vec::new();
    loop {
        if let Ok(c) = probe.canonicalize() {
            return Some(
                rest.iter()
                    .rev()
                    .fold(c, |acc: PathBuf, part: &OsString| acc.join(part)),
            );
        }
        rest.push(probe.file_name()?.to_os_string());
        probe = probe.parent()?.to_path_buf();
    }
}

#[must_use]
pub fn is_granted(grants: &[PathBuf], path: &Path) -> bool {
    require(grants, path).is_ok()
}

/// # Errors
/// `NotGranted` naming the path.
pub fn require(grants: &[PathBuf], path: &Path) -> Result<PathBuf, EngineError> {
    let target = canonical_or_nearest(path)
        .ok_or_else(|| EngineError::NotGranted(path.display().to_string()))?;
    grants
        .iter()
        .find(|g| target.starts_with(g))
        .cloned()
        .ok_or_else(|| EngineError::NotGranted(path.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_granted_inside_a_grant_and_not_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let granted = dir.path().join("a").canonicalize().unwrap_or_else(|_| {
            std::fs::create_dir_all(dir.path().join("a")).unwrap();
            dir.path().join("a").canonicalize().unwrap()
        });
        std::fs::create_dir_all(dir.path().join("b")).unwrap();
        let grants = vec![granted.clone()];
        assert_eq!(require(&grants, &granted.join("x/y")).unwrap(), granted);
        assert!(!is_granted(&grants, &dir.path().join("b")));
        assert!(!is_granted(&grants, &dir.path().join("ab")));
    }

    #[test]
    fn dotdot_through_a_missing_folder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a")).unwrap();
        let grants = vec![dir.path().join("a").canonicalize().unwrap()];
        assert!(!is_granted(&grants, &dir.path().join("a/missing/../../b")));
    }
}
