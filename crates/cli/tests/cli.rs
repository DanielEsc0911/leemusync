//! The binary end to end, on commands that don't need the OS keystore.

use std::path::Path;
use std::process::{Command, Output};

fn run(root: &Path, lang: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lemusync"))
        .args(args)
        .env("LEMUSYNC_TEST_ROOT", root)
        .env_remove("LC_ALL")
        .env_remove("LC_MESSAGES")
        .env_remove("LANGUAGE")
        .env("LANG", lang)
        .output()
        .expect("binary runs")
}

#[test]
fn status_json_on_a_fresh_device() {
    let root = tempfile::tempdir().unwrap();
    let out = run(root.path(), "en_US.UTF-8", &["--json", "status"]);
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["api_version"], 1);
    assert!(json["repo"].is_null());
}

#[test]
fn human_output_follows_the_locale() {
    let root = tempfile::tempdir().unwrap();
    let en = String::from_utf8(run(root.path(), "en_US.UTF-8", &["status"]).stdout).unwrap();
    let es = String::from_utf8(run(root.path(), "es_MX.UTF-8", &["status"]).stdout).unwrap();
    assert!(en.contains("No repo yet"), "{en}");
    assert!(es.contains("Todavía no hay un repo"), "{es}");
    let help = String::from_utf8(run(root.path(), "es_MX.UTF-8", &["--help"]).stdout).unwrap();
    assert!(
        help.contains("Mantiene tus partidas guardadas sincronizadas"),
        "{help}"
    );
}

#[test]
fn errors_use_the_spec_exit_codes() {
    let root = tempfile::tempdir().unwrap();
    let out = run(root.path(), "en_US.UTF-8", &["--json", "sync", "--dry-run"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "not set up yet is a usage error"
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["error"]["code"], "not_set_up");
    assert_eq!(
        run(root.path(), "en_US.UTF-8", &["grant", "add", "/"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        run(root.path(), "en_US.UTF-8", &["no-such-command"])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn backend_fs_and_grants_persist() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("remote");
    std::fs::create_dir_all(root.path().join("home/.local/share/Cemu")).unwrap();
    assert!(
        run(
            root.path(),
            "en_US.UTF-8",
            &["backend", "add", "fs", store.to_str().unwrap()]
        )
        .status
        .success()
    );
    let grant = root.path().join("home/.local/share/Cemu");
    assert!(
        run(
            root.path(),
            "en_US.UTF-8",
            &["grant", "add", grant.to_str().unwrap()]
        )
        .status
        .success()
    );
    let out = run(root.path(), "en_US.UTF-8", &["--json", "status"]);
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(json["backend"].as_str().unwrap().starts_with("fs "));
    assert_eq!(json["grants"].as_array().unwrap().len(), 1);
}

#[test]
fn scan_output_is_fully_formatted_in_both_languages() {
    for (lang, word) in [("en_US.UTF-8", "Allow it"), ("es_MX.UTF-8", "Permítelo")] {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("home/.local/share/Cemu")).unwrap();
        let out = String::from_utf8(run(root.path(), lang, &["scan"]).stdout).unwrap();
        assert!(out.contains(word), "{lang}: {out}");
        assert!(
            !out.contains("{ $") && !out.contains("scan-"),
            "{lang}: unformatted text: {out}"
        );
    }
}

#[test]
fn backend_s3_asks_for_the_secret_at_a_prompt_only() {
    let root = tempfile::tempdir().unwrap();
    let help = String::from_utf8(
        run(
            root.path(),
            "es_MX.UTF-8",
            &["backend", "add", "s3", "--help"],
        )
        .stdout,
    )
    .unwrap();
    assert!(help.contains("llavero del sistema"), "{help}");
    assert!(
        !help.contains("--secret"),
        "no flag takes the secret: {help}"
    );
    let args = [
        "backend",
        "add",
        "s3",
        "--endpoint",
        "https://acct.r2.cloudflarestorage.com",
        "--bucket",
        "saves",
        "--access-key-id",
        "id",
    ];
    let out = run(root.path(), "en_US.UTF-8", &args);
    assert_eq!(out.status.code(), Some(2), "no terminal, no secret prompt");
    let status = run(root.path(), "en_US.UTF-8", &["--json", "status"]);
    let json: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert!(
        json["backend"].is_null(),
        "nothing saved without the secret"
    );
}

#[test]
fn play_commands_fail_with_the_spec_exit_codes() {
    let root = tempfile::tempdir().unwrap();
    let out = run(
        root.path(),
        "en_US.UTF-8",
        &["--json", "resolve", "101c9400", "not-a-version"],
    );
    assert_eq!(out.status.code(), Some(2), "a bad version is a usage error");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["error"]["code"], "usage");
    let en = run(root.path(), "en_US.UTF-8", &["resolve", "101c9400", "nope"]);
    let es = run(root.path(), "es_MX.UTF-8", &["resolve", "101c9400", "nope"]);
    assert!(
        String::from_utf8_lossy(&en.stderr).contains("nope isn't a version id"),
        "{en:?}"
    );
    assert!(
        String::from_utf8_lossy(&es.stderr).contains("nope no es un id de versión"),
        "{es:?}"
    );
    // Without a terminal and without --yes nothing is created.
    let out = run(
        root.path(),
        "en_US.UTF-8",
        &["accounts", "create", "P", "cemu"],
    );
    assert_eq!(out.status.code(), Some(2));
    // `--force` is required to release a lease.
    let out = run(
        root.path(),
        "en_US.UTF-8",
        &["lease", "release", "101c9400"],
    );
    assert_eq!(out.status.code(), Some(2));
    for args in [
        &["--json", "conflicts"][..],
        &["--json", "lease", "status"],
        &["--json", "play", "cemu", "101c9400"],
    ] {
        let out = run(root.path(), "en_US.UTF-8", args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: no repo yet");
    }
}

fn config_json(root: &Path, args: &[&str]) -> serde_json::Value {
    let mut all = vec!["--json", "config"];
    all.extend_from_slice(args);
    let out = run(root, "en_US.UTF-8", &all);
    assert!(out.status.success(), "{args:?}: {out:?}");
    serde_json::from_slice(&out.stdout).expect("config --json prints JSON")
}

fn config_ok(root: &Path, args: &[&str]) {
    let mut all = vec!["config"];
    all.extend_from_slice(args);
    let out = run(root, "en_US.UTF-8", &all);
    assert!(out.status.success(), "{args:?}: {out:?}");
}

#[test]
fn config_set_get_unset_round_trips_every_key_family() {
    let root = tempfile::tempdir().unwrap();
    let r = root.path();
    let get = |key: &str| config_json(r, &["get", key])["value"].clone();
    assert_eq!(get("conflict_policy"), "ask");
    config_ok(r, &["set", "conflict_policy", "newest"]);
    assert_eq!(get("conflict_policy"), "newest");
    config_ok(r, &["unset", "conflict_policy"]);
    assert_eq!(get("conflict_policy"), "ask");

    config_ok(r, &["set", "leases.ttl_secs", "600"]);
    assert_eq!(get("leases.ttl_secs"), 600);
    config_ok(r, &["unset", "leases.ttl_secs"]);
    assert_eq!(get("leases.ttl_secs"), 300);

    // Paths must be absolute on the host OS: "/opt/cemu" has no drive on
    // Windows, so build them from the temp dir.
    let cemu_root = r.join("opt").join("cemu");
    let cemu_root = cemu_root.to_str().unwrap();
    let zelda = r.join("games").join("zelda.wux");
    let zelda = zelda.to_str().unwrap();
    config_ok(r, &["set", "emulators.cemu.root", cemu_root]);
    assert_eq!(get("emulators.cemu.root"), cemu_root);
    config_ok(
        r,
        &[
            "set",
            "emulators.cemu.exe",
            "flatpak",
            "run",
            "info.cemu.Cemu",
        ],
    );
    assert_eq!(
        get("emulators.cemu.exe"),
        serde_json::json!(["flatpak", "run", "info.cemu.Cemu"])
    );
    config_ok(r, &["set", "emulators.cemu.games.101C9500", zelda]);
    assert_eq!(get("emulators.cemu.games.101c9500"), zelda);
    let dirs = [r.join("games"), r.join("more")];
    let dirs = dirs.map(|d| d.to_str().unwrap().to_owned());
    config_ok(r, &["set", "emulators.cemu.game_dirs", &dirs[0], &dirs[1]]);
    assert_eq!(get("emulators.cemu.game_dirs"), serde_json::json!(dirs));
    assert_eq!(get("emulators.cemu")["root"], cemu_root);
    assert_eq!(get("emulators.cemu.games")["101c9500"], zelda);
    assert_eq!(get("backend"), serde_json::Value::Null);
    let list = config_json(r, &["list"]);
    assert_eq!(list["api_version"], 1);
    assert_eq!(list["values"]["emulators.cemu.exe"][0], "flatpak");
    assert_eq!(list["values"]["conflict_policy"], "ask");
    assert!(list["values"]["device.id"].is_string(), "{list}");
    for key in [
        "emulators.cemu.game_dirs",
        "emulators.cemu.games.101c9500",
        "emulators.cemu.exe",
        "emulators.cemu.root",
    ] {
        config_ok(r, &["unset", key]);
        assert!(get(key).is_null(), "{key}");
    }
    let path = config_json(r, &["path"])["path"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(path.ends_with("config.toml"), "{path}");
    assert!(Path::new(&path).starts_with(r));
}

#[test]
fn config_set_keeps_comments_and_refuses_bad_edits_untouched() {
    let root = tempfile::tempdir().unwrap();
    let r = root.path();
    let path = config_json(r, &["path"])["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("# my note\n{text}")).unwrap();
    config_ok(r, &["set", "conflict_policy", "newest"]);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# my note\n"), "{text}");
    assert!(text.contains("conflict_policy = \"newest\""), "{text}");

    let before = std::fs::read(&path).unwrap();
    for (args, what) in [
        (&["set", "conflict_policy", "merge"][..], "bad enum"),
        (&["set", "leases.ttl_secs", "5"], "ttl out of bounds"),
        (&["set", "leases.ttl_secs", "abc"], "ttl not a number"),
        (
            &["set", "emulators.cemu.root", "relative/dir"],
            "relative path",
        ),
        (
            &["set", "emulators.cemu.games.101c9500", "x.wux"],
            "relative game",
        ),
        (
            &["set", "emulators.nope.root", "/opt/x"],
            "unknown emulator",
        ),
        (&["set", "grants", "/"], "managed key"),
        (&["unset", "backend.kind"], "managed key"),
        (&["set", "device.id", "x"], "fixed key"),
        (&["set", "no.such.key", "x"], "unknown key"),
        (&["set", "conflict_policy", "ask", "newest"], "two values"),
    ] {
        let mut all = vec!["--json", "config"];
        all.extend_from_slice(args);
        let out = run(r, "en_US.UTF-8", &all);
        assert_eq!(out.status.code(), Some(2), "{what}: {out:?}");
        assert_eq!(std::fs::read(&path).unwrap(), before, "{what}");
    }
    let es = run(r, "es_MX.UTF-8", &["config", "set", "grants", "/"]);
    assert!(
        String::from_utf8_lossy(&es.stderr).contains("lemusync grant add"),
        "{es:?}"
    );
}

/// A native Cemu install under `root` whose granted settings.xml lists one games folder,
/// holding one synthetic unpacked game.
fn cemu_with_a_game(root: &Path) -> std::path::PathBuf {
    let home = root.join("home");
    std::fs::create_dir_all(home.join(".local/share/Cemu")).expect("fixture written");
    let games = root.join("games");
    let game = games.join("Synthetic");
    std::fs::create_dir_all(game.join("meta")).expect("fixture written");
    std::fs::create_dir_all(game.join("code")).expect("fixture written");
    std::fs::write(
        game.join("meta/meta.xml"),
        "<menu><title_id>000500001FFFFF00</title_id><longname_en>Synthetic Game</longname_en></menu>",
    )
    .expect("fixture written");
    std::fs::write(game.join("code/s.rpx"), b"").expect("fixture written");
    let cfg = home.join(".config/Cemu");
    std::fs::create_dir_all(&cfg).expect("fixture written");
    std::fs::write(
        cfg.join("settings.xml"),
        format!(
            "<content><GamePaths><Entry>{}</Entry></GamePaths></content>",
            games.display()
        ),
    )
    .expect("fixture written");
    let out = run(
        root,
        "en_US.UTF-8",
        &["grant", "add", cfg.to_str().expect("utf-8 temp path")],
    );
    assert!(out.status.success(), "{out:?}");
    game
}

#[test]
fn games_lists_detected_games_offline_in_both_languages() {
    let root = tempfile::tempdir().unwrap();
    let r = root.path();
    let out = run(r, "en_US.UTF-8", &["games"]);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("No games found"));
    let folder = cemu_with_a_game(r);
    let out = run(r, "en_US.UTF-8", &["--json", "games"]);
    assert!(out.status.success(), "no repo or storage needed: {out:?}");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["api_version"], 1);
    let g = &json["games"][0];
    assert_eq!(g["emulator"], "cemu");
    assert_eq!(g["key"], "1fffff00");
    assert_eq!(g["name"], "Synthetic Game");
    assert_eq!(g["path"], folder.display().to_string());
    assert_eq!(g["source"], "emulator");
    let en = run(r, "en_US.UTF-8", &["games"]);
    let en = String::from_utf8_lossy(&en.stdout);
    assert!(
        en.contains("cemu · Synthetic Game · 1fffff00") && en.contains("from the emulator"),
        "{en}"
    );
    let es = run(r, "es_MX.UTF-8", &["games"]);
    assert!(String::from_utf8_lossy(&es.stdout).contains("del emulador"));
    // The user's path wins. It must be absolute on the host OS.
    let elsewhere = r.join("elsewhere").join("game.wux");
    let elsewhere = elsewhere.to_str().unwrap();
    config_ok(r, &["set", "emulators.cemu.games.1FFFFF00", elsewhere]);
    let json = run(r, "en_US.UTF-8", &["--json", "games"]);
    let json: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(json["games"][0]["path"], elsewhere);
    assert_eq!(json["games"][0]["source"], "config");
    assert_eq!(json["games"][0]["name"], "Synthetic Game");
}
