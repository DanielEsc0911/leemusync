//! Shared `cargo metadata` loader for xtask checks.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

pub(crate) fn load(root: &Path) -> Result<Value, Vec<String>> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .map_err(|e| vec![format!("cannot run cargo metadata: {e}")])?;
    if !output.status.success() {
        return Err(vec![String::from_utf8_lossy(&output.stderr).into_owned()]);
    }
    serde_json::from_slice(&output.stdout).map_err(|e| vec![format!("invalid cargo metadata: {e}")])
}
