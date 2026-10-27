//! Game references and aliases (config-and-cli §Game references). Synthetic data only.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

mod common;

use common::{Pc, join, map};
use lemusync_engine::{EmulatorOverride, EngineError, Locations, MapChoice};

const GAME: &str = "101c9400";

/// Unpacked games `(title id, name)` in `<root>/games/<n>`, found through `game_dirs`.
fn install_games(pc: Pc, games: &[(&str, &str)]) -> Pc {
    let dir = pc.root.join("games");
    for (n, (title, name)) in games.iter().enumerate() {
        let g = dir.join(n.to_string());
        std::fs::create_dir_all(g.join("meta")).unwrap();
        std::fs::create_dir_all(g.join("code")).unwrap();
        std::fs::write(
            g.join("meta/meta.xml"),
            format!("<menu><title_id>{title}</title_id><longname_en>{name}</longname_en></menu>"),
        )
        .unwrap();
    }
    let path = Locations::under(&pc.root).config_dir.join("config.toml");
    let mut c = pc.engine.local_config().clone();
    c.emulators.insert(
        "cemu".into(),
        EmulatorOverride {
            game_dirs: vec![dir],
            ..EmulatorOverride::default()
        },
    );
    c.save(&path).unwrap();
    pc.reopen()
}

fn usage(r: Result<impl std::fmt::Debug, EngineError>) -> String {
    match r {
        Err(EngineError::Usage(s)) => s,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn aliases_keys_and_names_find_the_same_game() {
    let root = tempfile::tempdir().unwrap();
    let remote = root.path().join("remote");
    let mut a = Pc::new(&root.path().join("a"), false, b"save");
    a.engine.backend_add_fs(&remote).unwrap();
    a.engine.init("pass phrase").await.unwrap();
    a.engine.players_add("P").await.unwrap();
    map(&mut a, "P", "80000001", MapChoice::Ask).await;
    a.sync().await;
    let mut b = Pc::new(&root.path().join("b"), false, b"save");
    join(&mut b, &remote).await;

    let mut a = install_games(
        a,
        &[
            (
                "00050000101C9400",
                "The Legend of Zelda: Breath of the Wild",
            ),
            ("000500001FFFFF00", "Twin (USA)"),
            ("000500001FFFFF01", "Twin (EUR)"),
        ],
    );
    let want = ("cemu".to_owned(), GAME.to_owned());
    for q in [
        "101C9400",
        "the legend of zelda breath of the wild",
        "Zelda: Breath",
    ] {
        let got = a.engine.resolve_game(q, None).await;
        if q == "Zelda: Breath" {
            assert_eq!(usage(got), "game_unknown:Zelda: Breath");
        } else {
            assert_eq!(got.unwrap(), want, "{q}");
        }
    }
    assert!(
        usage(a.engine.resolve_game("twin", None).await)
            .starts_with("game_ambiguous:Twin (USA) (cemu/1fffff00); Twin (EUR) (cemu/1fffff01)")
    );
    assert_eq!(
        usage(a.engine.resolve_game("botw", Some("dolphin")).await),
        "game_unknown:botw"
    );

    // `config set` stores an alias for a known game, never another game's key or a taken alias.
    a.engine
        .config_set("aliases.botw", &["cemu/101c9400".into()])
        .unwrap();
    assert_eq!(a.engine.resolve_game("botw", None).await.unwrap(), want);
    // Both ways accept any game key of a bundled emulator, installed or not.
    a.engine
        .config_set("aliases.x", &["cemu/deadbeef".into()])
        .unwrap();
    assert_eq!(
        a.engine
            .alias_add("cemu/0badf00d", None, "y")
            .await
            .unwrap(),
        ("cemu".into(), "0badf00d".into())
    );
    assert_eq!(
        a.engine
            .alias_add("0BADF00E", Some("cemu"), "w")
            .await
            .unwrap(),
        ("cemu".into(), "0badf00e".into())
    );
    assert_eq!(
        usage(a.engine.alias_add("nope/0badf00d", None, "v").await),
        "config_unknown_emulator:nope"
    );
    assert_eq!(
        usage(a.engine.alias_add("0badf00f", None, "v").await),
        "game_unknown:0badf00f"
    );
    a.engine.alias_remove("y").unwrap();
    a.engine.alias_remove("w").unwrap();
    assert_eq!(
        a.engine.alias_add("x", None, "y").await.unwrap(),
        ("cemu".into(), "deadbeef".into())
    );
    for bad in ["nope/deadbeef", "Cemu/deadbeef"] {
        assert!(
            a.engine.config_set("aliases.z", &[bad.into()]).is_err(),
            "{bad}"
        );
    }
    assert_eq!(
        usage(a.engine.config_set("aliases.z", &["nope/deadbeef".into()])),
        "config_unknown_emulator:nope"
    );
    a.engine.alias_remove("x").unwrap();
    a.engine.alias_remove("y").unwrap();
    assert_eq!(
        usage(
            a.engine
                .config_set("aliases.1fffff00", &["cemu/101c9400".into()])
        ),
        "alias_is_key:1fffff00"
    );
    assert_eq!(
        usage(
            a.engine
                .config_set("aliases.botw", &["cemu/1fffff00".into()])
        ),
        "alias_taken:botw"
    );
    assert_eq!(
        usage(
            a.engine
                .config_set("aliases.Bad", &["cemu/101c9400".into()])
        ),
        "alias_invalid:Bad"
    );
    // `games alias` resolves, refuses an ambiguous game and stores the key, not the query.
    assert!(usage(a.engine.alias_add("twin", None, "tw").await).starts_with("game_ambiguous:"));
    assert_eq!(
        a.engine.alias_add("1FFFFF01", None, "tw").await.unwrap(),
        ("cemu".into(), "1fffff01".into())
    );
    assert_eq!(a.engine.local_config().aliases["tw"], "cemu/1fffff01");
    a.engine.alias_remove("tw").unwrap();
    assert!(!a.engine.local_config().aliases.contains_key("tw"));
    assert_eq!(usage(a.engine.alias_remove("tw")), "alias_unknown:tw");
    // An alias works as the game key of `emulators.<id>.games`.
    a.engine
        .config_set(
            "emulators.cemu.games.botw",
            // Host-absolute: "/g/botw.wux" has no drive on Windows.
            &[std::env::temp_dir()
                .join("g")
                .join("botw.wux")
                .to_str()
                .unwrap()
                .to_owned()],
        )
        .unwrap();
    assert!(
        a.engine.local_config().emulators["cemu"]
            .games
            .contains_key(GAME)
    );
    // The file keeps the key: dropping or retargeting the alias doesn't change that entry.
    a.engine.alias_remove("botw").unwrap();
    a.engine
        .config_set("aliases.botw", &["cemu/1fffff01".into()])
        .unwrap();
    let games = &a.engine.local_config().emulators["cemu"].games;
    assert!(games.contains_key(GAME) && !games.contains_key("botw"));
    a.engine.alias_remove("botw").unwrap();
    a.engine
        .config_set("aliases.botw", &["cemu/101c9400".into()])
        .unwrap();
    let report = a.engine.games_report().unwrap();
    let g = report.games.iter().find(|g| g.key == GAME).unwrap();
    assert_eq!(g.aliases, vec!["botw".to_owned()]);

    // Device b hasn't installed the game: the repo still knows its key, not its name.
    assert_eq!(b.engine.resolve_game(GAME, None).await.unwrap(), want);
    assert_eq!(
        usage(
            b.engine
                .resolve_game("the legend of zelda breath of the wild", None)
                .await
        ),
        "game_unknown:the legend of zelda breath of the wild"
    );
    assert_eq!(b.engine.alias_add(GAME, None, "z").await.unwrap(), want);
}
