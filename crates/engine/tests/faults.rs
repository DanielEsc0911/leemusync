//! Fail the engine at every journal step and every storage request; on restart or rerun
//! nothing is half old, half new and no false conflict appears (I1, I5, SC5). In-process
//! errors here; real process kills in `kill.rs`.

mod common;

use common::{Pc, SAVE, join, map};
use lemusync_api::StreamAction;
use lemusync_engine::{EngineError, MapChoice, SyncOptions};
use lemusync_store::{Fault, Faults};

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

/// How the network fails at request N: a cut (every request from N fails until healed) or a
/// lost response (request N reaches the backend, its answer doesn't).
#[derive(Clone, Copy, Debug)]
enum Cut {
    From,
    LostResponse,
}

fn plan(cut: Cut, n: u32) -> Faults {
    let f = Faults::default();
    match cut {
        Cut::From => f.cut_from(n),
        Cut::LostResponse => f.fail_nth(n, Fault::TransientAfterWrite),
    }
    f
}

#[allow(clippy::unwrap_used)]
fn write_save(pc: &Pc, account: &str, content: &[u8]) {
    std::fs::write(
        pc.cemu.join(SAVE).join(account).join("0/game_data.sav"),
        content,
    )
    .unwrap();
}

#[allow(clippy::unwrap_used)]
async fn heads(pc: &mut Pc) -> usize {
    let h = pc.engine.history("Ana", "cemu", "101c9400").await.unwrap();
    h.versions.iter().filter(|v| v.head).count()
}

/// SC5: fail request N of one upload sync (on A) and one fast-forward sync (on B), for every N
/// the sync makes. The sync errors or completes; a rerun converges; no mix, no false conflict.
#[tokio::test]
#[allow(clippy::many_single_char_names)] // a, b, k, n, f: the matrix loop
async fn a_network_fault_at_any_request_of_a_sync_never_corrupts_or_conflicts() {
    let root = tempfile::tempdir().unwrap();
    let (mut a, mut b) = two_pcs(root.path()).await;
    b.sync().await;
    let mut prev = b"version two!".to_vec();
    let mut k = 0;
    for upload in [true, false] {
        for cut in [Cut::From, Cut::LostResponse] {
            let mut n = 1;
            loop {
                k += 1;
                let content = format!("version {k}").into_bytes();
                write_save(&a, "80000001", &content);
                if !upload {
                    a.sync().await;
                }
                let (faulty, other, mine, theirs) = if upload {
                    (&mut a, &mut b, "80000001", "80000002")
                } else {
                    (&mut b, &mut a, "80000002", "80000001")
                };
                let f = plan(cut, n);
                faulty.engine.set_store_faults(Some(f.clone()));
                let _ = faulty.engine.sync(SyncOptions { dry_run: false }).await;
                let reached = f.requests() >= n;
                faulty.engine.set_store_faults(None);
                let case = format!("upload={upload} {cut:?} at {n}");
                let now = faulty.save(mine);
                assert!(now == content || now == prev, "{case}: mixed {now:?}");
                let again = faulty.sync().await;
                assert!(
                    !matches!(
                        again.streams[0].action,
                        StreamAction::UploadThenConflict | StreamAction::Conflict
                    ),
                    "{case}: false conflict"
                );
                assert_eq!(faulty.save(mine), content, "{case}");
                other.sync().await;
                assert_eq!(other.save(theirs), content, "{case}");
                assert_eq!(heads(faulty).await, 1, "{case}: no extra heads");
                assert_eq!(faulty.engine.conflicts().await.unwrap().0, vec![], "{case}");
                prev = content;
                if !reached {
                    break;
                }
                n += 1;
            }
            eprintln!("upload={upload} {cut:?}: {n} cases");
            assert!(
                n > 3,
                "upload={upload} {cut:?}: the matrix ran ({n} requests)"
            );
        }
    }
}
