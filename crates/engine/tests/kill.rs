//! Crash matrix (SC5): a child process arms a failpoint in `Kill` mode and aborts there, like
//! `kill -9` at that journal step. The parent reopens the engine and checks I1–I6: local files
//! are the old or the new version, never mixed; no extra heads; the backup is there.
//!
//! The child is this test binary run again on `kill_child` with the case in env vars.
#![allow(clippy::unwrap_used)] // tests: a failure here is a failed test

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{Pc, SAVE, join, map};
use leemusync_api::StreamAction;
use leemusync_core::VersionId;
use leemusync_engine::{Acquire, JoinSecret, MapChoice, SyncOptions, fsx};

const GAME: &str = "101c9400";
const ENV_ROOT: &str = "LEEMUSYNC_KILL_ROOT";
const ENV_POINT: &str = "LEEMUSYNC_KILL_POINT";

/// Every `hit` name in `crates/engine/src`. `every_failpoint_has_a_crash_case` keeps this whole.
const CASES: &[&str] = &[
    "init.after_create",
    "init.after_key",
    "init.before_config",
    "lease.after_write",
    "resolve.after_commit",
    "restore.after_backup",
    "restore.before_done",
    "restore.mid_swap",
    "upload.after_manifest",
    "upload.before_manifest",
];

/// Which simulated PC the child opens, and whether its Cemu is the Flatpak one.
fn victim(point: &str) -> (&'static str, bool) {
    if point.starts_with("restore.") || point.starts_with("resolve.") {
        ("b", false)
    } else {
        ("a", true)
    }
}

/// Every file under `dir` with its bytes (write temps excluded).
fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fsx::list_files(dir)
        .unwrap()
        .into_iter()
        .map(|(name, _)| {
            let bytes = std::fs::read(dir.join(&name)).unwrap();
            (name, bytes)
        })
        .collect()
}

fn account_dir(pc: &Pc, account: &str) -> PathBuf {
    pc.cemu.join(SAVE).join(account)
}

/// Runs the child for `point` on `root` and checks it died at the failpoint (abort), not by
/// finishing or by a panic.
fn crash(root: &Path, point: &str) {
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["kill_child", "--exact", "--nocapture", "--test-threads=1"])
        .env(ENV_ROOT, root)
        .env(ENV_POINT, point)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !out.status.success() && !stdout.contains("CHILD-FINISHED"),
        "{point}: the child must die at the failpoint\n{stdout}"
    );
    assert!(
        stdout.contains("CHILD-ARMED"),
        "{point}: the child panicked before arming\n{stdout}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(out.status.signal(), Some(6), "{point}: SIGABRT\n{stdout}");
    }
}

/// The child half: does nothing unless the parent set the env vars.
#[tokio::test]
async fn kill_child() {
    let (Ok(root), Ok(point)) = (std::env::var(ENV_ROOT), std::env::var(ENV_POINT)) else {
        return;
    };
    let point: &'static str = CASES.iter().find(|c| **c == point).unwrap();
    let (name, flatpak) = victim(point);
    let mut pc = Pc::attach(&PathBuf::from(root).join(name), flatpak);
    pc.engine.arm_kill(point);
    println!("CHILD-ARMED");
    let group = point.split('.').next().unwrap();
    match group {
        "init" => {
            let _ = pc.engine.init("pass phrase").await;
        }
        "lease" => {
            let (stream, player) = pc.engine.stream_for("Ana", "cemu", GAME).await.unwrap();
            let _ = pc.engine.lease_acquire(stream, player, false).await;
        }
        "resolve" => {
            let keep = other_head(&mut pc).await;
            let _ = pc.engine.resolve("Ana", "cemu", GAME, keep).await;
        }
        _ => {
            let _ = pc.engine.sync(SyncOptions { dry_run: false }).await;
        }
    }
    println!("CHILD-FINISHED");
}

/// The head another device wrote.
async fn other_head(pc: &mut Pc) -> VersionId {
    let me = pc.engine.device_id().to_string();
    let h = pc.engine.history("Ana", "cemu", GAME).await.unwrap();
    h.versions
        .iter()
        .find(|v| v.head && v.device != me)
        .unwrap()
        .id
        .parse()
        .unwrap()
}

async fn heads(pc: &mut Pc) -> usize {
    let h = pc.engine.history("Ana", "cemu", GAME).await.unwrap();
    h.versions.iter().filter(|v| v.head).count()
}

/// A at "version one" then "version two!"; B joined at "version one" on account 80000002.
async fn behind(root: &Path) -> (Pc, Pc) {
    let remote = root.join("remote");
    let mut a = Pc::new(&root.join("a"), true, b"version one");
    a.engine.backend_add_fs(&remote).unwrap();
    a.engine.init("pass phrase").await.unwrap();
    a.engine.players_add("Ana").await.unwrap();
    map(&mut a, "Ana", "80000001", MapChoice::Ask).await;
    a.sync().await;
    let mut b = Pc::new(&root.join("b"), false, b"b's own");
    join(&mut b, &remote).await;
    b.create_account("80000002");
    map(&mut b, "Ana", "80000002", MapChoice::Ask).await;
    b.sync().await;
    std::fs::write(
        account_dir(&a, "80000001").join("0/game_data.sav"),
        b"version two!",
    )
    .unwrap();
    a.sync().await;
    (a, b)
}

/// A and B both edited Ana's save from one base and both synced: two heads.
async fn diverged(root: &Path) -> (Pc, Pc) {
    let (mut a, mut b) = behind(root).await;
    b.sync().await;
    std::fs::write(
        account_dir(&a, "80000001").join("0/game_data.sav"),
        b"edited on A",
    )
    .unwrap();
    std::fs::write(
        account_dir(&b, "80000002").join("0/game_data.sav"),
        b"edited on B, longer",
    )
    .unwrap();
    a.sync().await;
    assert_eq!(
        b.sync().await.streams[0].action,
        StreamAction::UploadThenConflict
    );
    (a, b)
}

#[tokio::test]
async fn a_kill_during_a_fast_forward_leaves_the_old_or_new_files_and_a_backup() {
    for point in [
        "restore.after_backup",
        "restore.mid_swap",
        "restore.before_done",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (a, b) = behind(root.path()).await;
        let old = tree(&account_dir(&b, "80000002"));
        let new = tree(&account_dir(&a, "80000001"));
        drop((a, b));
        crash(root.path(), point);
        let mut b = Pc::attach(&root.path().join("b"), false);
        let now = tree(&account_dir(&b, "80000002"));
        assert!(now == old || now == new, "{point}: never mixed: {now:?}");
        let r = b.sync().await;
        assert!(
            matches!(
                r.streams[0].action,
                StreamAction::FastForward | StreamAction::Nothing
            ),
            "{point}: {:?}",
            r.streams[0].action
        );
        assert_eq!(tree(&account_dir(&b, "80000002")), new, "{point}");
        assert_eq!(heads(&mut b).await, 1, "{point}: no extra heads");
        let h = b.engine.history("Ana", "cemu", GAME).await.unwrap();
        assert!(!h.backups.is_empty(), "{point}: old files in local history");
        assert_eq!(b.sync().await.streams[0].action, StreamAction::Nothing);
    }
}

#[tokio::test]
async fn a_kill_around_the_manifest_never_makes_a_conflict() {
    for point in ["upload.before_manifest", "upload.after_manifest"] {
        let root = tempfile::tempdir().unwrap();
        let mut a = Pc::new(&root.path().join("a"), true, b"first");
        a.engine
            .backend_add_fs(&root.path().join("remote"))
            .unwrap();
        a.engine.init("pass phrase").await.unwrap();
        a.engine.players_add("Ana").await.unwrap();
        map(&mut a, "Ana", "80000001", MapChoice::Ask).await;
        a.engine.confirm_first_sync().unwrap();
        let before = tree(&account_dir(&a, "80000001"));
        drop(a);
        crash(root.path(), point);
        let mut a = Pc::attach(&root.path().join("a"), true);
        assert_eq!(tree(&account_dir(&a, "80000001")), before, "{point}");
        let r = a.sync().await;
        assert!(
            matches!(
                r.streams[0].action,
                StreamAction::Upload | StreamAction::Adopt
            ),
            "{point}: {:?}",
            r.streams[0].action
        );
        assert_eq!(a.sync().await.streams[0].action, StreamAction::Nothing);
        let h = a.engine.history("Ana", "cemu", GAME).await.unwrap();
        assert_eq!(h.versions.len(), 1, "{point}: one version, no conflict");
        assert_eq!(tree(&account_dir(&a, "80000001")), before, "{point}");
    }
}

#[tokio::test]
async fn a_kill_during_init_is_finished_by_join() {
    for point in ["init.after_create", "init.after_key", "init.before_config"] {
        let root = tempfile::tempdir().unwrap();
        let remote = root.path().join("remote");
        let mut a = Pc::new(&root.path().join("a"), true, b"first");
        a.engine.backend_add_fs(&remote).unwrap();
        drop(a);
        crash(root.path(), point);
        let mut a = Pc::attach(&root.path().join("a"), true);
        a.engine
            .join(JoinSecret::Passphrase("pass phrase".into()))
            .await
            .unwrap();
        a.engine.players_add("Ana").await.unwrap();
        assert_eq!(
            a.engine.players_list().await.unwrap(),
            vec!["Ana".to_string()],
            "{point}"
        );
        let repos = std::fs::read_dir(&remote).unwrap().count();
        assert_eq!(repos, 1, "{point}: one repo");
    }
}

#[tokio::test]
async fn a_kill_after_writing_a_lease_lets_this_device_take_it_again() {
    let point = "lease.after_write";
    let root = tempfile::tempdir().unwrap();
    let (a, b) = behind(root.path()).await;
    drop((a, b));
    crash(root.path(), point);
    let mut a = Pc::attach(&root.path().join("a"), true);
    let (stream, player) = a.engine.stream_for("Ana", "cemu", GAME).await.unwrap();
    let locks = root
        .path()
        .join("remote/leemusync/locks")
        .join(stream.to_string());
    assert_eq!(std::fs::read_dir(&locks).unwrap().count(), 1, "left behind");
    let (got, _) = a.engine.lease_acquire(stream, player, false).await.unwrap();
    let Acquire::Ours(h) = got else {
        panic!("our own stale lease must not block us");
    };
    a.engine.lease_release(h).await.unwrap();
    assert_eq!(std::fs::read_dir(&locks).unwrap().count(), 0);
}

#[tokio::test]
async fn a_kill_after_committing_a_resolution_fast_forwards_on_the_next_sync() {
    let point = "resolve.after_commit";
    let root = tempfile::tempdir().unwrap();
    let (a, b) = diverged(root.path()).await;
    let mine = tree(&account_dir(&b, "80000002"));
    let theirs = tree(&account_dir(&a, "80000001"));
    drop((a, b));
    crash(root.path(), point);
    let mut b = Pc::attach(&root.path().join("b"), false);
    assert_eq!(tree(&account_dir(&b, "80000002")), mine, "not restored yet");
    assert_eq!(heads(&mut b).await, 1, "the resolution is the one head");
    assert_eq!(b.sync().await.streams[0].action, StreamAction::FastForward);
    assert_eq!(tree(&account_dir(&b, "80000002")), theirs);
    let h = b.engine.history("Ana", "cemu", GAME).await.unwrap();
    assert!(!h.backups.is_empty(), "B's files went to local history");
    assert_eq!(b.engine.conflicts().await.unwrap().0, vec![]);
}

#[test]
fn every_failpoint_has_a_crash_case() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(src).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        for part in text.split("hit(\"").skip(1) {
            let name = part.split('"').next().unwrap();
            // Unit-test names in failpoints.rs are single letters.
            if name.contains('.') {
                found.push(name.to_owned());
            }
        }
    }
    found.sort();
    found.dedup();
    assert_eq!(found, CASES, "add a crash case to tests/kill.rs");
}
