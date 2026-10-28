//! `lemusync service` end to end with the recording runner (never the real user manager).
#![cfg(target_os = "linux")]

use std::path::Path;
use std::process::{Command, Output};

fn run(root: &Path, lang: &str, daemon: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lemusync"))
        .args(args)
        .env("LEMUSYNC_TEST_ROOT", root)
        .env("LEMUSYNC_TEST_DAEMON", daemon)
        .env_remove("LC_ALL")
        .env_remove("LC_MESSAGES")
        .env_remove("LANGUAGE")
        .env("LANG", lang)
        .output()
        .expect("binary runs")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn install_status_uninstall_recorded() {
    let root = tempfile::tempdir().unwrap();
    let daemon = root.path().join("lemusyncd");
    std::fs::write(&daemon, "").unwrap();
    let out = run(
        root.path(),
        "en_US.UTF-8",
        &daemon,
        &["service", "install", "--headless"],
    );
    assert!(out.status.success(), "{out:?}");
    assert!(
        stdout(&out).contains("installed and running"),
        "{}",
        stdout(&out)
    );
    let unit =
        std::fs::read_to_string(root.path().join("config/systemd/user/lemusyncd.service")).unwrap();
    assert!(
        unit.contains(&format!("ExecStart=\"{}\" --headless\n", daemon.display())),
        "{unit}"
    );

    std::fs::write(
        root.path().join("systemctl-show.txt"),
        "LoadState=loaded\nUnitFileState=enabled\nActiveState=active\nNRestarts=1\n",
    )
    .unwrap();
    let st = run(
        root.path(),
        "en_US.UTF-8",
        &daemon,
        &["--json", "service", "status"],
    );
    let json: serde_json::Value = serde_json::from_slice(&st.stdout).unwrap();
    assert_eq!(json["installed"], true);
    assert_eq!(json["active"], true);
    assert_eq!(json["restarts"], 1);
    let es = stdout(&run(
        root.path(),
        "es_MX.UTF-8",
        &daemon,
        &["service", "status"],
    ));
    assert!(es.contains("en ejecución, se reinició 1 veces"), "{es}");

    let un = run(
        root.path(),
        "es_MX.UTF-8",
        &daemon,
        &["service", "uninstall"],
    );
    assert!(
        stdout(&un).contains("loginctl disable-linger"),
        "{}",
        stdout(&un)
    );
    let log = std::fs::read_to_string(root.path().join("systemctl.log")).unwrap();
    assert_eq!(
        log.lines().collect::<Vec<_>>(),
        [
            "systemctl --user daemon-reload",
            "systemctl --user enable --now lemusyncd.service",
            "loginctl enable-linger",
            "systemctl --user show -p LoadState,UnitFileState,ActiveState,NRestarts lemusyncd.service",
            "systemctl --user show -p LoadState,UnitFileState,ActiveState,NRestarts lemusyncd.service",
            "systemctl --user disable --now lemusyncd.service",
            "systemctl --user daemon-reload",
        ]
    );
}

#[test]
fn relative_daemon_path_is_a_usage_error() {
    let root = tempfile::tempdir().unwrap();
    let out = run(
        root.path(),
        "en_US.UTF-8",
        Path::new("lemusyncd"),
        &["--json", "service", "install"],
    );
    assert_eq!(out.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["error"]["code"], "usage");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("daemon_path_invalid:")
    );
    assert!(
        !root
            .path()
            .join("config/systemd/user/lemusyncd.service")
            .exists()
    );
}
