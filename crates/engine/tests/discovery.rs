//! Cemu discovery against profiles/fixtures (fake data only).

mod common;

use std::path::{Path, PathBuf};

use common::{FIXTURE, env, install_fixture};
use leemusync_engine::Engine;

#[tokio::test]
async fn flatpak_fixture_matches_expected_toml() {
    let root = tempfile::tempdir().unwrap();
    install_fixture(root.path(), true);
    let mut engine = Engine::open(env(root.path())).unwrap();
    let before = engine.scan().await.unwrap();
    assert!(
        !before.installs[0].granted,
        "nothing is read before a grant"
    );
    assert_eq!(before.installs[0].accounts.len(), 0);
    engine
        .grant_add(&root.path().join("home/.var/app/info.cemu.Cemu"))
        .unwrap();
    let report = engine.scan().await.unwrap();
    let cemu = &report.installs[0];
    assert!(cemu.granted && cemu.needs_grant.is_empty());

    let expected: toml::Value =
        toml::from_str(&std::fs::read_to_string(Path::new(FIXTURE).join("expected.toml")).unwrap())
            .unwrap();
    let mut want: Vec<(String, String)> = expected["account"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["id"].as_str().unwrap().to_owned(),
                a["name"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    want.sort();
    let got: Vec<(String, String)> = cemu
        .accounts
        .iter()
        .map(|a| (a.id.clone(), a.name.clone().unwrap_or_default()))
        .collect();
    assert_eq!(got, want);
    let mut streams: Vec<(String, String)> = expected["stream"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["title_id"].as_str().unwrap().to_owned(),
                s["account"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    streams.sort();
    let saves: Vec<(String, String)> = cemu
        .saves
        .iter()
        .map(|s| (s.game.clone(), s.account.clone()))
        .collect();
    assert_eq!(saves, streams);
    assert!(cemu.saves.iter().all(|s| !s.orphaned));
}

#[tokio::test]
async fn native_install_with_a_deleted_account_shows_an_orphan() {
    let root = tempfile::tempdir().unwrap();
    let data = install_fixture(root.path(), false);
    std::fs::remove_dir_all(data.join("mlc01/usr/save/system/act/80000002")).unwrap();
    let mut engine = Engine::open(env(root.path())).unwrap();
    engine.grant_add(&data).unwrap();
    let report = engine.scan().await.unwrap();
    let cemu = &report.installs[0];
    assert_eq!(
        cemu.needs_grant.len(),
        1,
        "settings.xml in ~/.config/Cemu isn't granted yet"
    );
    let orphan = cemu.saves.iter().find(|s| s.account == "80000002").unwrap();
    assert!(orphan.orphaned);
    assert_eq!(cemu.accounts.len(), 1);
    engine
        .grant_add(&root.path().join("home/.config/Cemu"))
        .unwrap();
    assert_eq!(
        engine.scan().await.unwrap().installs[0].needs_grant.len(),
        0
    );
}

#[tokio::test]
async fn grants_refuse_home_and_root() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("home")).unwrap();
    let mut engine = Engine::open(env(root.path())).unwrap();
    assert!(engine.grant_add(Path::new("/")).is_err());
    assert!(engine.grant_add(&root.path().join("home")).is_err());
    assert!(engine.grant_add(&root.path().join("missing")).is_err());
}

#[tokio::test]
async fn a_mlc_outside_the_base_asks_for_the_mlc_not_the_base() {
    let root = tempfile::tempdir().unwrap();
    let data = install_fixture(root.path(), false);
    let config = root.path().join("home/.config/Cemu");
    let mlc = root.path().join("games/mlc");
    std::fs::create_dir_all(mlc.join("usr/save")).unwrap();
    let xml = format!("<content><mlc_path>{}</mlc_path></content>", mlc.display());
    std::fs::write(config.join("settings.xml"), xml).unwrap();
    let mut engine = Engine::open(env(root.path())).unwrap();
    engine.grant_add(&data).unwrap();
    engine.grant_add(&config).unwrap();
    let report = engine.scan().await.unwrap();
    let cemu = &report.installs[0];
    assert!(!cemu.granted);
    // Compared as paths: Windows reports `mlc/usr/save` with mixed separators.
    let needs: Vec<_> = cemu.needs_grant.iter().map(PathBuf::from).collect();
    assert_eq!(needs, [mlc.join("usr/save")]);
    engine.grant_add(&mlc).unwrap();
    let report = engine.scan().await.unwrap();
    assert!(report.installs[0].granted);
    assert_eq!(report.installs[0].needs_grant.len(), 0);
}

#[cfg(unix)]
#[tokio::test]
async fn an_unreadable_save_folder_is_a_warning_not_zero_files() {
    let root = tempfile::tempdir().unwrap();
    let data = install_fixture(root.path(), true);
    let save = data.join("mlc01/usr/save/00050000/101c9400/user/80000001");
    std::os::unix::fs::symlink(root.path(), save.join("link")).unwrap();
    let mut engine = Engine::open(env(root.path())).unwrap();
    engine
        .grant_add(&root.path().join("home/.var/app/info.cemu.Cemu"))
        .unwrap();
    let report = engine.scan().await.unwrap();
    assert!(report.warnings.iter().any(|w| {
        w.code == leemusync_api::WarningCode::UnreadableFolder && w.detail.ends_with("80000001")
    }));
}
