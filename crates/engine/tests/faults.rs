//! Kill the engine at every journal step; on restart nothing is half old, half new (I1, I5).

mod common;

use common::{Pc, SAVE, join, map};
use leemusync_api::StreamAction;
use leemusync_engine::{EngineError, MapChoice, SyncOptions};

#[allow(clippy::unwrap_used)] // test helper: a failure here is a failed test
async fn two_pcs(root: &std::path::Path) -> (Pc, Pc) {
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
        a.cemu.join(SAVE).join("80000001/0/game_data.sav"),
        b"version two!",
    )
    .unwrap();
    a.sync().await;
    (a, b)
}

#[tokio::test]
async fn a_crash_during_a_fast_forward_rolls_back_then_retries() {
    for point in [
        "restore.after_backup",
        "restore.mid_swap",
        "restore.before_done",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (_a, b) = two_pcs(root.path()).await;
        assert_eq!(b.save("80000002"), b"version one");
        b.engine.arm_failpoint(point);
        let mut b = b;
        let r = b.engine.sync(SyncOptions { dry_run: false }).await;
        assert!(matches!(r, Err(EngineError::Injected(_))), "{point}");
        let mut b = b.reopen();
        assert_eq!(
            b.save("80000002"),
            b"version one",
            "{point}: rolled back on restart"
        );
        let again = b.sync().await;
        assert_eq!(
            again.streams[0].action,
            StreamAction::FastForward,
            "{point}"
        );
        assert_eq!(b.save("80000002"), b"version two!", "{point}");
    }
}

#[tokio::test]
async fn a_crash_around_the_manifest_never_makes_a_conflict() {
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
        a.engine.arm_failpoint(point);
        assert!(
            a.engine.sync(SyncOptions { dry_run: false }).await.is_err(),
            "{point}"
        );
        let mut a = a.reopen();
        let r = a.sync().await;
        let expected = if point == "upload.after_manifest" {
            StreamAction::Adopt
        } else {
            StreamAction::Upload
        };
        assert_eq!(r.streams[0].action, expected, "{point}");
        assert_eq!(
            a.sync().await.streams[0].action,
            StreamAction::Nothing,
            "{point}"
        );
        let history = a.engine.history("Ana", "cemu", "101c9400").await.unwrap();
        assert_eq!(
            history.versions.len(),
            1,
            "{point}: exactly one version, no conflict"
        );
    }
}
