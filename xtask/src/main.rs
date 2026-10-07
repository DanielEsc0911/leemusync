//! Repository checks, run as `cargo xtask <task>`.

mod docs;
mod layers;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let root = repo_root();
    let task = std::env::args().nth(1).unwrap_or_default();
    let result = match task.as_str() {
        "docs-check" => docs::check(&root),
        "layers" => layers::check(&root),
        _ => Err(vec![format!(
            "unknown task `{task}`; available: docs-check, layers"
        )]),
    };
    match result {
        Ok(()) => {
            println!("xtask {task}: ok");
            ExitCode::SUCCESS
        }
        Err(errors) => {
            for error in &errors {
                eprintln!("error: {error}");
            }
            ExitCode::FAILURE
        }
    }
}

/// The repository root: the parent of this crate's directory.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}
