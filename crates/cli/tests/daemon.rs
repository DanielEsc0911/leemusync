//! The CLI against a real `lemusyncd --headless` on a tempdir socket (`LEMUSYNC_TEST_ROOT`),
//! then the same commands with no daemon (in-process). No repo is set up, so the OS keystore
//! is never read. The marker that a command went through IPC: the daemon logs
//! `ipc client lemusync-cli connected` once per connection, and `status --json` carries a
//! `daemon` object only when the daemon answered.
#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used, clippy::panic)] // tests: a failure here is a failed test

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(30);

/// `lemusyncd` sits next to `lemusync` in the target dir; `cargo test --workspace` builds
/// it. Built here when this crate's tests run alone.
fn daemon_bin() -> PathBuf {
    let cli = PathBuf::from(env!("CARGO_BIN_EXE_lemusync"));
    let bin = cli.with_file_name("lemusyncd");
    if !bin.exists() {
        let ok = Command::new(env!("CARGO"))
            .args(["build", "-p", "lemusync-daemon", "--bin", "lemusyncd"])
            .status()
            .unwrap()
            .success();
        assert!(ok, "building lemusyncd");
    }
    bin
}

fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lemusync"))
        .args(args)
        .env("LEMUSYNC_TEST_ROOT", root)
        .env("LANG", "en_US.UTF-8")
        .env_remove("LC_ALL")
        .env_remove("LC_MESSAGES")
        .env_remove("LANGUAGE")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn start(root: &Path) -> Child {
    let mut child = Command::new(daemon_bin())
        .arg("--headless")
        .env("LEMUSYNC_TEST_ROOT", root)
        .env("LANG", "C")
        .env_remove("LC_ALL")
        .env_remove("LC_MESSAGES")
        .env_remove("LANGUAGE")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let socket = root.join("run").join("daemon.sock");
    let end = Instant::now() + DEADLINE;
    while std::os::unix::net::UnixStream::connect(&socket).is_err() {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("lemusyncd exited early: {status}");
        }
        assert!(Instant::now() < end, "no socket");
        std::thread::sleep(Duration::from_millis(20));
    }
    child
}

fn stop(mut child: Child) {
    let ok = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let end = Instant::now() + DEADLINE;
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < end, "lemusyncd did not stop");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// How many CLI connections the daemon logged.
fn connections(root: &Path) -> usize {
    let log = root.join("data/lemusync/logs/lemusyncd.log");
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .matches("ipc client lemusync-cli connected")
        .count()
}

/// Runs `args`, checks it went through the daemon, returns (exit code, JSON).
fn through_daemon(root: &Path, args: &[&str]) -> (Option<i32>, serde_json::Value) {
    let before = connections(root);
    let out = cli(root, args);
    let end = Instant::now() + DEADLINE;
    while connections(root) <= before {
        assert!(Instant::now() < end, "{args:?} did not reach the daemon");
        std::thread::sleep(Duration::from_millis(20));
    }
    (out.status.code(), json(&out))
}

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)))
}

#[test]
fn commands_go_through_the_daemon_and_match_in_process() {
    let root = tempfile::tempdir().unwrap();
    let r = root.path();
    let daemon = start(r);

    let (code, mut status) = through_daemon(r, &["--json", "status"]);
    assert_eq!(code, Some(0));
    let live = status
        .as_object_mut()
        .unwrap()
        .remove("daemon")
        .expect("status from the daemon carries its live state");
    assert!(live["version"].is_string(), "{live}");
    assert!(live["status"].is_object(), "{live}");

    let sync = through_daemon(r, &["--json", "sync"]);
    let conflicts = through_daemon(r, &["--json", "conflicts"]);

    let (code, paused) = through_daemon(r, &["--json", "pause", "--for", "1h"]);
    assert_eq!(code, Some(0), "{paused}");
    let (_, during) = through_daemon(r, &["--json", "status"]);
    assert!(
        during["daemon"]["status"]["paused_until_ms"].is_u64(),
        "{during}"
    );
    let (code, resumed) = through_daemon(r, &["--json", "resume"]);
    assert_eq!(code, Some(0), "{resumed}");
    let (_, after) = through_daemon(r, &["--json", "status"]);
    assert!(
        after["daemon"]["status"]["paused_until_ms"].is_null(),
        "{after}"
    );

    stop(daemon);

    // No daemon: the same commands run in this process and print the same JSON.
    let out = cli(r, &["--json", "status"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(json(&out), status, "status: daemon vs in-process");
    for (args, via_daemon) in [
        (&["--json", "sync"][..], sync),
        (&["--json", "conflicts"], conflicts),
    ] {
        let out = cli(r, args);
        assert_eq!((out.status.code(), json(&out)), via_daemon, "{args:?}");
    }
    // Pause and resume need the daemon: a usage error, not a silent no-op.
    for args in [
        &["--json", "pause", "--for", "1h"][..],
        &["--json", "resume"],
    ] {
        let out = cli(r, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert_eq!(json(&out)["error"]["code"], "usage", "{args:?}");
    }
}

#[test]
fn doctor_json_has_the_desktop_checks() {
    let root = tempfile::tempdir().unwrap();
    let out = cli(root.path(), &["--json", "doctor"]);
    let report = json(&out);
    let names: Vec<&str> = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["name"].as_str())
        .collect();
    for want in ["daemon", "service", "linger", "tray host", "notifications"] {
        assert!(names.contains(&want), "{want} missing: {names:?}");
    }
}
