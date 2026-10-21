//! Launcher mode between two simulated PCs on one shared remote (sync-model §Launcher mode).
#![allow(clippy::unwrap_used)] // tests: a failure here is a failed test

mod common;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Pc, SAVE, join, map};
use leemusync_api::{ConflictInfo, PlayLease, StreamAction};
use leemusync_core::VersionId;
use leemusync_engine::{
    Child, EmulatorOverride, EngineError, Launcher, LeaseChoice, Locations, MapChoice, PlayPrompt,
};

const GAME: &str = "101c9400";

/// Writes `content` (if any) into the save after `ms`, then exits 0. Records the save it saw.
struct FakeLauncher {
    save: PathBuf,
    content: Option<Vec<u8>>,
    ms: u64,
    fail: bool,
    seen: Arc<Mutex<Option<Vec<u8>>>>,
    argv: Arc<Mutex<Vec<OsString>>>,
}

impl FakeLauncher {
    fn new(pc: &Pc, content: Option<&[u8]>, ms: u64) -> Self {
        Self {
            save: pc.cemu.join(SAVE).join("80000001/0/game_data.sav"),
            content: content.map(<[u8]>::to_vec),
            ms,
            fail: false,
            seen: Arc::default(),
            argv: Arc::default(),
        }
    }
}

struct FakeChild {
    save: PathBuf,
    content: Option<Vec<u8>>,
    ms: u64,
}

impl Launcher for FakeLauncher {
    fn spawn(&self, argv: &[OsString]) -> std::io::Result<Box<dyn Child>> {
        if self.fail {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no cemu"));
        }
        *self.argv.lock().unwrap() = argv.to_vec();
        *self.seen.lock().unwrap() = std::fs::read(&self.save).ok();
        Ok(Box::new(FakeChild {
            save: self.save.clone(),
            content: self.content.clone(),
            ms: self.ms,
        }))
    }
}

impl Child for FakeChild {
    fn wait(&mut self) -> std::io::Result<i32> {
        std::thread::sleep(Duration::from_millis(self.ms));
        if let Some(c) = &self.content {
            std::fs::write(&self.save, c)?;
        }
        Ok(0)
    }
}

/// Answers `choice` to a held lease; deletes `unlock` first (the holder releasing).
struct Prompt {
    choice: LeaseChoice,
    unlock: Option<PathBuf>,
    asked: AtomicU32,
}

impl Prompt {
    fn new(choice: LeaseChoice) -> Self {
        Self {
            choice,
            unlock: None,
            asked: AtomicU32::new(0),
        }
    }
}

impl PlayPrompt for Prompt {
    fn lease_held(&self, _by: &str, _ago: u64, _stale: bool) -> Result<LeaseChoice, EngineError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        if let Some(p) = &self.unlock {
            let _ = std::fs::remove_file(p);
        }
        Ok(self.choice)
    }

    fn conflict(&self, _c: &ConflictInfo) -> Result<Option<VersionId>, EngineError> {
        Ok(None)
    }
}

/// Points `[emulators.cemu]` at a fake exe and game, then reopens with short play timings.
fn launchable(pc: Pc) -> Pc {
    let path = Locations::under(&pc.root).config_dir.join("config.toml");
    let mut c = pc.engine.local_config().clone();
    c.emulators.insert(
        "cemu".into(),
        EmulatorOverride {
            root: None,
            exe: vec!["/usr/bin/cemu".into()],
            games: [(GAME.to_owned(), PathBuf::from("/games/botw.wux"))].into(),
        },
    );
    c.save(&path).unwrap();
    let mut pc = pc.reopen();
    pc.engine.set_play_timings(
        Duration::from_millis(50),
        Duration::from_millis(20),
        Duration::from_millis(20),
    );
    pc
}

struct Two {
    _root: tempfile::TempDir,
    remote: PathBuf,
    a: Pc,
    b: Pc,
}

async fn two() -> Two {
    let root = tempfile::tempdir().unwrap();
    let remote = root.path().join("remote");
    let mut a = Pc::new(&root.path().join("a"), true, b"same save");
    let mut b = Pc::new(&root.path().join("b"), false, b"same save");
    a.engine.backend_add_fs(&remote).unwrap();
    a.engine.init("pass phrase").await.unwrap();
    a.engine.players_add("P").await.unwrap();
    map(&mut a, "P", "80000001", MapChoice::Ask).await;
    a.sync().await;
    join(&mut b, &remote).await;
    map(&mut b, "P", "80000001", MapChoice::Replace).await;
    b.sync().await;
    Two {
        _root: root,
        remote,
        a: launchable(a),
        b: launchable(b),
    }
}

fn lock_files(remote: &Path) -> usize {
    let dir = remote.join("leemusync/locks");
    std::fs::read_dir(dir).map_or(0, |d| {
        d.flatten()
            .map(|s| std::fs::read_dir(s.path()).map_or(0, Iterator::count))
            .sum()
    })
}

fn versions(remote: &Path) -> usize {
    let dir = remote.join("leemusync/streams");
    std::fs::read_dir(dir).map_or(0, |d| {
        d.flatten()
            .map(|s| std::fs::read_dir(s.path().join("versions")).map_or(0, Iterator::count))
            .sum()
    })
}

#[tokio::test]
async fn a_handoff_starts_b_from_a_s_version_sc2() {
    let mut t = two().await;
    let la = FakeLauncher::new(&t.a, Some(b"played on A"), 10);
    let p = Prompt::new(LeaseChoice::Wait);
    let r =
        t.a.engine
            .play("P", "cemu", GAME, &[], &p, &la)
            .await
            .unwrap();
    assert_eq!(r.uploaded.action, StreamAction::Upload);
    assert_eq!(r.lease, PlayLease::Held);
    assert_eq!(
        *la.argv.lock().unwrap(),
        ["/usr/bin/cemu", "-g", "/games/botw.wux"]
    );
    assert_eq!(lock_files(&t.remote), 0, "A released");

    let lb = FakeLauncher::new(&t.b, Some(b"played on B"), 10);
    let r =
        t.b.engine
            .play("P", "cemu", GAME, &[], &p, &lb)
            .await
            .unwrap();
    assert_eq!(r.pulled.action, StreamAction::FastForward);
    assert_eq!(
        lb.seen.lock().unwrap().as_deref(),
        Some(&b"played on A"[..])
    );
    assert_eq!(r.uploaded.action, StreamAction::Upload);
    assert_eq!(p.asked.load(Ordering::SeqCst), 0);
    assert_eq!(lock_files(&t.remote), 0);
}

#[tokio::test]
async fn a_held_lease_waits_until_the_holder_releases() {
    let mut t = two().await;
    let (stream, player) = t.a.engine.stream_for("P", "cemu", GAME).await.unwrap();
    let (got, _) =
        t.a.engine
            .lease_acquire(stream, player, false)
            .await
            .unwrap();
    assert!(matches!(got, leemusync_engine::Acquire::Ours(_)));
    let a_lock = t
        .remote
        .join("leemusync/locks")
        .join(stream.to_string())
        .join(t.a.engine.device_id().to_string().to_lowercase());
    assert!(a_lock.exists());
    let p = Prompt {
        unlock: Some(a_lock),
        ..Prompt::new(LeaseChoice::Wait)
    };
    let lb = FakeLauncher::new(&t.b, Some(b"B"), 10);
    let r =
        t.b.engine
            .play("P", "cemu", GAME, &[], &p, &lb)
            .await
            .unwrap();
    assert_eq!(p.asked.load(Ordering::SeqCst), 1);
    assert_eq!(r.lease, PlayLease::Held);
    assert!(
        lb.seen.lock().unwrap().is_some(),
        "B launched after the wait"
    );
    assert_eq!(lock_files(&t.remote), 0);
}

#[tokio::test]
async fn playing_anyway_on_both_ends_ends_as_a_conflict_with_both_versions_sc3() {
    let t = two().await;
    let (mut a, mut b) = (t.a, t.b);
    let before = versions(&t.remote);
    let p = Prompt::new(LeaseChoice::PlayAnyway);
    let la = FakeLauncher::new(&a, Some(b"A's run"), 300);
    // B exits later, so its upload sees A's version and reports the divergence.
    let lb = FakeLauncher::new(&b, Some(b"B's run"), 800);
    let (ra, rb) = tokio::join!(
        a.engine.play("P", "cemu", GAME, &[], &p, &la),
        b.engine.play("P", "cemu", GAME, &[], &p, &lb),
    );
    let (ra, rb) = (ra.unwrap(), rb.unwrap());
    assert_eq!(p.asked.load(Ordering::SeqCst), 1, "only the second asks");
    let mut leases = [ra.lease, rb.lease];
    leases.sort_by_key(|l| *l == PlayLease::PlayedAnyway);
    assert_eq!(leases, [PlayLease::Held, PlayLease::PlayedAnyway]);
    assert_eq!(ra.uploaded.action, StreamAction::Upload);
    assert_eq!(rb.uploaded.action, StreamAction::UploadThenConflict);
    assert_eq!(versions(&t.remote), before + 2, "both runs are kept");
    let (conflicts, _) = a.engine.conflicts().await.unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].heads.len(), 2);
    assert_eq!(lock_files(&t.remote), 0);
}

#[tokio::test]
async fn the_lease_is_renewed_while_the_emulator_runs() {
    let mut t = two().await;
    let la = FakeLauncher::new(&t.a, None, 400);
    let p = Prompt::new(LeaseChoice::Wait);
    let r =
        t.a.engine
            .play("P", "cemu", GAME, &[], &p, &la)
            .await
            .unwrap();
    assert!(r.renewals >= 3, "renewals {}", r.renewals);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
}

#[tokio::test]
async fn a_launch_error_releases_the_lease() {
    let mut t = two().await;
    let mut la = FakeLauncher::new(&t.a, None, 0);
    la.fail = true;
    let p = Prompt::new(LeaseChoice::Wait);
    let e = t.a.engine.play("P", "cemu", GAME, &[], &p, &la).await;
    assert!(matches!(e, Err(EngineError::Io(_))), "{e:?}");
    assert_eq!(lock_files(&t.remote), 0);
}

#[tokio::test]
async fn an_exit_without_changes_uploads_nothing() {
    let mut t = two().await;
    let before = versions(&t.remote);
    let la = FakeLauncher::new(&t.a, None, 10);
    let p = Prompt::new(LeaseChoice::Wait);
    let r =
        t.a.engine
            .play("P", "cemu", GAME, &[], &p, &la)
            .await
            .unwrap();
    assert_eq!(r.uploaded.action, StreamAction::Nothing);
    assert_eq!(r.uploaded.version, None);
    assert_eq!(versions(&t.remote), before);
}

#[tokio::test]
async fn a_running_emulator_is_refused_before_anything_starts() {
    let mut t = two().await;
    t.a.probe.set(true);
    let la = FakeLauncher::new(&t.a, Some(b"x"), 0);
    let p = Prompt::new(LeaseChoice::Wait);
    let e = t.a.engine.play("P", "cemu", GAME, &[], &p, &la).await;
    assert!(matches!(e, Err(EngineError::EmulatorRunning(_))), "{e:?}");
    assert!(la.seen.lock().unwrap().is_none(), "never launched");
    assert_eq!(lock_files(&t.remote), 0);
}
