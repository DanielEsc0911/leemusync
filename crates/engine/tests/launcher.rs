//! `OsLauncher` runs real processes. Kept in its own test binary: a fork while another test
//! holds the state lock would briefly share that lock with the child.
#![allow(clippy::unwrap_used)] // tests: a failure here is a failed test

use std::ffi::OsString;

use lemusync_engine::{Launcher, OsLauncher};

#[test]
fn the_os_launcher_runs_argv_without_a_shell() {
    let argv: Vec<OsString> = ["sh", "-c", "exit 3"].map(OsString::from).into();
    assert_eq!(OsLauncher.spawn(&argv).unwrap().wait().unwrap(), 3);
    // One argument with shell syntax stays one argument: `true` ignores it and exits 0.
    let argv: Vec<OsString> = ["true", "; exit 7"].map(OsString::from).into();
    assert_eq!(OsLauncher.spawn(&argv).unwrap().wait().unwrap(), 0);
    assert!(OsLauncher.spawn(&[]).is_err());
}
