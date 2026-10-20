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
