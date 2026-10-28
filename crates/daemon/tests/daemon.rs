//! The `lemusyncd` binary under `LEMUSYNC_TEST_ROOT` (config, data and socket in a tempdir),
//! `--headless` so no D-Bus is touched. No repo is set up, so the OS keystore is never read.
#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used, clippy::panic)] // tests: a failure here is a failed test

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use lemusync_api::{Event, Request, Response};
use lemusync_ipc::Client;

const DEADLINE: Duration = Duration::from_secs(20);

fn daemon(root: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_lemusyncd"));
    c.arg("--headless")
        .env("LEMUSYNC_TEST_ROOT", root)
        .env("LANG", "C")
        .env_remove("LC_ALL")
        .env_remove("LC_MESSAGES")
        .env_remove("LANGUAGE")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    c
}

fn socket(root: &Path) -> PathBuf {
    root.join("run").join("daemon.sock")
}

/// Starts a daemon and waits until its socket answers.
fn start(root: &Path) -> Child {
    let mut child = daemon(root).spawn().unwrap();
    let end = Instant::now() + DEADLINE;
    while std::os::unix::net::UnixStream::connect(socket(root)).is_err() {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("lemusyncd exited early: {status}");
        }
        assert!(Instant::now() < end, "no socket");
        std::thread::sleep(Duration::from_millis(20));
    }
    child
}

fn term(child: &Child) {
    let ok = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

fn wait(mut child: Child) -> std::process::ExitStatus {
    let end = Instant::now() + DEADLINE;
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            return s;
        }
        assert!(Instant::now() < end, "lemusyncd did not exit");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_second_daemon_exits_with_a_clear_message() {
    let root = tempfile::tempdir().unwrap();
    let first = start(root.path());
    let out = daemon(root.path()).output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("already running"), "{err}");
    // The first one is untouched.
    assert!(std::os::unix::net::UnixStream::connect(socket(root.path())).is_ok());
    term(&first);
    assert!(wait(first).success());
}

#[test]
fn sigterm_stops_cleanly_and_leaves_no_socket() {
    let root = tempfile::tempdir().unwrap();
    let child = start(root.path());
    term(&child);
    assert!(wait(child).success());
    assert!(!socket(root.path()).exists());
    let log =
        std::fs::read_to_string(root.path().join("data/lemusync/logs/lemusyncd.log")).unwrap();
    assert!(log.contains("stopped"), "{log}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clients_get_answers_and_events_over_the_socket() {
    let root = tempfile::tempdir().unwrap();
    let child = start(root.path());
    let path = socket(root.path());
    // No repo yet: the engine says so, through the daemon.
    let mut c = Client::connect(&path, "test").await.unwrap();
    let r = c.call(Request::Status).await.unwrap();
    assert!(
        matches!(&r, Response::Status(_) | Response::Error(_)),
        "{r:?}"
    );
    let mut events = Box::pin(
        Client::connect(&path, "test")
            .await
            .unwrap()
            .subscribe()
            .await
            .unwrap(),
    );
    let first = tokio::time::timeout(DEADLINE, events.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(first, Event::StatusChanged(_)), "{first:?}");
    // `Shutdown` over IPC stops the daemon like SIGTERM.
    let mut c = Client::connect(&path, "test").await.unwrap();
    assert_eq!(
        c.call(Request::Shutdown).await.unwrap(),
        Response::ShuttingDown
    );
    let status = tokio::task::spawn_blocking(move || wait(child))
        .await
        .unwrap();
    assert!(status.success());
    assert!(!path.exists());
}
