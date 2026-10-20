//! The M1a test day on two simulated PCs: Fedora (Flatpak Cemu) and `CachyOS` (native Cemu).

mod common;

use common::{Pc, SAVE, join, map};
use leemusync_api::StreamAction;
use leemusync_engine::{MapChoice, MapOutcome};

#[tokio::test]
#[allow(clippy::too_many_lines)] // one test mirrors the whole runbook, step for step
async fn both_saves_end_up_on_both_pcs() {
    let root = tempfile::tempdir().unwrap();
    let remote = root.path().join("remote");
    let mut fedora = Pc::new(&root.path().join("fedora"), true, b"Fedora's BotW");
    let mut cachy = Pc::new(&root.path().join("cachy"), false, b"CachyOS's BotW");
    // CachyOS has an orphaned folder left by a deleted account (seen on the real laptop).
    let orphan = cachy.cemu.join(SAVE).join("80000002");
    std::fs::create_dir_all(orphan.join("0")).unwrap();
    std::fs::write(orphan.join("0/game_data.sav"), b"old orphan").unwrap();

    // Runbook step 3: CachyOS creates the repo.
    cachy.engine.backend_add_fs(&remote).unwrap();
    let (_, recovery) = cachy.engine.init("pass phrase").await.unwrap();
    assert!(!recovery.is_empty());
    cachy.engine.players_add("Cachy").await.unwrap();
    cachy.engine.players_add("Fedora").await.unwrap();
    let scan = cachy.engine.scan().await.unwrap();
    assert!(
        scan.installs[0]
            .saves
            .iter()
            .any(|s| s.account == "80000002" && s.orphaned)
    );
    assert!(matches!(
        map(&mut cachy, "Cachy", "80000001", MapChoice::Ask).await,
        MapOutcome::Mapped
    ));

    // Step 4: Fedora joins.
    join(&mut fedora, &remote).await;
    map(&mut fedora, "Fedora", "80000001", MapChoice::Ask).await;

    // Step 5: first pushes.
    assert_eq!(cachy.sync().await.streams[0].action, StreamAction::Upload);
    let r = fedora.sync().await;
    let action_of = |r: &leemusync_api::SyncReport, p: &str| {
        r.streams.iter().find(|s| s.player == p).map(|s| s.action)
    };
    assert_eq!(action_of(&r, "Fedora"), Some(StreamAction::Upload));
    assert_eq!(action_of(&r, "Cachy"), Some(StreamAction::NeedsAccount));

    // Step 6, Fedora: choice b (the account is created in Cemu by hand).
    fedora.create_account("80000002");
    map(&mut fedora, "Cachy", "80000002", MapChoice::Ask).await;
    assert_eq!(
        action_of(&fedora.sync().await, "Cachy"),
        Some(StreamAction::FastForward)
    );

    // Step 6, CachyOS: keep the orphan as its own player first, then choice a with Replace.
    cachy.engine.players_add("Old CachyOS").await.unwrap();
    cachy
        .engine
        .accounts_recover("cemu", "80000002", "Old CachyOS")
        .await
        .unwrap();
    cachy.create_account("80000002");
    match map(&mut cachy, "Fedora", "80000002", MapChoice::Ask).await {
        MapOutcome::NeedsChoice { games } => assert_eq!(games, vec!["101c9400".to_string()]),
        MapOutcome::Mapped => panic!("the folder has files, so the CLI must ask keep or replace"),
    }
    map(&mut cachy, "Fedora", "80000002", MapChoice::Replace).await;
    assert_eq!(
        action_of(&cachy.sync().await, "Fedora"),
        Some(StreamAction::FastForward)
    );

    // Step 7: both saves on both PCs, byte for byte.
    assert_eq!(fedora.save("80000001"), b"Fedora's BotW");
    assert_eq!(fedora.save("80000002"), b"CachyOS's BotW");
    assert_eq!(cachy.save("80000001"), b"CachyOS's BotW");
    assert_eq!(cachy.save("80000002"), b"Fedora's BotW");
    for pc in [&mut fedora, &mut cachy] {
        for s in pc.sync().await.streams {
            let ok = s.action == StreamAction::Nothing
                || (s.player == "Old CachyOS" && s.action == StreamAction::NeedsAccount);
            assert!(ok, "{} {:?}", s.player, s.action);
        }
    }
    let old = cachy
        .engine
        .history("Old CachyOS", "cemu", "101c9400")
        .await
        .unwrap();
    assert_eq!(
        old.versions.len(),
        1,
        "the orphan's save is kept in the repo"
    );
    let fedora_backups = cachy
        .engine
        .history("Fedora", "cemu", "101c9400")
        .await
        .unwrap()
        .backups;
    assert!(
        fedora_backups.iter().any(|b| b.ends_with("-pre-restore")),
        "the replaced files went to local history"
    );

    // A forced double edit becomes a conflict with nothing lost (SC3 preview).
    std::fs::write(
        fedora.cemu.join(SAVE).join("80000002/0/game_data.sav"),
        b"edited on Fedora",
    )
    .unwrap();
    std::fs::write(
        cachy.cemu.join(SAVE).join("80000001/0/game_data.sav"),
        b"edited on CachyOS",
    )
    .unwrap();
    assert_eq!(
        action_of(&fedora.sync().await, "Cachy"),
        Some(StreamAction::Upload)
    );
    assert_eq!(
        action_of(&cachy.sync().await, "Cachy"),
        Some(StreamAction::UploadThenConflict)
    );
    let h = cachy
        .engine
        .history("Cachy", "cemu", "101c9400")
        .await
        .unwrap();
    assert_eq!(h.versions.iter().filter(|v| v.head).count(), 2);
    assert_eq!(
        cachy.save("80000001"),
        b"edited on CachyOS",
        "the local copy is kept while the conflict is open"
    );
}
