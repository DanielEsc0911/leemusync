//! The binary end to end, on commands that don't need the OS keystore.

use std::path::Path;
use std::process::{Command, Output};

fn run(root: &Path, lang: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_leemusync"))
        .args(args)
        .env("LEEMUSYNC_TEST_ROOT", root)
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
