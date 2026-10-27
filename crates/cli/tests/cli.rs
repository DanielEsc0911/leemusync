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

    config_ok(r, &["set", "emulators.cemu.root", "/opt/cemu"]);
    assert_eq!(get("emulators.cemu.root"), "/opt/cemu");
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
    config_ok(
        r,
        &["set", "emulators.cemu.games.101C9500", "/games/zelda.wux"],
    );
    assert_eq!(get("emulators.cemu.games.101c9500"), "/games/zelda.wux");
    let list = config_json(r, &["list"]);
    assert_eq!(list["api_version"], 1);
    assert_eq!(list["values"]["emulators.cemu.exe"][0], "flatpak");
    assert_eq!(list["values"]["conflict_policy"], "ask");
    assert!(list["values"]["device.id"].is_string(), "{list}");
    for key in [
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
