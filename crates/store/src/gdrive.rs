//! Google Drive through `OpenDAL` with an access token only; the store refreshes it (D27).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use opendal::services::Gdrive;
use opendal::{HttpTransporter, Operator};
use opendal_http_transport_reqwest::ReqwestTransport;
use tokio::sync::Mutex;
use zeroize::Zeroizing;

use crate::StoreError;
use crate::oauth::{self, OAuthClient};

/// Rebuild the operator this long before its access token expires.
const REFRESH_MARGIN_MS: u64 = 60_000;

pub struct GdriveConfig {
    /// Folder inside "My Drive", e.g. `/`. The repo itself lives under `lemusync/`.
    pub root: String,
    pub client: OAuthClient,
    pub refresh_token: Zeroizing<String>,
}

pub(crate) struct GdriveSession {
    config: GdriveConfig,
    http: reqwest::Client,
    current: Mutex<Option<(Operator, u64)>>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// True while a token that expires at `expires_at_ms` is still good for at least the margin.
fn still_fresh(expires_at_ms: u64, now_ms: u64) -> bool {
    expires_at_ms > now_ms.saturating_add(REFRESH_MARGIN_MS)
}

impl GdriveSession {
    pub(crate) fn new(config: GdriveConfig, http: reqwest::Client) -> Arc<Self> {
        Arc::new(Self {
            config,
            http,
            current: Mutex::new(None),
        })
    }

    /// The current operator, rebuilt with a fresh access token when the old one is about to expire.
    pub(crate) async fn operator(&self) -> Result<Operator, StoreError> {
        let mut current = self.current.lock().await;
        if let Some((op, expires)) = current.as_ref()
            && still_fresh(*expires, now_ms())
        {
            return Ok(op.clone());
        }
        let access =
            oauth::refresh(&self.config.client, &self.http, &self.config.refresh_token).await?;
        let op = Operator::new(
            Gdrive::default()
                .root(&self.config.root)
                .access_token(&access.token),
        )?;
        let transport = HttpTransporter::new(ReqwestTransport::new(self.http.clone()));
        let ctx = op.base_context().with_http_transport(transport);
        let op = op.with_context(ctx);
        *current = Some((op.clone(), access.expires_at_ms));
        Ok(op)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshes_a_minute_before_expiry() {
        let now = 1_000_000;
        assert!(still_fresh(now + REFRESH_MARGIN_MS + 1, now));
        assert!(!still_fresh(now + REFRESH_MARGIN_MS, now));
        assert!(!still_fresh(now, now));
        assert!(!still_fresh(0, u64::MAX), "no overflow");
    }

    #[tokio::test]
    async fn a_fresh_operator_is_reused_without_a_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let fs = Operator::new(opendal::services::Fs::default().root(dir.path().to_str().unwrap()))
            .unwrap();
        let session = GdriveSession::new(
            GdriveConfig {
                root: "/".into(),
                client: OAuthClient {
                    client_id: "id".into(),
                    client_secret: None,
                },
                refresh_token: Zeroizing::new("unused".into()),
            },
            crate::http::http_client().unwrap(),
        );
        *session.current.lock().await = Some((fs, u64::MAX));
        // A refresh would need the network and fail here; the cached operator comes back instead.
        let op = session.operator().await.unwrap();
        assert_eq!(op.info().scheme(), "fs");
    }
}
