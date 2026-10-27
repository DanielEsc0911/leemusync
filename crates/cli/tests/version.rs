use std::process::Command;

#[test]
fn prints_name_and_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_lemusync"))
        .arg("--version")
        .output()
        .expect("binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        format!("lemusync {}", env!("CARGO_PKG_VERSION"))
    );
}
