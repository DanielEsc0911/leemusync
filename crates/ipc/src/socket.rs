//! Where the socket lives, and the same-user checks (security §Local IPC).

use std::ffi::OsString;
use std::path::PathBuf;

use crate::IpcError;

/// `$XDG_RUNTIME_DIR/lemusync/daemon.sock`, or `<LEMUSYNC_TEST_ROOT>/run/daemon.sock` in tests.
/// Never falls back to `/tmp`.
///
/// # Errors
/// [`IpcError::NoRuntimeDir`] when neither variable is set.
pub fn default_path() -> Result<PathBuf, IpcError> {
    path_from(
        std::env::var_os("LEMUSYNC_TEST_ROOT"),
        std::env::var_os("XDG_RUNTIME_DIR"),
    )
}

fn path_from(test_root: Option<OsString>, runtime: Option<OsString>) -> Result<PathBuf, IpcError> {
    if let Some(root) = test_root.filter(|r| !r.is_empty()) {
        return Ok(PathBuf::from(root).join("run").join("daemon.sock"));
    }
    match runtime.filter(|r| !r.is_empty()) {
        Some(dir) => Ok(PathBuf::from(dir).join("lemusync").join("daemon.sock")),
        None => Err(IpcError::NoRuntimeDir),
    }
}

/// True when a peer with uid `peer` may talk to a daemon running as `own`.
#[must_use]
pub fn same_user(peer: u32, own: u32) -> bool {
    peer == own
}

/// This process's effective uid.
#[cfg(unix)]
#[must_use]
pub fn own_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_must_be_the_same_uid() {
        assert!(same_user(1000, 1000));
        assert!(!same_user(0, 1000));
        assert!(!same_user(1001, 1000));
    }

    #[test]
    fn path_rules() {
        let p = path_from(None, Some("/run/user/1000".into())).unwrap();
        assert_eq!(p, PathBuf::from("/run/user/1000/lemusync/daemon.sock"));
        let p = path_from(Some("/t".into()), Some("/run/user/1000".into())).unwrap();
        assert_eq!(p, PathBuf::from("/t/run/daemon.sock"));
        assert!(matches!(path_from(None, None), Err(IpcError::NoRuntimeDir)));
        assert!(matches!(
            path_from(Some("".into()), Some("".into())),
            Err(IpcError::NoRuntimeDir)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn own_uid_owns_new_files() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(std::fs::metadata(dir.path()).unwrap().uid(), own_uid());
    }
}
