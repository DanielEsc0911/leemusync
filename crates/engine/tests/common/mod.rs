//! Shared helpers for engine integration tests. Fake data only.
#![allow(clippy::unwrap_used)] // test helpers: a failure here is a failed test
#![allow(dead_code)] // each test binary uses a different subset

use std::path::{Path, PathBuf};
use std::sync::Arc;

use leemusync_engine::{EngineEnv, FakeProbe, Locations, MemorySecrets, fsx};

pub const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../profiles/fixtures/cemu/flatpak-botw-two-accounts"
);

pub fn env(root: &Path) -> EngineEnv {
    env_with(
        root,
        Arc::new(FakeProbe::default()),
        Arc::new(MemorySecrets::default()),
    )
}

/// Keeping `secrets` across `Engine::open` calls simulates the OS keystore surviving a restart.
pub fn env_with(root: &Path, probe: Arc<FakeProbe>, secrets: Arc<MemorySecrets>) -> EngineEnv {
    EngineEnv {
        locations: Locations::under(root),
        secrets,
        processes: probe,
        kdf: leemusync_crypto::KdfParams {
            m_kib: 19_456,
            t: 1,
            p: 1,
        },
    }
}

/// Installs the fixture as a Flatpak (`flatpak = true`) or native Cemu under `root/home`.
/// Returns the Cemu data folder.
pub fn install_fixture(root: &Path, flatpak: bool) -> PathBuf {
    let home = root.join("home");
    let (data, config) = if flatpak {
        (
            home.join(".var/app/info.cemu.Cemu/data/Cemu"),
            home.join(".var/app/info.cemu.Cemu/config/Cemu"),
        )
    } else {
        (home.join(".local/share/Cemu"), home.join(".config/Cemu"))
    };
    fsx::copy_tree(&Path::new(FIXTURE).join("data/Cemu"), &data).unwrap();
    fsx::copy_tree(&Path::new(FIXTURE).join("config/Cemu"), &config).unwrap();
    data
}
