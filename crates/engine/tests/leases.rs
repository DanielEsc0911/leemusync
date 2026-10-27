//! Advisory leases between two simulated PCs on one shared remote (sync-model §Leases).
#![allow(clippy::unwrap_used)] // tests: a failure here is a failed test

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::{Pc, SAVE, join, map};
use lemusync_api::WarningCode;
use lemusync_core::{PlayerId, StreamId};
use lemusync_engine::{Acquire, LeaseHandle, MapChoice};

const GAME: &str = "101c9400";

struct Two {
    _root: tempfile::TempDir,
    remote: PathBuf,
    a: Pc,
    b: Pc,
    stream: StreamId,
    player: PlayerId,
}

/// A creates the repo with player `P` on account 80000001 and uploads; B joins and adopts it.
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
    let (stream, player) = a.engine.stream_for("P", "cemu", GAME).await.unwrap();
    Two {
        _root: root,
        remote,
        a,
        b,
        stream,
        player,
    }
}

fn lock_dir(remote: &Path, stream: StreamId) -> PathBuf {
    remote.join("lemusync/locks").join(stream.to_string())
}

fn lock_files(remote: &Path, stream: StreamId) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(lock_dir(remote, stream))
        .map(|d| {
            d.map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn lower(pc: &Pc) -> String {
    pc.engine.device_id().to_string().to_lowercase()
}

fn ours(a: Acquire) -> LeaseHandle {
    match a {
        Acquire::Ours(h) => h,
        Acquire::HeldElsewhere { .. } => panic!("expected our lease"),
    }
}

#[tokio::test]
async fn a_held_lease_is_reported_with_the_holder_and_released_by_it_only() {
    let mut t = two().await;
    let (got, w) =
        t.a.engine
            .lease_acquire(t.stream, t.player, false)
            .await
            .unwrap();
    assert_eq!(w, vec![]);
    let mut h = ours(got);
    t.a.engine.lease_renew(&mut h).await.unwrap();
    let (got, _) =
        t.b.engine
            .lease_acquire(t.stream, t.player, true)
            .await
            .unwrap();
    match got {
        Acquire::HeldElsewhere {
            device,
            device_name,
            stale,
            ..
        } => {
            assert_eq!(device, t.a.engine.device_id());
            assert_eq!(device_name, t.a.engine.local_config().device.name);
            assert!(!stale, "a live lease is never taken over");
        }
        Acquire::Ours(_) => panic!("B must not get A's live lease"),
    }
    assert_eq!(lock_files(&t.remote, t.stream), vec![lower(&t.a)]);
    let (status, _) = t.b.engine.lease_status().await.unwrap();
    assert_eq!(status.len(), 1);
    assert!(status[0].live && !status[0].ours && status[0].player == "P");

    // B's forced release only ever deletes B's own lease.
    t.b.engine.lease_release_forced(t.stream).await.unwrap();
    assert_eq!(t.b.engine.lease_release_game(GAME).await.unwrap(), 0);
    assert_eq!(lock_files(&t.remote, t.stream), vec![lower(&t.a)]);
    t.a.engine.lease_release(h).await.unwrap();
    assert_eq!(lock_files(&t.remote, t.stream), Vec::<String>::new());
    let (got, _) =
        t.b.engine
            .lease_acquire(t.stream, t.player, false)
            .await
            .unwrap();
    ours(got);
    // `lease release --force <game>`: B's own lease, by game id in any case.
    assert_eq!(t.b.engine.lease_release_game("101C9400").await.unwrap(), 1);
    assert_eq!(lock_files(&t.remote, t.stream), Vec::<String>::new());
}

#[tokio::test]
async fn when_both_write_at_once_exactly_one_keeps_the_lease() {
    let mut t = two().await;
    // Both pause after writing their lease, so each lists again with both leases present.
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    t.a.engine.arm_pause("lease.after_write", barrier.clone());
    t.b.engine.arm_pause("lease.after_write", barrier);
    let (ra, rb) = tokio::join!(
        t.a.engine.lease_acquire(t.stream, t.player, false),
        t.b.engine.lease_acquire(t.stream, t.player, false),
    );
    let (ra, rb) = (ra.unwrap().0, rb.unwrap().0);
    let (winner, loser, held_by) = match (&ra, &rb) {
        (Acquire::Ours(_), Acquire::HeldElsewhere { device, .. }) => (&t.a, &t.b, *device),
        (Acquire::HeldElsewhere { device, .. }, Acquire::Ours(_)) => (&t.b, &t.a, *device),
        _ => panic!("exactly one must win: {ra:?} / {rb:?}"),
    };
    assert_eq!(held_by, winner.engine.device_id());
    assert_ne!(loser.engine.device_id(), held_by);
    assert_eq!(
        lock_files(&t.remote, t.stream),
        vec![lower(winner)],
        "the loser deleted its own lease"
    );
}

#[tokio::test]
async fn a_stale_lease_is_taken_over_only_on_request_and_recorded() {
    let mut t = two().await;
    let (got, _) =
        t.a.engine
            .lease_acquire(t.stream, t.player, false)
            .await
            .unwrap();
    ours(got);
    // B's clock is 10 min ahead: A's lease is past TTL (5 min) + skew (2 min).
    t.b.engine.shift_lease_clock_ms(10 * 60_000);
    let (got, _) =
        t.b.engine
            .lease_acquire(t.stream, t.player, false)
            .await
            .unwrap();
    assert!(matches!(
        got,
        Acquire::HeldElsewhere { stale: true, device, .. } if device == t.a.engine.device_id()
    ));
    let (got, _) =
        t.b.engine
            .lease_acquire(t.stream, t.player, true)
            .await
            .unwrap();
    ours(got);
    assert_eq!(
        lock_files(&t.remote, t.stream).len(),
        2,
        "A's file is A's to delete"
    );

    std::fs::write(
        t.b.cemu.join(SAVE).join("80000001/0/game_data.sav"),
        b"B played",
    )
    .unwrap();
    let r = t.b.sync().await;
    assert_eq!(
        r.streams[0].action,
        lemusync_api::StreamAction::Upload,
        "{r:?}"
    );
    let history = t.b.engine.history("P", "cemu", GAME).await.unwrap();
    assert_eq!(
        history.versions[0].takeover,
        Some(t.a.engine.device_id().to_string())
    );
    assert_eq!(history.versions[1].takeover, None);
}

#[tokio::test]
async fn a_lease_that_does_not_decrypt_is_ignored_with_a_warning() {
    let mut t = two().await;
    let (got, _) =
        t.a.engine
            .lease_acquire(t.stream, t.player, false)
            .await
            .unwrap();
    let h = ours(got);
    // A's real lease copied under another device's name fails its AAD; garbage fails to open.
    let dir = lock_dir(&t.remote, t.stream);
    let a_file = dir.join(lower(&t.a));
    std::fs::copy(&a_file, dir.join(lower(&t.b))).unwrap();
    t.a.engine.lease_release(h).await.unwrap();
    let fake = "01arz3ndektsv4rrffq69g5fav";
    std::fs::write(dir.join(fake), b"not a lease").unwrap();

    let (got, warnings) =
        t.a.engine
            .lease_acquire(t.stream, t.player, false)
            .await
            .unwrap();
    ours(got);
    let unreadable: Vec<&str> = warnings
        .iter()
        .filter(|w| w.code == WarningCode::LeaseUnreadable)
        .map(|w| w.detail.as_str())
        .collect();
    assert_eq!(unreadable.len(), 2, "{warnings:?}");
}
