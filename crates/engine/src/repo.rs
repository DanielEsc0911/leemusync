//! The remote repo: `repo.json`, encrypted objects, names (sync-model §Remote layout).
//!
//! Remote data is data (B2): `repo.json` is parsed strictly and bounded, its KDF parameters are
//! checked before any key derivation, and every object is sealed with AAD that binds its kind and
//! id, so the server can't swap one object for another. Object names are lowercase.

use std::fmt::Write as _;

use leemusync_core::config::ConfigVersion;
use leemusync_core::manifest::Version;
use leemusync_core::{BlobId, DeviceId, Hash32, PlayerId, StreamId, VersionId};
use leemusync_crypto::{
    KdfParams, Key, RecoveryKey, RepoKeys, Zeroizing, keyed_hash, open as open_sealed,
    passphrase_kek, random_bytes, seal, unwrap_key, wrap,
};
use leemusync_store::{ErrorClass, PutMode, Store};
use serde::{Deserialize, Serialize};

use crate::EngineError;

pub(crate) const REPO_JSON: &str = "leemusync/repo.json";
const REPO_FORMAT: u32 = 1;
const ALGORITHM: &str = "argon2id";
const CHECK: &[u8] = b"leemusync-repo-check";
const CONFIG_DIR: &str = "leemusync/config/versions/";
const DEVICE_DIR: &str = "leemusync/devices/";

/// Upper bounds on what a (possibly hostile) server can make us parse.
pub(crate) const MAX_REPO_JSON: usize = 64 * 1024;
const MAX_CONFIG: usize = 4 * 1024 * 1024;
const MAX_VERSION: usize = 64 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepoFile {
    format: u32,
    repo_id: String,
    kdf: KdfFile,
    wrapped: Wrapped,
    check: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct KdfFile {
    algorithm: String,
    m_kib: u32,
    t: u32,
    p: u32,
    salt: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wrapped {
    passphrase: String,
    recovery: String,
}

/// Only `format`, read before the strict parse.
#[derive(Deserialize)]
struct FormatProbe {
    format: u32,
}

/// `repo.json` after validation: every field decoded and in range.
struct RepoMeta {
    id: String,
    kdf: KdfParams,
    salt: [u8; 16],
    wrapped_passphrase: Vec<u8>,
    wrapped_recovery: Vec<u8>,
    check: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceRecord {
    pub format: u32,
    pub id: DeviceId,
    pub name: String,
    pub created_at: u64,
}

pub(crate) enum Unlock<'a> {
    Passphrase(&'a str),
    Recovery(&'a RecoveryKey),
    Master(Key),
}

pub(crate) struct Repo {
    store: Store,
    keys: RepoKeys,
    id: String,
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Lowercase hex only, so every value has exactly one spelling.
pub(crate) fn unhex(text: &str) -> Option<Vec<u8>> {
    fn digit(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        }
    }
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    bytes
        .chunks(2)
        .map(|p| Some((digit(p[0])? << 4) | digit(p[1])?))
        .collect()
}

fn lower(id: &VersionId) -> String {
    id.to_string().to_lowercase()
}

pub(crate) fn blob_path(id: &BlobId) -> String {
    let h = id.0.to_hex();
    format!("leemusync/blobs/{}/{h}", &h[..2])
}

pub(crate) fn config_path(id: &VersionId) -> String {
    format!("{CONFIG_DIR}{}", lower(id))
}

fn version_dir(stream: &StreamId) -> String {
    format!("leemusync/streams/{stream}/versions/")
}

fn version_path(stream: &StreamId, id: &VersionId) -> String {
    format!("{}{}", version_dir(stream), lower(id))
}

/// The id at the end of a listed name; names that aren't canonical ids are ignored.
fn last_id(path: &str) -> Option<VersionId> {
    let name = path.rsplit('/').next()?;
    let id: VersionId = name.parse().ok()?;
    (lower(&id) == name).then_some(id)
}

fn wrap_aad(kind: &str, repo: &str) -> Vec<u8> {
    format!("wrap\0{kind}\0{repo}").into_bytes()
}

fn check_aad(repo: &str) -> Vec<u8> {
    format!("check\0{repo}").into_bytes()
}

fn data(path: &str) -> EngineError {
    EngineError::Data(path.to_owned())
}

async fn get_bounded(store: &Store, path: &str, max: usize) -> Result<Vec<u8>, EngineError> {
    let bytes = store.get(path).await?;
    if bytes.len() > max {
        return Err(data(path));
    }
    Ok(bytes)
}

/// Reads and validates `repo.json`. The KDF parameters are checked here, before any Argon2 run.
async fn read_meta(store: &Store) -> Result<RepoMeta, EngineError> {
    let bytes = get_bounded(store, REPO_JSON, MAX_REPO_JSON).await?;
    // The format is read first: a newer one may add fields, and that's an upgrade, not corruption.
    let probe: FormatProbe = serde_json::from_slice(&bytes).map_err(|_| data(REPO_JSON))?;
    if probe.format > REPO_FORMAT {
        return Err(EngineError::Data("repo_format_newer".into()));
    }
    let file: RepoFile = serde_json::from_slice(&bytes).map_err(|_| data(REPO_JSON))?;
    if file.format != REPO_FORMAT || file.kdf.algorithm != ALGORITHM {
        return Err(data(REPO_JSON));
    }
    let id_ok = file.repo_id.len() == 32 && unhex(&file.repo_id).is_some();
    if !id_ok {
        return Err(data(REPO_JSON));
    }
    let kdf = KdfParams {
        m_kib: file.kdf.m_kib,
        t: file.kdf.t,
        p: file.kdf.p,
    };
    kdf.check()?;
    let decode = |text: &str| unhex(text).ok_or_else(|| data(REPO_JSON));
    let salt: [u8; 16] = decode(&file.kdf.salt)?
        .try_into()
        .map_err(|_| data(REPO_JSON))?;
    Ok(RepoMeta {
        id: file.repo_id.clone(),
        kdf,
        salt,
        wrapped_passphrase: decode(&file.wrapped.passphrase)?,
        wrapped_recovery: decode(&file.wrapped.recovery)?,
        check: decode(&file.check)?,
    })
}

impl Repo {
    /// Creates `repo.json` with a new master key wrapped under the passphrase and a new recovery key.
    ///
    /// # Errors
    /// `Usage("repo_exists")` if the storage already holds a repo, `Usage("repo_race")` if another
    /// device created one at the same moment (V-STORE-8); storage and crypto errors.
    pub(crate) async fn create(
        store: Store,
        passphrase: &str,
        kdf: KdfParams,
    ) -> Result<(Self, Key, RecoveryKey), EngineError> {
        kdf.check()?;
        if store.stat(REPO_JSON).await?.is_some() {
            return Err(EngineError::Usage("repo_exists".into()));
        }
        let master = Key::random()?;
        let recovery = RecoveryKey::generate()?;
        let salt: [u8; 16] = random_bytes()?;
        let repo_id = hex(&random_bytes::<16>()?);
        let kek = passphrase_kek(passphrase.as_bytes(), &salt, &kdf)?;
        let keys = RepoKeys::from_master(&master);
        let file = RepoFile {
            format: REPO_FORMAT,
            repo_id: repo_id.clone(),
            kdf: KdfFile {
                algorithm: ALGORITHM.into(),
                m_kib: kdf.m_kib,
                t: kdf.t,
                p: kdf.p,
                salt: hex(&salt),
            },
            wrapped: Wrapped {
                passphrase: hex(&wrap(&kek, &wrap_aad("passphrase", &repo_id), &master)?),
                recovery: hex(&wrap(
                    &recovery.kek(),
                    &wrap_aad("recovery", &repo_id),
                    &master,
                )?),
            },
            check: hex(&seal(&keys.object, &check_aad(&repo_id), CHECK)?),
        };
        let json = serde_json::to_vec(&file).map_err(|_| data(REPO_JSON))?;
        match store.put(REPO_JSON, json, PutMode::CreateOnly).await {
            Err(e) if e.class() == ErrorClass::AlreadyExists => {
                return Err(EngineError::Usage("repo_race".into()));
            }
            other => other?,
        }
        // V-STORE-8: Drive may keep two repo.json files and serve the newest. Read back and compare.
        if read_meta(&store).await?.id != repo_id {
            return Err(EngineError::Usage("repo_race".into()));
        }
        Ok((
            Self {
                store,
                keys,
                id: repo_id,
            },
            master,
            recovery,
        ))
    }

    /// Unlocks the repo. A wrong passphrase, recovery key or cached master key gives `Unlock`.
    ///
    /// # Errors
    /// `Unlock`; `Data` for an invalid `repo.json`; `Crypto(KdfParams)` for out-of-range KDF
    /// parameters; storage errors.
    pub(crate) async fn open(store: Store, unlock: Unlock<'_>) -> Result<(Self, Key), EngineError> {
        let meta = read_meta(&store).await?;
        let id = meta.id;
        let master = match unlock {
            Unlock::Master(key) => key,
            Unlock::Passphrase(p) => {
                let kek = passphrase_kek(p.as_bytes(), &meta.salt, &meta.kdf)?;
                unwrap_key(&kek, &wrap_aad("passphrase", &id), &meta.wrapped_passphrase)
                    .map_err(|_| EngineError::Unlock)?
            }
            Unlock::Recovery(r) => {
                unwrap_key(&r.kek(), &wrap_aad("recovery", &id), &meta.wrapped_recovery)
                    .map_err(|_| EngineError::Unlock)?
            }
        };
        let keys = RepoKeys::from_master(&master);
        let check = open_sealed(&keys.object, &check_aad(&id), &meta.check)
            .map_err(|_| EngineError::Unlock)?;
        if check != CHECK {
            return Err(EngineError::Unlock);
        }
        Ok((Self { store, keys, id }, master))
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn blob_id(&self, data: &[u8]) -> BlobId {
        BlobId(Hash32(keyed_hash(&self.keys.blob_id, data)))
    }

    pub(crate) fn stream_id(&self, player: PlayerId, emulator: &str, game: &str) -> StreamId {
        let canonical = format!("save\0{player}\0{emulator}\0{game}\0default");
        StreamId(Hash32(keyed_hash(
            &self.keys.stream_id,
            canonical.as_bytes(),
        )))
    }

    async fn put_sealed(&self, path: &str, aad: &str, plain: &[u8]) -> Result<(), EngineError> {
        let sealed = seal(&self.keys.object, aad.as_bytes(), plain)?;
        self.store.put(path, sealed, PutMode::CreateOnly).await?;
        Ok(())
    }

    async fn get_sealed(&self, path: &str, aad: &str, max: usize) -> Result<Vec<u8>, EngineError> {
        let sealed = get_bounded(&self.store, path, max).await?;
        open_sealed(&self.keys.object, aad.as_bytes(), &sealed).map_err(|_| data(path))
    }

    async fn list_ids(&self, dir: &str) -> Result<Vec<VersionId>, EngineError> {
        let mut ids: Vec<VersionId> = self
            .store
            .list(dir)
            .await?
            .iter()
            .filter_map(|e| last_id(&e.path))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }

    /// Idempotent: the name is the content's keyed hash. An existing object is verified first;
    /// `Data("blob_corrupt:<id>")` if it fails to decrypt or hash.
    pub(crate) async fn put_blob(&self, data: &[u8]) -> Result<BlobId, EngineError> {
        let id = self.blob_id(data);
        match self
            .put_sealed(&blob_path(&id), &format!("blob\0{id}"), data)
            .await
        {
            // Same name, so it should be the same content; a torn or tampered object is
            // never referenced by a new manifest (I6) and never overwritten (I2).
            Err(EngineError::Store(e)) if e.class() == ErrorClass::AlreadyExists => {
                match self.get_blob(&id).await {
                    Ok(_) => Ok(id),
                    Err(EngineError::Data(_)) => {
                        Err(EngineError::Data(format!("blob_corrupt:{id}")))
                    }
                    Err(other) => Err(other),
                }
            }
            other => other.map(|()| id),
        }
    }

    /// Decrypts and checks that the content still hashes to `id`.
    pub(crate) async fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>, EngineError> {
        let path = blob_path(id);
        let data = self
            .get_sealed(&path, &format!("blob\0{id}"), usize::MAX)
            .await?;
        if self.blob_id(&data) != *id {
            return Err(EngineError::Data(path));
        }
        Ok(data)
    }

    pub(crate) async fn put_version(&self, v: &Version) -> Result<(), EngineError> {
        let path = version_path(&v.stream, &v.id);
        let json = v.to_json().map_err(|_| data(&path))?;
        self.put_sealed(&path, &format!("version\0{}\0{}", v.stream, v.id), &json)
            .await
    }

    pub(crate) async fn list_versions(
        &self,
        stream: &StreamId,
    ) -> Result<Vec<VersionId>, EngineError> {
        self.list_ids(&version_dir(stream)).await
    }

    /// The decrypted manifest JSON; parse it with `Version::from_json`.
    pub(crate) async fn get_version(
        &self,
        stream: &StreamId,
        id: &VersionId,
    ) -> Result<Vec<u8>, EngineError> {
        self.get_sealed(
            &version_path(stream, id),
            &format!("version\0{stream}\0{id}"),
            MAX_VERSION,
        )
        .await
    }

    pub(crate) async fn put_config(&self, v: &ConfigVersion) -> Result<(), EngineError> {
        let path = config_path(&v.id);
        let json = v.to_json().map_err(|_| data(&path))?;
        self.put_sealed(&path, &format!("config\0{}", v.id), &json)
            .await
    }

    pub(crate) async fn list_configs(&self) -> Result<Vec<VersionId>, EngineError> {
        self.list_ids(CONFIG_DIR).await
    }

    /// The decrypted config-version JSON; parse it with `ConfigVersion::from_json`.
    pub(crate) async fn get_config(&self, id: &VersionId) -> Result<Vec<u8>, EngineError> {
        self.get_sealed(&config_path(id), &format!("config\0{id}"), MAX_CONFIG)
            .await
    }

    /// Idempotent: a device that joins again keeps its first record.
    pub(crate) async fn put_device(&self, d: &DeviceRecord) -> Result<(), EngineError> {
        let path = format!("{DEVICE_DIR}{}", lower(&d.id.0));
        let json = serde_json::to_vec(d).map_err(|_| data(&path))?;
        match self
            .put_sealed(&path, &format!("device\0{}", d.id), &json)
            .await
        {
            Err(EngineError::Store(e)) if e.class() == ErrorClass::AlreadyExists => Ok(()),
            other => other,
        }
    }
}

/// The master key as the keystore holds it; wiped on drop.
pub(crate) fn key_to_hex(key: &Key) -> Zeroizing<String> {
    Zeroizing::new(hex(key.as_bytes()))
}

/// `None` unless `text` is exactly 64 lowercase hex characters.
pub(crate) fn key_from_hex(text: &str) -> Option<Key> {
    let bytes = Zeroizing::new(unhex(text)?);
    let array: Zeroizing<[u8; 32]> = Zeroizing::new(bytes.as_slice().try_into().ok()?);
    Some(Key::from_bytes(*array))
}

#[cfg(test)]
mod tests {
    use super::*;
    use leemusync_crypto::KdfParams;

    const FAST: KdfParams = KdfParams {
        m_kib: 19_456,
        t: 1,
        p: 1,
    };

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::fs(dir.path()).unwrap();
        (dir, s)
    }

    #[tokio::test]
    async fn create_then_open_three_ways() {
        let (_d, s) = store();
        let (repo, master, recovery) = Repo::create(s.clone(), "pass phrase", FAST).await.unwrap();
        let id = repo.id().to_owned();
        let (r1, m1) = Repo::open(s.clone(), Unlock::Passphrase("pass phrase"))
            .await
            .unwrap();
        assert_eq!((r1.id(), m1.as_bytes()), (id.as_str(), master.as_bytes()));
        let (_, m2) = Repo::open(s.clone(), Unlock::Recovery(&recovery))
            .await
            .unwrap();
        assert_eq!(m2.as_bytes(), master.as_bytes());
        assert!(
            Repo::open(s.clone(), Unlock::Master(master.clone()))
                .await
                .is_ok()
        );
        assert!(matches!(
            Repo::open(s.clone(), Unlock::Passphrase("wrong")).await,
            Err(EngineError::Unlock)
        ));
        assert!(matches!(
            Repo::open(s.clone(), Unlock::Master(Key::from_bytes([0; 32]))).await,
            Err(EngineError::Unlock)
        ));
        assert!(
            matches!(Repo::create(s, "x", FAST).await, Err(EngineError::Usage(_))),
            "never two repos"
        );
    }

    #[tokio::test]
    async fn repo_json_never_holds_the_master_key() {
        let (_d, s) = store();
        let (_, master, _) = Repo::create(s.clone(), "p", FAST).await.unwrap();
        let text = String::from_utf8(s.get(REPO_JSON).await.unwrap()).unwrap();
        let hex = master.as_bytes().iter().fold(String::new(), |mut acc, b| {
            let _ = write!(acc, "{b:02x}");
            acc
        });
        assert!(!text.contains(&hex));
        assert!(text.contains("\"algorithm\":\"argon2id\""));
    }

    #[tokio::test]
    async fn blobs_are_content_addressed_and_authenticated() {
        let (_d, s) = store();
        let (repo, _, _) = Repo::create(s.clone(), "p", FAST).await.unwrap();
        let a = repo.put_blob(b"save A").await.unwrap();
        assert_eq!(
            repo.put_blob(b"save A").await.unwrap(),
            a,
            "idempotent, dedupes"
        );
        assert_eq!(repo.get_blob(&a).await.unwrap(), b"save A");
        let b = repo.put_blob(b"save B").await.unwrap();
        // A server swapping two objects is caught by the AAD.
        let b_bytes = s.get(&blob_path(&b)).await.unwrap();
        s.put(&blob_path(&a), b_bytes, leemusync_store::PutMode::Overwrite)
            .await
            .unwrap();
        assert!(repo.get_blob(&a).await.is_err());
    }

    #[tokio::test]
    async fn versions_list_and_read_back() {
        let (_d, s) = store();
        let (repo, _, _) = Repo::create(s, "p", FAST).await.unwrap();
        let stream = repo.stream_id(
            leemusync_core::PlayerId(VersionId::from_parts(1, [1; 10])),
            "cemu",
            "101c9400",
        );
        assert_eq!(
            repo.list_versions(&stream).await.unwrap(),
            Vec::<VersionId>::new()
        );
        let id = VersionId::from_parts(5, [5; 10]);
        let v = leemusync_core::manifest::Version {
            format: 1,
            stream,
            id,
            parents: vec![],
            device: DeviceId(VersionId::from_parts(2, [2; 10])),
            player: leemusync_core::PlayerId(VersionId::from_parts(1, [1; 10])),
            created_at: 5,
            emulator: leemusync_core::manifest::EmulatorRef {
                id: "cemu".into(),
                version: None,
            },
            files: vec![],
            meta: leemusync_core::manifest::Meta {
                session_secs: None,
                changed_files: 0,
                takeover: false,
                note: None,
            },
        };
        repo.put_version(&v).await.unwrap();
        assert_eq!(repo.list_versions(&stream).await.unwrap(), vec![id]);
        let json = repo.get_version(&stream, &id).await.unwrap();
        assert_eq!(
            leemusync_core::manifest::Version::from_json(&json, &stream, &id).unwrap(),
            v
        );
    }

    type Edit = fn(&mut serde_json::Value);

    /// Rewrites one field of the stored `repo.json`.
    async fn tamper(s: &Store, f: impl FnOnce(&mut serde_json::Value)) {
        let mut v: serde_json::Value =
            serde_json::from_slice(&s.get(REPO_JSON).await.unwrap()).unwrap();
        f(&mut v);
        s.put(
            REPO_JSON,
            serde_json::to_vec(&v).unwrap(),
            PutMode::Overwrite,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn remote_kdf_params_are_checked_before_any_key_derivation() {
        for (field, value) in [
            ("m_kib", 2_000_000),
            ("m_kib", 1024),
            ("t", 0),
            ("t", 11),
            ("p", 5),
        ] {
            let (_d, s) = store();
            let (_, master, _) = Repo::create(s.clone(), "pass phrase", FAST).await.unwrap();
            tamper(&s, |v| v["kdf"][field] = value.into()).await;
            for unlock in [
                Unlock::Passphrase("pass phrase"),
                Unlock::Master(master.clone()),
            ] {
                assert!(
                    matches!(
                        Repo::open(s.clone(), unlock).await,
                        Err(EngineError::Crypto(
                            leemusync_crypto::CryptoError::KdfParams
                        ))
                    ),
                    "{field} = {value}"
                );
            }
        }
    }

    #[tokio::test]
    async fn repo_json_is_parsed_strictly() {
        let cases: [(&str, Edit); 7] = [
            ("unknown field", |v| v["extra"] = 1.into()),
            ("unknown kdf field", |v| v["kdf"]["extra"] = 1.into()),
            ("format", |v| v["format"] = 2.into()),
            ("algorithm", |v| v["kdf"]["algorithm"] = "scrypt".into()),
            ("repo id path", |v| v["repo_id"] = "../../etc".into()),
            ("repo id case", |v| {
                let id = v["repo_id"].as_str().unwrap().to_uppercase();
                v["repo_id"] = id.into();
            }),
            ("salt", |v| v["kdf"]["salt"] = "00".into()),
        ];
        for (name, f) in cases {
            let (_d, s) = store();
            Repo::create(s.clone(), "pass phrase", FAST).await.unwrap();
            tamper(&s, f).await;
            assert!(
                matches!(
                    Repo::open(s.clone(), Unlock::Passphrase("pass phrase")).await,
                    Err(EngineError::Data(_))
                ),
                "{name}"
            );
        }
        let (_d, s) = store();
        Repo::create(s.clone(), "pass phrase", FAST).await.unwrap();
        // Still valid JSON (trailing whitespace), only too big.
        let mut padded = s.get(REPO_JSON).await.unwrap();
        padded.resize(MAX_REPO_JSON + 1, b' ');
        s.put(REPO_JSON, padded, PutMode::Overwrite).await.unwrap();
        assert!(
            matches!(
                Repo::open(s, Unlock::Passphrase("pass phrase")).await,
                Err(EngineError::Data(_))
            ),
            "size"
        );
    }

    #[tokio::test]
    async fn a_newer_repo_format_is_refused_with_its_own_key() {
        let (_d, s) = store();
        Repo::create(s.clone(), "pass phrase", FAST).await.unwrap();
        // A newer format may add fields: it must not be reported as corruption.
        tamper(&s, |v| {
            v["format"] = 2.into();
            v["new_field"] = true.into();
        })
        .await;
        assert!(matches!(
            Repo::open(s.clone(), Unlock::Passphrase("pass phrase")).await,
            Err(EngineError::Data(k)) if k == "repo_format_newer"
        ));
        tamper(&s, |v| v["format"] = 0.into()).await;
        assert!(matches!(
            Repo::open(s, Unlock::Passphrase("pass phrase")).await,
            Err(EngineError::Data(k)) if k == REPO_JSON
        ));
    }

    #[tokio::test]
    async fn a_swapped_repo_id_fails_the_key_check() {
        let (_d, s) = store();
        let (_, master, _) = Repo::create(s.clone(), "pass phrase", FAST).await.unwrap();
        tamper(&s, |v| {
            v["repo_id"] = "0123456789abcdef0123456789abcdef".into();
        })
        .await;
        assert!(matches!(
            Repo::open(s.clone(), Unlock::Master(master)).await,
            Err(EngineError::Unlock)
        ));
        assert!(matches!(
            Repo::open(s, Unlock::Passphrase("pass phrase")).await,
            Err(EngineError::Unlock)
        ));
    }

    #[tokio::test]
    async fn objects_are_bound_to_their_kind_and_id() {
        let (_d, s) = store();
        let (repo, _, _) = Repo::create(s.clone(), "p", FAST).await.unwrap();
        let dev = DeviceId(VersionId::from_parts(1, [1; 10]));
        let config = |n: u64| ConfigVersion {
            format: leemusync_core::config::CONFIG_FORMAT,
            id: VersionId::from_parts(n, [u8::try_from(n).unwrap(); 10]),
            parents: vec![],
            device: dev,
            created_at: n,
            config: leemusync_core::config::RepoConfig::default(),
        };
        let (a, b) = (config(10), config(20));
        repo.put_config(&a).await.unwrap();
        repo.put_config(&b).await.unwrap();
        assert_eq!(repo.list_configs().await.unwrap(), vec![a.id, b.id]);
        assert_eq!(
            ConfigVersion::from_json(&repo.get_config(&a.id).await.unwrap(), &a.id).unwrap(),
            a
        );
        // Same kind, other id.
        let b_bytes = s.get(&config_path(&b.id)).await.unwrap();
        s.put(&config_path(&a.id), b_bytes, PutMode::Overwrite)
            .await
            .unwrap();
        assert!(matches!(
            repo.get_config(&a.id).await,
            Err(EngineError::Data(_))
        ));
        // Other kind, same id text: a device record can't pass as a config version.
        let record = DeviceRecord {
            format: 1,
            id: DeviceId(b.id),
            name: "pc".into(),
            created_at: 1,
        };
        repo.put_device(&record).await.unwrap();
        repo.put_device(&record).await.unwrap();
        let d_bytes = s
            .get(&format!("leemusync/devices/{}", lower(&b.id)))
            .await
            .unwrap();
        s.put(&config_path(&b.id), d_bytes, PutMode::Overwrite)
            .await
            .unwrap();
        assert!(matches!(
            repo.get_config(&b.id).await,
            Err(EngineError::Data(_))
        ));
    }

    #[tokio::test]
    async fn object_names_are_lowercase() {
        let (_d, s) = store();
        let (repo, _, _) = Repo::create(s.clone(), "p", FAST).await.unwrap();
        let id = VersionId::from_parts(1_700_000_000_000, [0xab; 10]);
        assert!(id.to_string().chars().any(|c| c.is_ascii_uppercase()));
        assert_eq!(
            config_path(&id),
            format!(
                "leemusync/config/versions/{}",
                id.to_string().to_lowercase()
            )
        );
        let blob = repo.put_blob(b"x").await.unwrap();
        assert!(
            blob_path(&blob).starts_with(&format!("leemusync/blobs/{}/", &blob.to_string()[..2]))
        );
        assert_eq!(repo.id().len(), 32);
        assert!(
            repo.id()
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
    }
}
