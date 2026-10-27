//! Storage backend interface and adapters. Moves opaque encrypted objects.
//! Spec: docs/specs/storage-backends.md.

mod error;
#[cfg(any(test, feature = "faults"))]
mod faults;
mod gdrive;
pub mod http;
pub mod oauth;
mod redact;
mod retry;
mod s3;

use std::path::Path;
use std::sync::Arc;

use opendal::services::Fs;
use opendal::{Buffer, Operator};

pub use error::{ErrorClass, StoreError};
#[cfg(any(test, feature = "faults"))]
pub use faults::{Fault, Faults};
pub use gdrive::GdriveConfig;
pub use redact::redact;
pub use s3::S3Config;

pub use retry::{RetryNotice, RetryObserver, Watch as RetryWatch};
use retry::{RetryPolicy, Watch, retry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PutMode {
    CreateOnly,
    /// Only leases and `repo.json` (storage-backends §Interface).
    Overwrite,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub size: u64,
    /// The backend's server-side modified time, when it reports one (`None` on Drive `list`, V-SYNC-2).
    pub modified: Option<std::time::SystemTime>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub native_create_only: bool,
}

#[derive(Clone)]
pub struct Store {
    inner: Inner,
    caps: Capabilities,
    policy: RetryPolicy,
    watch: Watch,
    #[cfg(any(test, feature = "faults"))]
    faults: faults::Faults,
}

#[derive(Clone)]
enum Inner {
    Plain(Operator),
    Gdrive(Arc<gdrive::GdriveSession>),
}

impl Store {
    /// A local folder. Test-grade in Phase 1: V-STORE-5 (crash-safe atomic create) is still open.
    ///
    /// # Errors
    /// `Permanent` if the folder can't be created or opened.
    pub fn fs(root: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(root)
            .map_err(|e| StoreError::new(ErrorClass::Permanent, &e.to_string()))?;
        let root = root
            .to_str()
            .ok_or_else(|| StoreError::new(ErrorClass::Permanent, "store path is not UTF-8"))?;
        let op = Operator::new(Fs::default().root(root))?;
        let native = op.info().capability().write_with_if_not_exists;
        Ok(Self::with(Inner::Plain(op), native))
    }

    fn with(inner: Inner, native_create_only: bool) -> Self {
        Self {
            inner,
            caps: Capabilities { native_create_only },
            policy: RetryPolicy::default(),
            watch: Watch::default(),
            #[cfg(any(test, feature = "faults"))]
            faults: faults::Faults::default(),
        }
    }

    /// Tells `watch` about each retry and the recovery after it (reliability §Network).
    #[must_use]
    pub fn with_retry_watch(mut self, watch: RetryWatch) -> Self {
        self.watch = watch;
        self
    }

    #[must_use]
    pub fn capabilities(&self) -> Capabilities {
        self.caps
    }

    /// Test-only: `inner` with injected request faults from `plan` and millisecond retries.
    #[cfg(feature = "faults")]
    #[must_use]
    pub fn faulty(mut inner: Store, plan: Faults) -> Store {
        inner.faults = plan;
        inner.policy = RetryPolicy {
            attempts: 3,
            base: std::time::Duration::from_millis(1),
            cap: std::time::Duration::from_millis(4),
        };
        inner
    }

    /// Google Drive with the user's OAuth client and refresh token (D26, D27).
    ///
    /// # Errors
    /// `Permanent` if the HTTP client can't be built.
    pub fn gdrive(config: GdriveConfig) -> Result<Self, StoreError> {
        let http = http::http_client()?;
        Ok(Self::with(
            Inner::Gdrive(gdrive::GdriveSession::new(config, http)),
            false,
        ))
    }

    /// S3-compatible storage with static keys (Cloudflare R2 for the first test, D31).
    ///
    /// # Errors
    /// `Permanent` for an endpoint that isn't `https://`, or if the HTTP client can't be built.
    pub fn s3(config: &S3Config) -> Result<Self, StoreError> {
        let op = s3::operator(config, http::http_client()?)?;
        let native = op.info().capability().write_with_if_not_exists;
        Ok(Self::with(Inner::Plain(op), native))
    }

    /// The operator to use now. Drive's is rebuilt before its access token expires (D27).
    async fn op(&self) -> Result<Operator, StoreError> {
        match &self.inner {
            Inner::Plain(op) => Ok(op.clone()),
            Inner::Gdrive(session) => session.operator().await,
        }
    }

    /// # Errors
    /// `NotFound`, or any classified backend error.
    pub async fn get(&self, path: &str) -> Result<Vec<u8>, StoreError> {
        check_path(path)?;
        retry(&self.policy, &self.watch, "get", |_| async move {
            let after = self.fault().await?;
            let bytes = self.op().await?.read(path).await?.to_vec();
            self.fault_after(after)?;
            Ok(bytes)
        })
        .await
    }

    /// `CreateOnly` uses the backend's conditional write, or stat-then-write where there is none
    /// (safe because names are unique, D5). A retried `CreateOnly` that finds the name taken reads
    /// it back: the same bytes mean an earlier try landed.
    ///
    /// # Errors
    /// `AlreadyExists` for `CreateOnly` on an existing name, or any classified backend error.
    pub async fn put(&self, path: &str, bytes: Vec<u8>, mode: PutMode) -> Result<(), StoreError> {
        check_path(path)?;
        let bytes = Buffer::from(bytes);
        retry(&self.policy, &self.watch, "put", |attempt| {
            let bytes = bytes.clone();
            async move {
                match self.put_once(path, bytes.clone(), mode).await {
                    Err(e) if attempt > 0 && e.class() == ErrorClass::AlreadyExists => {
                        self.read_back(path, &bytes, e).await
                    }
                    other => other,
                }
            }
        })
        .await
    }

    async fn put_once(&self, path: &str, bytes: Buffer, mode: PutMode) -> Result<(), StoreError> {
        let after = self.fault().await?;
        let op = self.op().await?;
        match mode {
            PutMode::Overwrite => {
                op.write(path, bytes).await?;
            }
            PutMode::CreateOnly if self.caps.native_create_only => {
                op.write_with(path, bytes).if_not_exists(true).await?;
            }
            PutMode::CreateOnly => {
                if op.exists(path).await? {
                    return Err(StoreError::new(ErrorClass::AlreadyExists, path));
                }
                op.write(path, bytes).await?;
            }
        }
        self.fault_after(after)
    }

    /// After a lost response: our bytes there → `Ok`, other bytes or nothing → `exists`.
    async fn read_back(
        &self,
        path: &str,
        ours: &Buffer,
        exists: StoreError,
    ) -> Result<(), StoreError> {
        #[cfg(any(test, feature = "faults"))]
        self.faults.count_read();
        match self.op().await?.read(path).await {
            Ok(found) if found.to_bytes() == ours.to_bytes() => Ok(()),
            Ok(_) => Err(exists),
            Err(e) if e.kind() == opendal::ErrorKind::NotFound => Err(exists),
            Err(e) => Err(e.into()),
        }
    }

    /// Files directly under `prefix`. The prefix is a valid object path that ends with `/`; the
    /// store root itself (`""`) can't be listed, because the engine only lists named folders.
    ///
    /// # Errors
    /// `Permanent` for a prefix without a trailing `/`. Any classified backend error except `NotFound`, which gives an empty list.
    pub async fn list(&self, prefix: &str) -> Result<Vec<Entry>, StoreError> {
        check_path(prefix)?;
        if !prefix.ends_with('/') {
            return Err(StoreError::new(
                ErrorClass::Permanent,
                "list prefix must end with /",
            ));
        }
        retry(&self.policy, &self.watch, "list", |_| async move {
            let after = self.fault().await?;
            let listed = self.op().await?.list(prefix).await;
            self.fault_after(after)?;
            match listed {
                Ok(entries) => Ok(entries
                    .into_iter()
                    .filter(|e| e.metadata().is_file())
                    .map(|e| Entry {
                        path: e.path().to_owned(),
                        size: e.metadata().content_length(),
                        modified: modified(e.metadata()),
                    })
                    .collect()),
                Err(e) if e.kind() == opendal::ErrorKind::NotFound => Ok(Vec::new()),
                Err(e) => Err(e.into()),
            }
        })
        .await
    }

    /// # Errors
    /// Any classified backend error except `NotFound`, which gives `None`.
    pub async fn stat(&self, path: &str) -> Result<Option<Entry>, StoreError> {
        check_path(path)?;
        retry(&self.policy, &self.watch, "stat", |_| async move {
            let after = self.fault().await?;
            let found = self.op().await?.stat(path).await;
            self.fault_after(after)?;
            match found {
                Ok(meta) => Ok(Some(Entry {
                    path: path.to_owned(),
                    size: meta.content_length(),
                    modified: modified(&meta),
                })),
                Err(e) if e.kind() == opendal::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.into()),
            }
        })
        .await
    }

    /// Idempotent. On Google Drive this moves the file to the trash (V-STORE-9).
    ///
    /// # Errors
    /// `Permanent` for a path ending in `/` (folders aren't objects). Any classified backend error.
    pub async fn delete(&self, path: &str) -> Result<(), StoreError> {
        check_path(path)?;
        if path.ends_with('/') {
            return Err(StoreError::new(
                ErrorClass::Permanent,
                "delete takes an object, not a folder",
            ));
        }
        retry(&self.policy, &self.watch, "delete", |_| async move {
            let after = self.fault().await?;
            self.op().await?.delete(path).await?;
            self.fault_after(after)
        })
        .await
    }

    /// Test failpoint before an attempt: `Err` to fail now, `true` to fail after the real call.
    /// A slow link (`Faults::delay`) waits here first.
    #[cfg(any(test, feature = "faults"))]
    async fn fault(&self) -> Result<bool, StoreError> {
        let wait = self.faults.wait();
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        self.faults.before()
    }

    #[cfg(not(any(test, feature = "faults")))]
    #[allow(
        clippy::unnecessary_wraps,
        clippy::unused_self,
        clippy::unused_async,
        clippy::unused_async_trait_impl
    )]
    async fn fault(&self) -> Result<bool, StoreError> {
        Ok(false)
    }

    #[allow(clippy::unused_self)]
    fn fault_after(&self, after: bool) -> Result<(), StoreError> {
        if after {
            Err(StoreError::new(ErrorClass::Transient, "injected"))
        } else {
            Ok(())
        }
    }
}

fn modified(meta: &opendal::Metadata) -> Option<std::time::SystemTime> {
    meta.last_modified().map(std::time::SystemTime::from)
}

fn check_path(path: &str) -> Result<(), StoreError> {
    let ok = !path.is_empty()
        && path.len() <= 512
        && !path.starts_with('/')
        && path.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'/' | b'-')
        })
        && path
            .strip_suffix('/')
            .unwrap_or(path)
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != "..");
    if ok {
        Ok(())
    } else {
        Err(StoreError::new(
            ErrorClass::Permanent,
            "invalid object path",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::faults::Fault;
    use super::*;
    use std::time::Duration;

    fn fast_store(dir: &Path) -> Store {
        let mut store = Store::fs(dir).unwrap();
        store.policy = RetryPolicy {
            attempts: 5,
            base: Duration::from_millis(1),
            cap: Duration::from_millis(4),
        };
        store
    }

    #[tokio::test]
    async fn a_transient_put_is_retried() {
        let dir = tempfile::tempdir().unwrap();
        let store = fast_store(dir.path());
        store.faults.push(Fault::Transient, 1);
        store
            .put("a/b", b"hi".to_vec(), PutMode::Overwrite)
            .await
            .unwrap();
        assert_eq!(store.get("a/b").await.unwrap(), b"hi");
    }

    #[tokio::test]
    async fn a_create_only_whose_response_was_lost_reads_back_our_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let store = fast_store(dir.path());
        store.faults.push(Fault::TransientAfterWrite, 1);
        store
            .put("a/b", b"ours".to_vec(), PutMode::CreateOnly)
            .await
            .unwrap();
        assert_eq!(store.faults.reads(), 1);
        assert_eq!(store.get("a/b").await.unwrap(), b"ours");
    }

    #[tokio::test]
    async fn a_retried_create_only_on_other_bytes_is_already_exists() {
        let dir = tempfile::tempdir().unwrap();
        let store = fast_store(dir.path());
        store
            .put("a/b", b"theirs".to_vec(), PutMode::CreateOnly)
            .await
            .unwrap();
        store.faults.push(Fault::Transient, 1);
        let e = store
            .put("a/b", b"ours".to_vec(), PutMode::CreateOnly)
            .await
            .unwrap_err();
        assert_eq!(e.class(), ErrorClass::AlreadyExists);
        assert_eq!(store.faults.reads(), 1);
        assert_eq!(store.get("a/b").await.unwrap(), b"theirs");
    }

    #[tokio::test]
    async fn a_first_try_create_only_on_an_existing_name_reads_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = fast_store(dir.path());
        store
            .put("a/b", b"ours".to_vec(), PutMode::CreateOnly)
            .await
            .unwrap();
        let e = store
            .put("a/b", b"ours".to_vec(), PutMode::CreateOnly)
            .await
            .unwrap_err();
        assert_eq!(e.class(), ErrorClass::AlreadyExists);
        assert_eq!(store.faults.reads(), 0);
    }

    #[tokio::test]
    async fn get_survives_four_transient_failures_but_not_five() {
        let dir = tempfile::tempdir().unwrap();
        let store = fast_store(dir.path());
        store
            .put("a/b", b"hi".to_vec(), PutMode::CreateOnly)
            .await
            .unwrap();
        store.faults.push(Fault::Transient, 4);
        assert_eq!(store.get("a/b").await.unwrap(), b"hi");
        store.faults.push(Fault::Transient, 5);
        assert_eq!(
            store.get("a/b").await.unwrap_err().class(),
            ErrorClass::Transient
        );
    }

    #[tokio::test]
    async fn a_transient_list_is_retried() {
        let dir = tempfile::tempdir().unwrap();
        let store = fast_store(dir.path());
        store
            .put("a/b", b"hi".to_vec(), PutMode::CreateOnly)
            .await
            .unwrap();
        store.faults.push(Fault::Transient, 2);
        let listed = store.list("a/").await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(store.faults.requests(), 4, "put, two failures, the list");
    }

    #[tokio::test]
    async fn an_outage_shorter_than_the_budget_heals_by_itself() {
        let dir = tempfile::tempdir().unwrap();
        let store = fast_store(dir.path());
        store.faults.outage(2, 3);
        store
            .put("a/b", b"hi".to_vec(), PutMode::Overwrite)
            .await
            .unwrap();
        assert_eq!(store.get("a/b").await.unwrap(), b"hi");
        assert_eq!(store.faults.requests(), 5, "put, three failures, the get");
        assert_eq!(store.get("a/b").await.unwrap(), b"hi", "healed");
    }

    #[tokio::test]
    async fn an_outage_longer_than_the_budget_is_transient_then_heals() {
        let dir = tempfile::tempdir().unwrap();
        let store = fast_store(dir.path());
        store.faults.outage(1, 5);
        let e = store.stat("a").await.unwrap_err();
        assert_eq!(e.class(), ErrorClass::Transient);
        assert_eq!(store.stat("a").await.unwrap(), None, "healed");
    }

    #[tokio::test(start_paused = true)]
    async fn a_slow_link_waits_before_every_request() {
        let dir = tempfile::tempdir().unwrap();
        let store = fast_store(dir.path());
        store.faults.delay(Duration::from_secs(2));
        let start = tokio::time::Instant::now();
        store
            .put("a/b", b"hi".to_vec(), PutMode::Overwrite)
            .await
            .unwrap();
        assert_eq!(store.get("a/b").await.unwrap(), b"hi");
        assert!(start.elapsed() >= Duration::from_secs(4));
        store.faults.heal();
        let start = tokio::time::Instant::now();
        store.get("a/b").await.unwrap();
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn the_observer_hears_each_retry_then_the_recovery() {
        use std::sync::Mutex;
        let dir = tempfile::tempdir().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let store = fast_store(dir.path()).with_retry_watch(RetryWatch::new(Arc::new(move |n| {
            s.lock().unwrap().push(n);
        })));
        store.faults.push(Fault::Transient, 2);
        store
            .put("a/b", b"hi".to_vec(), PutMode::Overwrite)
            .await
            .unwrap();
        store.get("a/b").await.unwrap();
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 3, "{seen:?}");
        for (i, n) in seen[..2].iter().enumerate() {
            let RetryNotice::Retrying {
                operation,
                attempt,
                max_attempts,
                delay,
            } = n
            else {
                panic!("{n:?}")
            };
            assert_eq!(*operation, "put");
            assert_eq!(*attempt, u32::try_from(i).unwrap() + 2);
            assert_eq!(*max_attempts, 5);
            assert!(*delay <= Duration::from_millis(4));
        }
        assert_eq!(
            seen[2],
            RetryNotice::Recovered,
            "once, not again on the get"
        );
    }

    #[tokio::test]
    async fn after_giving_up_the_next_success_is_a_recovery() {
        use std::sync::Mutex;
        let dir = tempfile::tempdir().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let store = fast_store(dir.path()).with_retry_watch(RetryWatch::new(Arc::new(move |n| {
            s.lock().unwrap().push(n);
        })));
        store.faults.push(Fault::Transient, 5);
        assert!(store.stat("a").await.is_err());
        assert_eq!(seen.lock().unwrap().len(), 4);
        store.stat("a").await.unwrap();
        assert_eq!(seen.lock().unwrap().last(), Some(&RetryNotice::Recovered));
    }

    #[test]
    fn check_path_accepts_object_names() {
        for ok in [
            "a",
            "lemusync/streams/ab-cd_1.2/versions/",
            "x".repeat(512).as_str(),
        ] {
            assert!(check_path(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn check_path_rejects_everything_else() {
        let long = "x".repeat(513);
        for bad in [
            "",
            "/abs",
            "Upper",
            "a/../b",
            "..",
            "a//b",
            "a b",
            "caf\u{e9}",
            long.as_str(),
        ] {
            let e = check_path(bad).unwrap_err();
            assert_eq!(e.class(), ErrorClass::Permanent, "{bad}");
        }
    }

    #[tokio::test]
    async fn list_needs_a_trailing_slash_and_delete_forbids_one() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::fs(dir.path()).unwrap();
        for bad in ["a", "", "/"] {
            assert_eq!(
                store.list(bad).await.unwrap_err().class(),
                ErrorClass::Permanent,
                "{bad:?}"
            );
        }
        assert_eq!(
            store.delete("a/").await.unwrap_err().class(),
            ErrorClass::Permanent
        );
    }
}
