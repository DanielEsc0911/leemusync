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
