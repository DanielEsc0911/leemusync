//! `accounts create`: a new Cemu account from profile data, mapped and restored (Task 6, D32).
#![allow(clippy::unwrap_used)] // test helpers: a failure here is a failed test

mod common;

use std::collections::BTreeSet;
use std::path::Path;

use common::{Pc, SAVE, join, map};
use leemusync_engine::{EngineError, MapChoice};

const ACT: &str = "mlc01/usr/save/system/act";

fn tree(root: &Path) -> BTreeSet<String> {
    walkdir(root, root)
}

fn walkdir(root: &Path, dir: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walkdir(root, &p));
        } else {
            out.insert(p.strip_prefix(root).unwrap().display().to_string());
        }
    }
    out
}

/// Fedora uploads Ana's save; `CachyOS` (only account `80000001`) creates an account for Ana.
async fn two_pcs(root: &Path) -> (Pc, Pc) {
    let remote = root.join("remote");
    let mut fedora = Pc::new(&root.join("fedora"), true, b"Ana's BotW");
    let mut cachy = Pc::new(&root.join("cachy"), false, b"Cachy's BotW");
    fedora.engine.backend_add_fs(&remote).unwrap();
    fedora.engine.init("pass phrase").await.unwrap();
    fedora.engine.players_add("Ana").await.unwrap();
    map(&mut fedora, "Ana", "80000001", MapChoice::Ask).await;
    fedora.sync().await;
    join(&mut cachy, &remote).await;
    cachy.engine.confirm_first_sync().unwrap();
    (fedora, cachy)
}

#[tokio::test]
async fn creates_maps_and_restores() {
    let root = tempfile::tempdir().unwrap();
    let (_fedora, mut cachy) = two_pcs(root.path()).await;
    let act = cachy.cemu.join(ACT);
    std::fs::write(act.join("persisid.dat"), "PersistentIdHead=80000004\n").unwrap();
    let before = tree(&cachy.cemu);

    let created = cachy
        .engine
        .create_account("Ana", "cemu", None)
        .await
        .unwrap();
    assert_eq!(created.account, "80000005");
    assert_eq!(created.name, "Ana");

    let after = tree(&cachy.cemu);
    let new: Vec<&String> = after.difference(&before).collect();
    let restored = format!("{SAVE}/80000005/");
    assert!(after.is_superset(&before), "nothing removed");
    assert!(new.contains(&&format!("{ACT}/80000005/account.dat")));
    assert!(
        new.iter()
            .all(|p| *p == &format!("{ACT}/80000005/account.dat") || p.starts_with(&restored)),
        "{new:?}"
    );
    assert_eq!(
        std::fs::read_to_string(act.join("persisid.dat")).unwrap(),
        "PersistentIdHead=80000004\n",
        "persisid.dat is never written"
    );
    let text = std::fs::read_to_string(act.join("80000005/account.dat")).unwrap();
    assert!(text.starts_with("AccountInstance_20120705\nPersistentId=80000005\n"));
    assert_eq!(cachy.save("80000005"), b"Ana's BotW");
    assert_eq!(cachy.save("80000001"), b"Cachy's BotW");
    let scan = cachy.engine.scan().await.unwrap();
    let acc = scan.installs[0]
        .accounts
        .iter()
        .find(|a| a.id == "80000005")
        .unwrap();
    assert_eq!(acc.name.as_deref(), Some("Ana"));
    assert_eq!(acc.player.as_deref(), Some("Ana"));
}

#[tokio::test]
async fn refuses_while_cemu_runs() {
    let root = tempfile::tempdir().unwrap();
    let (_fedora, mut cachy) = two_pcs(root.path()).await;
    let before = tree(&cachy.cemu);
    cachy.probe.set(true);
    let err = cachy
        .engine
        .create_account("Ana", "cemu", Some("Ana"))
        .await
        .unwrap_err();
    assert!(matches!(err, EngineError::EmulatorRunning(_)), "{err}");
    assert_eq!(tree(&cachy.cemu), before);
}

#[tokio::test]
async fn refuses_when_the_folder_exists() {
    let root = tempfile::tempdir().unwrap();
    let (_fedora, mut cachy) = two_pcs(root.path()).await;
    let act = cachy.cemu.join(ACT);
    let outside = root.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    // The next id is 80000002. A file or a symlink with that name isn't an account, but the name
    // is taken: never write over it or through it.
    std::fs::write(act.join("80000002"), b"not an account").unwrap();
    let before = tree(&cachy.cemu);
    let err = cachy
        .engine
        .create_account("Ana", "cemu", None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, EngineError::Usage(ref k) if k.starts_with("account_exists")),
        "{err}"
    );
    assert_eq!(tree(&cachy.cemu), before);
    #[cfg(unix)]
    {
        std::fs::remove_file(act.join("80000002")).unwrap();
        std::os::unix::fs::symlink(&outside, act.join("80000002")).unwrap();
        let err = cachy
            .engine
            .create_account("Ana", "cemu", None)
            .await
            .unwrap_err();
        assert!(
            matches!(err, EngineError::Usage(ref k) if k.starts_with("account_exists")),
            "{err}"
        );
        assert!(tree(&outside).is_empty(), "never writes through a link");
    }
}
