//! Storage backend interface and adapters. Moves opaque encrypted objects.
//! Spec: docs/specs/storage-backends.md.

mod error;
mod gdrive;
pub mod http;
pub mod oauth;
mod redact;
mod s3;

use std::path::Path;
use std::sync::Arc;

use opendal::Operator;
use opendal::services::Fs;

pub use error::{ErrorClass, StoreError};
pub use gdrive::GdriveConfig;
pub use redact::redact;
pub use s3::S3Config;

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub native_create_only: bool,
}

#[derive(Clone)]
pub struct Store {
    inner: Inner,
    caps: Capabilities,
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
        Ok(Self {
            inner: Inner::Plain(op),
            caps: Capabilities {
                native_create_only: native,
            },
        })
    }

    #[must_use]
    pub fn capabilities(&self) -> Capabilities {
        self.caps
    }

    /// Google Drive with the user's OAuth client and refresh token (D26, D27).
    ///
    /// # Errors
    /// `Permanent` if the HTTP client can't be built.
    pub fn gdrive(config: GdriveConfig) -> Result<Self, StoreError> {
        let http = http::http_client()?;
        Ok(Self {
            inner: Inner::Gdrive(gdrive::GdriveSession::new(config, http)),
            caps: Capabilities {
                native_create_only: false,
            },
        })
    }

    /// S3-compatible storage with static keys (Cloudflare R2 for the first test, D31).
    ///
    /// # Errors
    /// `Permanent` for an endpoint that isn't `https://`, or if the HTTP client can't be built.
    pub fn s3(config: &S3Config) -> Result<Self, StoreError> {
        let op = s3::operator(config, http::http_client()?)?;
        let native = op.info().capability().write_with_if_not_exists;
        Ok(Self {
            inner: Inner::Plain(op),
            caps: Capabilities {
                native_create_only: native,
            },
        })
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
        Ok(self.op().await?.read(path).await?.to_vec())
    }

    /// `CreateOnly` uses the backend's conditional write, or stat-then-write where there is none
    /// (safe because names are unique, D5).
    ///
    /// # Errors
    /// `AlreadyExists` for `CreateOnly` on an existing name, or any classified backend error.
    pub async fn put(&self, path: &str, bytes: Vec<u8>, mode: PutMode) -> Result<(), StoreError> {
        check_path(path)?;
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
        Ok(())
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
        let op = self.op().await?;
        match op.list(prefix).await {
            Ok(entries) => Ok(entries
                .into_iter()
                .filter(|e| e.metadata().is_file())
                .map(|e| Entry {
                    path: e.path().to_owned(),
                    size: e.metadata().content_length(),
                })
                .collect()),
            Err(e) if e.kind() == opendal::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.into()),
        }
    }

    /// # Errors
    /// Any classified backend error except `NotFound`, which gives `None`.
    pub async fn stat(&self, path: &str) -> Result<Option<Entry>, StoreError> {
        check_path(path)?;
        match self.op().await?.stat(path).await {
            Ok(meta) => Ok(Some(Entry {
                path: path.to_owned(),
                size: meta.content_length(),
            })),
            Err(e) if e.kind() == opendal::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
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
        Ok(self.op().await?.delete(path).await?)
    }
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
    use super::*;

    #[test]
    fn check_path_accepts_object_names() {
        for ok in [
            "a",
            "leemusync/streams/ab-cd_1.2/versions/",
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
