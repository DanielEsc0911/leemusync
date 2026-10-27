//! Shared helpers for engine integration tests. Fake data only.
#![allow(clippy::unwrap_used)] // test helpers: a failure here is a failed test
#![allow(dead_code)] // each test binary uses a different subset

use std::path::{Path, PathBuf};
use std::sync::Arc;

use std::collections::BTreeMap;

use lemusync_engine::{
    Engine, EngineEnv, EngineError, FakeProbe, JoinSecret, Locations, MapChoice, MapOutcome,
    MemorySecrets, SecretStore, SyncOptions, fsx,
};
use zeroize::Zeroizing;

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

/// A keystore in one JSON file, so it survives a killed process (`tests/kill.rs`). Fake keys only.
pub struct FileSecrets(PathBuf);

impl FileSecrets {
    pub fn under(root: &Path) -> Self {
        Self(root.join("keystore.json"))
    }

    fn load(&self) -> BTreeMap<String, String> {
        std::fs::read(&self.0)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn store(&self, map: &BTreeMap<String, String>) -> Result<(), EngineError> {
        fsx::write_atomic(&self.0, &serde_json::to_vec(map).unwrap())?;
        Ok(())
    }
}

impl SecretStore for FileSecrets {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<String>>, EngineError> {
        Ok(self.load().remove(name).map(Zeroizing::new))
    }

    fn set(&self, name: &str, value: &str) -> Result<(), EngineError> {
        let mut map = self.load();
        map.insert(name.to_owned(), value.to_owned());
        self.store(&map)
    }

    fn delete(&self, name: &str) -> Result<(), EngineError> {
        let mut map = self.load();
        map.remove(name);
        self.store(&map)
    }
}

/// Keeping `secrets` across `Engine::open` calls simulates the OS keystore surviving a restart.
pub fn env_with(root: &Path, probe: Arc<FakeProbe>, secrets: Arc<dyn SecretStore>) -> EngineEnv {
    EngineEnv {
        locations: Locations::under(root),
        secrets,
        processes: probe,
        kdf: lemusync_crypto::KdfParams {
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

pub const SAVE: &str = "mlc01/usr/save/00050000/101c9400/user";

/// One simulated PC: its own config, data, keystore and Cemu tree. Shared remote at `remote`.
pub struct Pc {
    pub root: PathBuf,
    pub cemu: PathBuf,
    pub probe: Arc<FakeProbe>,
    pub secrets: Arc<dyn SecretStore>,
    pub engine: Engine,
}

impl Pc {
    /// A PC with one Cemu account `80000001` whose `BotW` save contains `content`.
    pub fn new(root: &Path, flatpak: bool, content: &[u8]) -> Self {
        let cemu = install_fixture(root, flatpak);
        std::fs::remove_dir_all(cemu.join("mlc01/usr/save/system/act/80000002")).unwrap();
        std::fs::remove_dir_all(cemu.join(SAVE).join("80000002")).unwrap();
        std::fs::write(cemu.join(SAVE).join("80000001/0/game_data.sav"), content).unwrap();
        let probe = Arc::new(FakeProbe::default());
        let secrets: Arc<dyn SecretStore> = Arc::new(FileSecrets::under(root));
        let mut engine = Engine::open(env_with(root, probe.clone(), secrets.clone())).unwrap();
        let grant = if flatpak {
            root.join("home/.var/app/info.cemu.Cemu")
        } else {
            cemu.clone()
        };
        engine.grant_add(&grant).unwrap();
        if !flatpak {
            engine.grant_add(&root.join("home/.config/Cemu")).unwrap();
        }
        Pc {
            root: root.to_path_buf(),
            cemu,
            probe,
            secrets,
            engine,
        }
    }

    /// Opens the PC another process set up under `root` (its keystore is `FileSecrets`).
    pub fn attach(root: &Path, flatpak: bool) -> Self {
        let cemu = if flatpak {
            root.join("home/.var/app/info.cemu.Cemu/data/Cemu")
        } else {
            root.join("home/.local/share/Cemu")
        };
        let probe = Arc::new(FakeProbe::default());
        let secrets: Arc<dyn SecretStore> = Arc::new(FileSecrets::under(root));
        let engine = Engine::open(env_with(root, probe.clone(), secrets.clone())).unwrap();
        Pc {
            root: root.to_path_buf(),
            cemu,
            probe,
            secrets,
            engine,
        }
    }

    /// Simulates a crash and restart: drops the engine (releasing the lock) and opens a new one.
    #[must_use]
    pub fn reopen(self) -> Self {
        let Pc {
            root,
            cemu,
            probe,
            secrets,
            engine,
        } = self;
        drop(engine);
        let engine = Engine::open(env_with(&root, probe.clone(), secrets.clone())).unwrap();
        Pc {
            root,
            cemu,
            probe,
            secrets,
            engine,
        }
    }

    pub fn save(&self, account: &str) -> Vec<u8> {
        std::fs::read(self.cemu.join(SAVE).join(account).join("0/game_data.sav")).unwrap()
    }

    /// Creates a Cemu account folder by copying 80000001's account.dat (what choice b does by hand).
    pub fn create_account(&self, id: &str) {
        let act = self.cemu.join("mlc01/usr/save/system/act");
        fsx::copy_tree(&act.join("80000001"), &act.join(id)).unwrap();
    }

    pub async fn sync(&mut self) -> lemusync_api::SyncReport {
        self.engine.confirm_first_sync().unwrap();
        self.engine
            .sync(SyncOptions { dry_run: false })
            .await
            .unwrap()
    }
}

pub async fn map(pc: &mut Pc, player: &str, account: &str, choice: MapChoice) -> MapOutcome {
    pc.engine
        .accounts_map(player, "cemu", account, choice)
        .await
        .unwrap()
}

pub async fn join(pc: &mut Pc, remote: &Path) {
    pc.engine.backend_add_fs(remote).unwrap();
    pc.engine
        .join(JoinSecret::Passphrase("pass phrase".into()))
        .await
        .unwrap();
}
