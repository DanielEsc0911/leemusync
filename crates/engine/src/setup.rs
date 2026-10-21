//! Backend login, repo creation and joining, players (config-and-cli §Commands).
//!
//! Error payloads are stable keys or data, never prose: the CLI turns them into Fluent text (P6).

use std::fmt;
use std::path::Path;

use leemusync_api::{API_VERSION, InitReport, Warning, WarningCode};
use leemusync_core::PlayerId;
use leemusync_core::config::{ConfigError, RepoConfig};
use leemusync_crypto::{Key, RecoveryKey};
use leemusync_store::oauth::{self, OAuthClient, PendingLogin};
use leemusync_store::{ErrorClass, GdriveConfig, PutMode, S3Config, Store, StoreError};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::repo::{DeviceRecord, Repo, Unlock, key_from_hex, key_to_hex};
use crate::secrets::{secret_gdrive_client, secret_gdrive_refresh, secret_master, secret_s3_key};
use crate::{BackendConfig, Engine, EngineError, new_version_id, now_ms, repo_config};

const MIN_PASSPHRASE_CHARS: usize = 8;

/// Where an S3-compatible bucket is. The secret key is passed separately and never stored here.
#[derive(Clone, Debug)]
pub struct S3Backend {
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub access_key_id: String,
}

/// What the user typed to join. Wiped on drop; `Debug` never shows it.
#[derive(Zeroize, ZeroizeOnDrop)]
pub enum JoinSecret {
    Passphrase(String),
    Recovery(String),
}

impl fmt::Debug for JoinSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Passphrase(_) => "JoinSecret::Passphrase(..)",
            Self::Recovery(_) => "JoinSecret::Recovery(..)",
        })
    }
}

/// A Google login waiting for the browser redirect (D28).
pub struct GdriveLogin {
    pending: PendingLogin,
    client: OAuthClient,
}

impl GdriveLogin {
    /// The consent URL to open in the browser. It holds no secret.
    #[must_use]
    pub fn url(&self) -> &str {
        self.pending.url()
    }
}

impl fmt::Debug for GdriveLogin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GdriveLogin")
            .field("client", &self.client)
            .finish_non_exhaustive()
    }
}

fn player_error(e: &ConfigError) -> EngineError {
    match e {
        ConfigError::BadName => EngineError::Usage("player_name_invalid".into()),
        ConfigError::NameTaken(_) => EngineError::Usage("player_name_taken".into()),
        _ => EngineError::Config("repo_config".into()),
    }
}

/// Write, read back, list and delete one object (storage-backends §Setup).
async fn probe(store: &Store, name: &str) -> Result<(), EngineError> {
    let path = format!("leemusync/.probe/{}", name.to_ascii_lowercase());
    store
        .put(&path, b"probe".to_vec(), PutMode::CreateOnly)
        .await?;
    let read = store.get(&path).await?;
    let listed = store.list("leemusync/.probe/").await?;
    store.delete(&path).await?;
    if read != b"probe" || !listed.iter().any(|e| e.path == path) {
        return Err(EngineError::Store(StoreError::new(
            ErrorClass::Permanent,
            "probe object didn't round-trip",
        )));
    }
    Ok(())
}

impl Engine {
    /// Uses a local folder as the storage (tests, and a synced folder the user manages).
    ///
    /// # Errors
    /// `Io` if the config can't be saved.
    pub fn backend_add_fs(&mut self, path: &Path) -> Result<(), EngineError> {
        self.config.backend = Some(BackendConfig::Fs {
            path: path.to_path_buf(),
        });
        self.repo = None;
        self.save_config()
    }

    /// Uses an S3-compatible bucket (R2 for the first test, D31). Probes first (storage-backends
    /// §Setup): writes, reads, lists and deletes an object under `leemusync/.probe/`, so a wrong
    /// endpoint, bucket or key, or a read-only key, fails here and nothing is saved. The secret
    /// key goes to the OS keystore, the rest to `config.toml`.
    ///
    /// # Errors
    /// `Store` (`Auth` for refused keys) if the probe fails; `Secrets` if the keystore fails.
    pub async fn backend_add_s3(
        &mut self,
        backend: S3Backend,
        secret_access_key: &str,
    ) -> Result<(), EngineError> {
        let config = S3Config {
            endpoint: backend.endpoint.trim().trim_end_matches('/').to_owned(),
            bucket: backend.bucket.trim().to_owned(),
            region: backend.region.trim().to_owned(),
            root: "/".into(),
            access_key_id: backend.access_key_id.trim().to_owned(),
            secret_access_key: Zeroizing::new(secret_access_key.trim().to_owned()),
        };
        probe(
            &Store::s3(&config)?,
            &format!("{}-{}", self.device_id(), now_ms()),
        )
        .await?;
        self.env
            .secrets
            .set(&secret_s3_key(self.device_id()), &config.secret_access_key)?;
        self.config.backend = Some(BackendConfig::S3 {
            endpoint: config.endpoint,
            bucket: config.bucket,
            region: config.region,
            root: config.root,
            access_key_id: config.access_key_id,
        });
        self.repo = None;
        self.save_config()
    }

    /// Starts the Google login (D28). Open `url()` in a browser, then call `gdrive_login_finish`.
    ///
    /// # Errors
    /// `Store` if the loopback port can't be bound.
    pub async fn gdrive_login_start(
        &mut self,
        client_id: &str,
        client_secret: &str,
    ) -> Result<GdriveLogin, EngineError> {
        let secret = client_secret.trim();
        let client = OAuthClient {
            client_id: client_id.trim().to_owned(),
            client_secret: (!secret.is_empty()).then(|| Zeroizing::new(secret.to_owned())),
        };
        let pending = oauth::start_login(&client).await?;
        Ok(GdriveLogin { pending, client })
    }

    /// Waits for the redirect, then keeps the refresh token and client secret in the OS keystore.
    ///
    /// # Errors
    /// `Store(Auth)` for a refused or timed-out login; `Secrets` if the keystore fails.
    pub async fn gdrive_login_finish(&mut self, login: GdriveLogin) -> Result<(), EngineError> {
        let http = leemusync_store::http::http_client()?;
        let tokens = login.pending.finish(&login.client, &http).await?;
        let device = self.device_id();
        self.env
            .secrets
            .set(&secret_gdrive_refresh(device), &tokens.refresh_token)?;
        match &login.client.client_secret {
            Some(secret) => self
                .env
                .secrets
                .set(&secret_gdrive_client(device), secret)?,
            None => self.env.secrets.delete(&secret_gdrive_client(device))?,
        }
        self.config.backend = Some(BackendConfig::Gdrive {
            root: "/".into(),
            client_id: login.client.client_id.clone(),
        });
        self.repo = None;
        self.save_config()
    }

    /// The configured storage. Drive credentials come from the keystore.
    pub(crate) fn store(&self) -> Result<Store, EngineError> {
        let store = self.plain_store()?;
        #[cfg(feature = "failpoints")]
        if let Some(f) = &self.store_faults {
            return Ok(Store::faulty(store, f.clone()));
        }
        Ok(store)
    }

    fn plain_store(&self) -> Result<Store, EngineError> {
        match &self.config.backend {
            None => Err(EngineError::NotSetUp("backend")),
            Some(BackendConfig::Fs { path }) => Ok(Store::fs(path)?),
            Some(BackendConfig::Gdrive { root, client_id }) => {
                let device = self.device_id();
                let refresh = self
                    .env
                    .secrets
                    .get(&secret_gdrive_refresh(device))?
                    .ok_or(EngineError::NotSetUp("gdrive_login"))?;
                let secret = self.env.secrets.get(&secret_gdrive_client(device))?;
                Ok(Store::gdrive(GdriveConfig {
                    root: root.clone(),
                    client: OAuthClient {
                        client_id: client_id.clone(),
                        client_secret: secret,
                    },
                    refresh_token: refresh,
                })?)
            }
            Some(BackendConfig::S3 {
                endpoint,
                bucket,
                region,
                root,
                access_key_id,
            }) => {
                let secret = self
                    .env
                    .secrets
                    .get(&secret_s3_key(self.device_id()))?
                    .ok_or(EngineError::NotSetUp("s3_key"))?;
                Ok(Store::s3(&S3Config {
                    endpoint: endpoint.clone(),
                    bucket: bucket.clone(),
                    region: region.clone(),
                    root: root.clone(),
                    access_key_id: access_key_id.clone(),
                    secret_access_key: secret,
                })?)
            }
        }
    }

    /// Opens the repo once per engine with the master key cached in the keystore.
    pub(crate) async fn ensure_repo(&mut self) -> Result<(), EngineError> {
        if self.repo.is_none() {
            let repo_id = self
                .config
                .repo
                .clone()
                .ok_or(EngineError::NotSetUp("repo"))?;
            let hex = self
                .env
                .secrets
                .get(&secret_master(&repo_id))?
                .ok_or(EngineError::NotSetUp("repo_key"))?;
            let master =
                key_from_hex(&hex).ok_or_else(|| EngineError::Secrets("bad_master_key".into()))?;
            let (repo, _) = Repo::open(self.store()?, Unlock::Master(master)).await?;
            if repo.id() != repo_id {
                return Err(EngineError::Config("repo_mismatch".into()));
            }
            self.repo = Some(repo);
        }
        Ok(())
    }

    pub(crate) fn repo_ref(&self) -> Result<&Repo, EngineError> {
        self.repo.as_ref().ok_or(EngineError::NotSetUp("repo"))
    }

    /// Caches the master key, records this device, and remembers the repo id in `config.toml`.
    async fn adopt_repo(&mut self, repo: Repo, master: &Key) -> Result<InitReport, EngineError> {
        self.env
            .secrets
            .set(&secret_master(repo.id()), &key_to_hex(master))?;
        self.failpoints.hit("init.after_key")?;
        repo.put_device(&DeviceRecord {
            format: 1,
            id: self.device_id(),
            name: self.config.device.name.clone(),
            created_at: now_ms(),
        })
        .await?;
        self.config.repo = Some(repo.id().to_owned());
        self.save_config()?;
        let report = InitReport {
            api_version: API_VERSION,
            repo: repo.id().to_owned(),
            device: self.config.device.name.clone(),
            warnings: Vec::new(),
        };
        self.repo = Some(repo);
        Ok(report)
    }

    /// Creates the repo. Returns the recovery key's display form: show it once, never store it.
    ///
    /// # Errors
    /// `Usage("already_joined")` if this device already has a repo, `Usage("passphrase_too_short")`,
    /// `Usage("repo_exists")`/`Usage("repo_race")` if the storage already holds one.
    pub async fn init(
        &mut self,
        passphrase: &str,
    ) -> Result<(InitReport, Zeroizing<String>), EngineError> {
        if self.config.repo.is_some() {
            return Err(EngineError::Usage("already_joined".into()));
        }
        if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
            return Err(EngineError::Usage("passphrase_too_short".into()));
        }
        let (repo, master, recovery) =
            Repo::create(self.store()?, passphrase, self.env.kdf).await?;
        // repo.json is committed: from here on `init` never fails, or the recovery key (wrapped
        // in repo.json, never shown yet) would be lost. A failed step becomes a warning, and
        // `leemusync join` with the passphrase finishes this device.
        let shown = recovery.to_display();
        drop(recovery);
        let repo_id = repo.id().to_owned();
        let mut warnings = Vec::new();
        if let Err(e) = self.finish_init(repo, &master).await {
            warnings.push(Warning {
                code: WarningCode::SetupIncomplete,
                detail: e.code().to_owned(),
            });
        }
        let report = InitReport {
            api_version: API_VERSION,
            repo: repo_id,
            device: self.config.device.name.clone(),
            warnings,
        };
        Ok((report, shown))
    }

    /// The steps of `init` after `repo.json` exists. Each one is redone by `join`.
    async fn finish_init(&mut self, repo: Repo, master: &Key) -> Result<(), EngineError> {
        self.failpoints.hit("init.after_create")?;
        self.adopt_repo(repo, master).await?;
        self.failpoints.hit("init.before_config")?;
        let device = self.device_id();
        repo_config::write(
            self.repo_ref()?,
            &self.state,
            device,
            vec![],
            RepoConfig::default(),
        )
        .await?;
        Ok(())
    }

    /// Joins the repo on the configured storage. Joining the same repo again is allowed (it
    /// re-caches the master key); joining a different one is refused.
    ///
    /// # Errors
    /// `Unlock` for a wrong passphrase or recovery key; `Usage("already_joined")` for another repo.
    pub async fn join(&mut self, secret: JoinSecret) -> Result<InitReport, EngineError> {
        let store = self.store()?;
        let (repo, master) = match &secret {
            JoinSecret::Passphrase(p) => Repo::open(store, Unlock::Passphrase(p)).await?,
            JoinSecret::Recovery(text) => {
                let key = RecoveryKey::parse(text).map_err(|_| EngineError::Unlock)?;
                Repo::open(store, Unlock::Recovery(&key)).await?
            }
        };
        drop(secret);
        if self
            .config
            .repo
            .as_deref()
            .is_some_and(|id| id != repo.id())
        {
            return Err(EngineError::Usage("already_joined".into()));
        }
        self.adopt_repo(repo, &master).await
    }

    /// Returns what the config merge did (renamed players, merged heads, skipped invalid
    /// versions), for the user to see (D30).
    ///
    /// # Errors
    /// `Usage("player_name_invalid")` or `Usage("player_name_taken")`; storage or repo errors.
    pub async fn players_add(&mut self, name: &str) -> Result<Vec<Warning>, EngineError> {
        let id = PlayerId(new_version_id()?);
        let device = self.device_id();
        self.ensure_repo().await?;
        let repo = self.repo_ref()?;
        let (_, warnings) = repo_config::update(repo, &self.state, device, |c| {
            c.add_player(id, name).map_err(|e| player_error(&e))
        })
        .await?;
        Ok(warnings)
    }

    /// Player names in creation order (player ids are time-ordered).
    ///
    /// # Errors
    /// Storage or repo errors.
    pub async fn players_list(&mut self) -> Result<Vec<String>, EngineError> {
        self.ensure_repo().await?;
        let loaded = repo_config::load(self.repo_ref()?, &self.state).await?;
        Ok(loaded
            .config
            .players
            .values()
            .map(|p| p.name.clone())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use leemusync_api::{Warning, WarningCode};
    use leemusync_core::config::RepoConfig;
    use leemusync_core::{PlayerId, VersionId};

    use crate::tests::test_env;
    use crate::{Engine, EngineError, JoinSecret, S3Backend};

    #[tokio::test]
    async fn the_probe_round_trips_and_cleans_up() {
        let root = tempfile::tempdir().unwrap();
        let store = leemusync_store::Store::fs(root.path()).unwrap();
        super::probe(&store, "01ABC-1").await.unwrap();
        assert_eq!(store.list("leemusync/.probe/").await.unwrap(), Vec::new());
    }

    #[tokio::test]
    async fn a_bad_s3_backend_saves_nothing() {
        let root = tempfile::tempdir().unwrap();
        let mut a = Engine::open(test_env(root.path())).unwrap();
        let backend = S3Backend {
            endpoint: "http://insecure.example".into(),
            bucket: "saves".into(),
            region: "auto".into(),
            access_key_id: "id".into(),
        };
        assert!(matches!(
            a.backend_add_s3(backend, "secret").await,
            Err(EngineError::Store(_))
        ));
        assert_eq!(a.local_config().backend, None);
        let key = crate::secrets::secret_s3_key(a.device_id());
        assert!(a.env.secrets.get(&key).unwrap().is_none());
    }

    #[tokio::test]
    async fn init_on_one_device_and_join_on_another() {
        let root = tempfile::tempdir().unwrap();
        let remote = root.path().join("remote");
        let mut a = Engine::open(test_env(&root.path().join("a"))).unwrap();
        a.backend_add_fs(&remote).unwrap();
        let (report, recovery) = a.init("pass phrase").await.unwrap();
        assert_eq!(recovery.split('-').count(), 13);
        a.players_add("Ana").await.unwrap();

        let mut b = Engine::open(test_env(&root.path().join("b"))).unwrap();
        b.backend_add_fs(&remote).unwrap();
        assert!(b.join(JoinSecret::Passphrase("nope".into())).await.is_err());
        let joined = b
            .join(JoinSecret::Passphrase("pass phrase".into()))
            .await
            .unwrap();
        assert_eq!(joined.repo, report.repo);
        b.players_add("Beto").await.unwrap();
        assert!(b.players_add("ana").await.is_err(), "names are unique");
        assert_eq!(
            a.players_list().await.unwrap(),
            vec!["Ana".to_string(), "Beto".to_string()]
        );

        let mut c = Engine::open(test_env(&root.path().join("c"))).unwrap();
        c.backend_add_fs(&remote).unwrap();
        c.join(JoinSecret::Recovery(recovery.to_lowercase()))
            .await
            .unwrap();
        let config =
            std::fs::read_to_string(root.path().join("c/config/leemusync/config.toml")).unwrap();
        assert!(
            !config.contains("pass phrase")
                && !config.to_lowercase().contains(&recovery.to_lowercase())
        );
    }

    #[tokio::test]
    async fn setup_errors_are_keys_and_never_echo_secrets() {
        let root = tempfile::tempdir().unwrap();
        let remote = root.path().join("remote");
        let mut a = Engine::open(test_env(&root.path().join("a"))).unwrap();
        assert!(matches!(
            a.init("pass phrase").await,
            Err(EngineError::NotSetUp("backend"))
        ));
        assert!(matches!(
            a.players_list().await,
            Err(EngineError::NotSetUp("repo"))
        ));
        a.backend_add_fs(&remote).unwrap();
        assert!(
            matches!(a.init("short").await, Err(EngineError::Usage(k)) if k == "passphrase_too_short")
        );
        let (_, recovery) = a.init("pass phrase").await.unwrap();
        assert!(
            matches!(a.init("pass phrase").await, Err(EngineError::Usage(k)) if k == "already_joined")
        );
        assert!(
            matches!(a.players_add("").await, Err(EngineError::Usage(k)) if k == "player_name_invalid")
        );
        a.players_add("Ana").await.unwrap();
        assert!(
            matches!(a.players_add("ANA").await, Err(EngineError::Usage(k)) if k == "player_name_taken")
        );

        let mut b = Engine::open(test_env(&root.path().join("b"))).unwrap();
        b.backend_add_fs(&remote).unwrap();
        assert!(
            matches!(b.init("another pass").await, Err(EngineError::Usage(k)) if k == "repo_exists")
        );
        let wrong = b
            .join(JoinSecret::Recovery("not a key".into()))
            .await
            .unwrap_err();
        assert!(matches!(wrong, EngineError::Unlock));
        let wrong = b
            .join(JoinSecret::Passphrase("pass phrasE".into()))
            .await
            .unwrap_err();
        assert!(!wrong.to_string().contains("pass phras"));
        assert!(
            b.local_config().repo.is_none(),
            "a failed join changes nothing"
        );
        b.join(JoinSecret::Recovery(recovery.to_string()))
            .await
            .unwrap();
        // A second join of the same repo is harmless (e.g. after the keystore lost the key).
        b.join(JoinSecret::Passphrase("pass phrase".into()))
            .await
            .unwrap();
        assert_eq!(b.players_list().await.unwrap(), vec!["Ana".to_string()]);
    }

    #[tokio::test]
    async fn a_failed_step_after_create_still_returns_the_recovery_key() {
        // Each point leaves a different partial state: nothing adopted; master key cached but no
        // device record or config.toml; fully adopted but no first config version.
        for point in ["init.after_create", "init.after_key", "init.before_config"] {
            let root = tempfile::tempdir().unwrap();
            let remote = root.path().join("remote");
            let mut a = Engine::open(test_env(&root.path().join("a"))).unwrap();
            a.backend_add_fs(&remote).unwrap();
            a.arm_failpoint(point);
            let (report, recovery) = a.init("pass phrase").await.unwrap();
            assert_eq!(recovery.split('-').count(), 13, "{point}");
            assert_eq!(
                report.warnings,
                vec![Warning {
                    code: WarningCode::SetupIncomplete,
                    detail: "injected".into()
                }],
                "{point}"
            );
            // The repo exists: init again is refused, join finishes this device.
            assert!(
                matches!(a.init("pass phrase").await, Err(EngineError::Usage(_))),
                "{point}"
            );
            let joined = a
                .join(JoinSecret::Passphrase("pass phrase".into()))
                .await
                .unwrap();
            assert_eq!(
                (joined.repo.as_str(), joined.warnings.len()),
                (report.repo.as_str(), 0),
                "{point}"
            );
            assert_eq!(a.players_add("Ana").await.unwrap(), vec![], "{point}");
            assert_eq!(
                a.players_list().await.unwrap(),
                vec!["Ana".to_string()],
                "{point}"
            );
            // And the recovery key that was shown works.
            let mut c = Engine::open(test_env(&root.path().join("c"))).unwrap();
            c.backend_add_fs(&remote).unwrap();
            c.join(JoinSecret::Recovery(recovery.to_string()))
                .await
                .unwrap();
            assert_eq!(
                c.players_list().await.unwrap(),
                vec!["Ana".to_string()],
                "{point}"
            );
        }
    }

    #[tokio::test]
    async fn players_add_reports_what_the_merge_did() {
        let root = tempfile::tempdir().unwrap();
        let mut a = Engine::open(test_env(&root.path().join("a"))).unwrap();
        a.backend_add_fs(&root.path().join("remote")).unwrap();
        a.init("pass phrase").await.unwrap();
        a.ensure_repo().await.unwrap();
        // Two devices added "Zoe" at the same moment: two config heads with the same name.
        let dev = a.device_id();
        for (n, name) in [(2, "Zoe"), (3, "zoe")] {
            let mut c = RepoConfig::default();
            c.add_player(
                PlayerId(VersionId::from_parts(n, [u8::try_from(n).unwrap(); 10])),
                name,
            )
            .unwrap();
            crate::repo_config::write(a.repo_ref().unwrap(), &a.state, dev, vec![], c)
                .await
                .unwrap();
        }
        let warnings = a.players_add("Ana").await.unwrap();
        assert!(warnings.contains(&Warning {
            code: WarningCode::PlayerRenamed,
            detail: "zoe (2)".into()
        }));
        assert!(warnings.contains(&Warning {
            code: WarningCode::ConfigMerged,
            detail: String::new()
        }));
        assert_eq!(
            a.players_add("Beto").await.unwrap(),
            vec![],
            "one head again"
        );
    }

    #[tokio::test]
    async fn a_device_keeps_working_after_a_restart() {
        let root = tempfile::tempdir().unwrap();
        let remote = root.path().join("remote");
        let env = test_env(&root.path().join("a"));
        let secrets = env.secrets.clone();
        let mut a = Engine::open(env).unwrap();
        a.backend_add_fs(&remote).unwrap();
        a.init("pass phrase").await.unwrap();
        a.players_add("Ana").await.unwrap();
        drop(a);
        let mut env = test_env(&root.path().join("a"));
        env.secrets = secrets;
        let mut a = Engine::open(env).unwrap();
        assert_eq!(a.players_list().await.unwrap(), vec!["Ana".to_string()]);
    }
}
