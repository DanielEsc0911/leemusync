//! S3-compatible storage (AWS S3, Cloudflare R2, B2, …) through `OpenDAL` with static keys.

use opendal::services::S3;
use opendal::{HttpTransporter, Operator};
use opendal_http_transport_reqwest::ReqwestTransport;
use zeroize::Zeroizing;

use crate::{ErrorClass, StoreError};

pub struct S3Config {
    /// `https://…`, e.g. `https://<account id>.r2.cloudflarestorage.com` for R2.
    pub endpoint: String,
    pub bucket: String,
    /// `auto` for R2.
    pub region: String,
    /// Folder inside the bucket, e.g. `/`. The repo itself lives under `leemusync/`.
    pub root: String,
    pub access_key_id: String,
    pub secret_access_key: Zeroizing<String>,
}

/// Static keys only: no `AWS_*` variables, no `~/.aws` files, no instance metadata (B2).
pub(crate) fn operator(config: &S3Config, http: reqwest::Client) -> Result<Operator, StoreError> {
    if !config.endpoint.starts_with("https://") {
        return Err(StoreError::new(
            ErrorClass::Permanent,
            "the S3 endpoint must start with https://",
        ));
    }
    let op = Operator::new(
        S3::default()
            .endpoint(&config.endpoint)
            .bucket(&config.bucket)
            .region(&config.region)
            .root(&config.root)
            .access_key_id(&config.access_key_id)
            .secret_access_key(&config.secret_access_key)
            .disable_config_load()
            .disable_ec2_metadata(),
    )?;
    let transport = HttpTransporter::new(ReqwestTransport::new(http));
    let ctx = op.base_context().with_http_transport(transport);
    Ok(op.with_context(ctx))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(endpoint: &str) -> S3Config {
        S3Config {
            endpoint: endpoint.into(),
            bucket: "saves".into(),
            region: "auto".into(),
            root: "/".into(),
            access_key_id: "id".into(),
            secret_access_key: Zeroizing::new("secret".into()),
        }
    }

    #[test]
    fn needs_https() {
        let http = crate::http::http_client().unwrap();
        let e = operator(&config("http://localhost:9000"), http).unwrap_err();
        assert_eq!(e.class(), ErrorClass::Permanent);
    }

    #[test]
    fn has_native_create_only() {
        let http = crate::http::http_client().unwrap();
        let op = operator(&config("https://example.r2.cloudflarestorage.com"), http).unwrap();
        assert_eq!(op.info().scheme(), "s3");
        assert!(op.info().capability().write_with_if_not_exists);
    }
}
