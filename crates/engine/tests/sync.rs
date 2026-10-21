//! Sync behaviour on one or two simulated PCs (fake data).

mod common;

use common::{Pc, join, map};
use leemusync_api::StreamAction;
use leemusync_engine::{EngineError, MapChoice, SyncOptions};

#[tokio::test]
async fn first_sync_needs_confirmation_and_uploads() {
    let root = tempfile::tempdir().unwrap();
    let remote = root.path().join("remote");
    let mut a = Pc::new(&root.path().join("a"), true, b"A's save");
    a.engine.backend_add_fs(&remote).unwrap();
    a.engine.init("pass phrase").await.unwrap();
    a.engine.players_add("Ana").await.unwrap();
    map(&mut a, "Ana", "80000001", MapChoice::Ask).await;
    let dry = a.engine.sync(SyncOptions { dry_run: true }).await.unwrap();
    assert_eq!(dry.streams[0].action, StreamAction::Upload);
    assert!(matches!(
        a.engine.sync(SyncOptions { dry_run: false }).await,
        Err(EngineError::NeedsConfirmation)
    ));
    let r = a.sync().await;
    assert_eq!(r.streams[0].action, StreamAction::Upload);
    assert!(r.streams[0].done);
    assert_eq!(
        a.sync().await.streams[0].action,
        StreamAction::Nothing,
        "second sync has nothing to do"
    );
}

#[tokio::test]
async fn emulator_running_skips_everything() {
    let root = tempfile::tempdir().unwrap();
    let mut a = Pc::new(&root.path().join("a"), false, b"x");
    a.engine
        .backend_add_fs(&root.path().join("remote"))
        .unwrap();
    a.engine.init("pass phrase").await.unwrap();
    a.engine.players_add("Ana").await.unwrap();
    map(&mut a, "Ana", "80000001", MapChoice::Ask).await;
    a.probe.set(true);
    let r = a.sync().await;
    assert_eq!(r.streams[0].action, StreamAction::Skipped);
    assert!(
        r.warnings
            .iter()
            .any(|w| w.code == leemusync_api::WarningCode::EmulatorRunning)
    );
}

#[tokio::test]
async fn edit_on_one_pc_fast_forwards_the_other() {
    let root = tempfile::tempdir().unwrap();
    let remote = root.path().join("remote");
    let mut a = Pc::new(&root.path().join("a"), true, b"v1");
    a.engine.backend_add_fs(&remote).unwrap();
    a.engine.init("pass phrase").await.unwrap();
    a.engine.players_add("Ana").await.unwrap();
    map(&mut a, "Ana", "80000001", MapChoice::Ask).await;
    a.sync().await;
    let mut b = Pc::new(&root.path().join("b"), false, b"other");
    join(&mut b, &remote).await;
    b.create_account("80000002");
    map(&mut b, "Ana", "80000002", MapChoice::Ask).await;
    assert_eq!(b.sync().await.streams[0].action, StreamAction::FastForward);
    assert_eq!(b.save("80000002"), b"v1");
    // A different length, so the size changes even if the mtime lands in the same millisecond.
    std::fs::write(
        b.cemu.join(common::SAVE).join("80000002/0/game_data.sav"),
        b"version two",
    )
    .unwrap();
    assert_eq!(b.sync().await.streams[0].action, StreamAction::Upload);
    assert_eq!(a.sync().await.streams[0].action, StreamAction::FastForward);
    assert_eq!(a.save("80000001"), b"version two");
}

#[tokio::test]
async fn progress_reports_each_transfer_and_an_unchanged_save_moves_nothing() {
    use leemusync_api::{TransferPhase, TransferProgress};
    use std::sync::{Arc, Mutex};
    let seen: Arc<Mutex<Vec<TransferProgress>>> = Arc::default();
    let sink = {
        let seen = seen.clone();
        Arc::new(move |p: &TransferProgress| seen.lock().unwrap().push(p.clone()))
    };
    let root = tempfile::tempdir().unwrap();
    let remote = root.path().join("remote");
    let mut a = Pc::new(&root.path().join("a"), true, b"A's save");
    a.engine.set_progress(sink.clone());
    a.engine.backend_add_fs(&remote).unwrap();
    a.engine.init("pass phrase").await.unwrap();
    a.engine.players_add("Ana").await.unwrap();
    map(&mut a, "Ana", "80000001", MapChoice::Ask).await;
    let r = a.sync().await;
    let up = seen.lock().unwrap().clone();
    assert_eq!(up[0].phase, TransferPhase::Checking);
    assert_eq!(
        (up[0].player.as_str(), up[0].game.as_str()),
        ("Ana", "101c9400")
    );
    let last = up.last().unwrap();
    assert_eq!(last.phase, TransferPhase::Uploading);
    assert!(last.files_total > 0);
    assert_eq!(last.files_done, last.files_total);
    assert_eq!(last.bytes_done, last.bytes_total);
    assert_eq!(
        (r.streams[0].files, r.streams[0].bytes),
        (last.files_total, last.bytes_total)
    );

    let again = a.sync().await;
    assert_eq!((again.streams[0].files, again.streams[0].bytes), (0, 0));

    seen.lock().unwrap().clear();
    let mut b = Pc::new(&root.path().join("b"), false, b"other");
    b.engine.set_progress(sink);
    join(&mut b, &remote).await;
    b.create_account("80000002");
    map(&mut b, "Ana", "80000002", MapChoice::Ask).await;
    let r = b.sync().await;
    assert_eq!(r.streams[0].action, StreamAction::FastForward);
    let down = seen.lock().unwrap().clone();
    let last = down.last().unwrap();
    assert_eq!(last.phase, TransferPhase::Downloading);
    assert_eq!(last.files_done, last.files_total);
    assert_eq!(r.streams[0].files, last.files_total);
}

/// Ana's save diverged: A and B both edited from one base and both synced.
#[allow(clippy::unwrap_used)] // test helper
async fn diverged(root: &std::path::Path) -> (Pc, Pc) {
    let remote = root.join("remote");
    let mut a = Pc::new(&root.join("a"), true, b"base");
    a.engine.backend_add_fs(&remote).unwrap();
    a.engine.init("pass phrase").await.unwrap();
    a.engine.players_add("Ana").await.unwrap();
    map(&mut a, "Ana", "80000001", MapChoice::Ask).await;
    a.sync().await;
    let mut b = Pc::new(&root.join("b"), false, b"other");
    join(&mut b, &remote).await;
    b.create_account("80000002");
    map(&mut b, "Ana", "80000002", MapChoice::Ask).await;
    b.sync().await;
    std::fs::write(
        a.cemu.join(common::SAVE).join("80000001/0/game_data.sav"),
        b"edited on A",
    )
    .unwrap();
    std::fs::write(
        b.cemu.join(common::SAVE).join("80000002/0/game_data.sav"),
        b"edited on B, longer",
    )
    .unwrap();
    assert_eq!(a.sync().await.streams[0].action, StreamAction::Upload);
    assert_eq!(
        b.sync().await.streams[0].action,
        StreamAction::UploadThenConflict
    );
    (a, b)
}

#[tokio::test]
async fn a_conflict_is_listed_and_resolved_with_nothing_lost() {
    let root = tempfile::tempdir().unwrap();
    let (a, mut b) = diverged(root.path()).await;
    let (list, _) = b.engine.conflicts().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(
        (list[0].player.as_str(), list[0].game.as_str()),
        ("Ana", "101c9400")
    );
    assert_eq!(list[0].heads.len(), 2);
    let history = b.engine.history("Ana", "cemu", "101c9400").await.unwrap();
    let a_head: leemusync_core::VersionId = history
        .versions
        .iter()
        .find(|v| v.head && v.device == a.engine.device_id().to_string())
        .unwrap()
        .id
        .parse()
        .unwrap();
    let b_head = history
        .versions
        .iter()
        .find(|v| v.head && v.device == b.engine.device_id().to_string())
        .unwrap()
        .id
        .clone();

    // A version that isn't a head is refused.
    let old: leemusync_core::VersionId = history
        .versions
        .iter()
        .find(|v| !v.head)
        .unwrap()
        .id
        .parse()
        .unwrap();
    assert!(matches!(
        b.engine.resolve("Ana", "cemu", "101c9400", old).await,
        Err(EngineError::Usage(_))
    ));

    let (r, _) = b
        .engine
        .resolve("Ana", "cemu", "101c9400", a_head)
        .await
        .unwrap();
    assert_eq!(r.action, StreamAction::Resolved);
    assert_eq!(b.save("80000002"), b"edited on A");
    let after = b.engine.history("Ana", "cemu", "101c9400").await.unwrap();
    let heads: Vec<_> = after.versions.iter().filter(|v| v.head).collect();
    assert_eq!(heads.len(), 1);
    let mut parents = heads[0].parents.clone();
    parents.sort();
    let mut want = vec![a_head.to_string(), b_head.clone()];
    want.sort();
    assert_eq!(parents, want, "the resolution's parents are both heads");
    assert!(
        after.backups.iter().any(|n| n.ends_with("-pre-resolve")),
        "B's files went to local history first"
    );
    assert_eq!(b.engine.conflicts().await.unwrap().0, vec![]);
    assert_eq!(b.sync().await.streams[0].action, StreamAction::Nothing);

    // B's own head is still there to restore.
    b.engine
        .restore("Ana", "cemu", "101c9400", &b_head)
        .await
        .unwrap();
    assert_eq!(b.save("80000002"), b"edited on B, longer");
}

#[tokio::test]
async fn newest_policy_resolves_during_sync() {
    let root = tempfile::tempdir().unwrap();
    let remote = root.path().join("remote");
    let mut a = Pc::new(&root.path().join("a"), true, b"base");
    a.engine.backend_add_fs(&remote).unwrap();
    a.engine.init("pass phrase").await.unwrap();
    a.engine.players_add("Ana").await.unwrap();
    map(&mut a, "Ana", "80000001", MapChoice::Ask).await;
    a.sync().await;
    let mut b = Pc::new(&root.path().join("b"), false, b"other");
    join(&mut b, &remote).await;
    b.create_account("80000002");
    map(&mut b, "Ana", "80000002", MapChoice::Ask).await;
    b.sync().await;
    b.engine
        .set_conflict_policy(leemusync_engine::ConflictPolicy::Newest)
        .unwrap();
    std::fs::write(
        a.cemu.join(common::SAVE).join("80000001/0/game_data.sav"),
        b"edited on A",
    )
    .unwrap();
    a.sync().await;
    std::fs::write(
        b.cemu.join(common::SAVE).join("80000002/0/game_data.sav"),
        b"edited on B, later",
    )
    .unwrap();
    let r = b.sync().await;
    assert_eq!(r.streams[0].action, StreamAction::Resolved);
    let h = b.engine.history("Ana", "cemu", "101c9400").await.unwrap();
    assert_eq!(h.versions.iter().filter(|v| v.head).count(), 1);
    assert_eq!(
        h.versions.iter().find(|v| v.head).unwrap().id,
        r.streams[0].version.clone().unwrap()
    );
    // B's upload is the newest head, so its files are kept; A's head stays in history.
    assert_eq!(b.save("80000002"), b"edited on B, later");
    assert_eq!(h.versions.len(), 4);
    assert_eq!(a.sync().await.streams[0].action, StreamAction::FastForward);
    assert_eq!(a.save("80000001"), b"edited on B, later");
}
