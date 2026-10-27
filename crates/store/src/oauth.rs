//! Google OAuth for Drive (`drive.file`): authorization code + PKCE with a loopback redirect (D28),
//! and token refresh with a form body so secrets never enter a URL (D27).

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use zeroize::Zeroizing;

use crate::{ErrorClass, StoreError};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REQUEST: usize = 8 * 1024;
const DONE_PAGE: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n<!doctype html><meta charset=utf-8><title>LemuSync</title><p>LemuSync: you can close this tab.</p><p>LemuSync: ya puedes cerrar esta pestaña.</p>";

/// The user's own OAuth client (D26). `Debug` never shows the secret.
pub struct OAuthClient {
    pub client_id: String,
    pub client_secret: Option<Zeroizing<String>>,
}

impl fmt::Debug for OAuthClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthClient")
            .field("client_id", &self.client_id)
            .finish_non_exhaustive()
    }
}

pub struct AccessToken {
    pub token: Zeroizing<String>,
    pub expires_at_ms: u64,
}

pub struct Tokens {
    pub refresh_token: Zeroizing<String>,
    pub access: AccessToken,
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessToken")
            .field("expires_at_ms", &self.expires_at_ms)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Tokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tokens")
            .field("access", &self.access)
            .finish_non_exhaustive()
    }
}

/// A bound loopback listener waiting for Google's one redirect (D28).
pub struct PendingLogin {
    listener: TcpListener,
    state: String,
    verifier: Zeroizing<String>,
    pub(crate) redirect: String,
    url: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    refresh_token: Option<String>,
}

fn store_err(class: ErrorClass, text: &str) -> StoreError {
    StoreError::new(class, text)
}

fn random_b64(n: usize) -> Result<String, StoreError> {
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes)
        .map_err(|_| store_err(ErrorClass::Permanent, "random number generator failed"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub(crate) fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn expires_at(now_ms: u64, expires_in_s: u64) -> u64 {
    now_ms.saturating_add(expires_in_s.saturating_mul(1000))
}

/// Binds `127.0.0.1:<random>` and builds the consent URL. Open the URL, then call `finish`.
///
/// # Errors
/// `Permanent` if the loopback port can't be bound.
pub async fn start_login(client: &OAuthClient) -> Result<PendingLogin, StoreError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| store_err(ErrorClass::Permanent, &e.to_string()))?;
    let port = listener
        .local_addr()
        .map_err(|e| store_err(ErrorClass::Permanent, &e.to_string()))?
        .port();
    let redirect = format!("http://127.0.0.1:{port}");
    let state = random_b64(16)?;
    let verifier = Zeroizing::new(random_b64(32)?);
    let url = url::Url::parse_with_params(
        AUTH_URL,
        &[
            ("client_id", client.client_id.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("response_type", "code"),
            ("scope", SCOPE),
            ("code_challenge", challenge(&verifier).as_str()),
            ("code_challenge_method", "S256"),
            ("state", state.as_str()),
            ("access_type", "offline"),
            ("prompt", "consent"),
        ],
    )
    .map_err(|e| store_err(ErrorClass::Permanent, &e.to_string()))?
    .to_string();
    Ok(PendingLogin {
        listener,
        state,
        verifier,
        redirect,
        url,
    })
}

impl PendingLogin {
    /// The consent URL to open in the browser. It holds no secret.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Accepts exactly one redirect (5 minutes at most), closes the port, then exchanges the code.
    ///
    /// # Errors
    /// `Auth` for a timeout, a wrong `state`, a refusal or a missing refresh token; else classified HTTP errors.
    pub async fn finish(
        self,
        client: &OAuthClient,
        http: &reqwest::Client,
    ) -> Result<Tokens, StoreError> {
        self.finish_within(client, http, LOGIN_TIMEOUT).await
    }

    async fn finish_within(
        self,
        client: &OAuthClient,
        http: &reqwest::Client,
        wait: Duration,
    ) -> Result<Tokens, StoreError> {
        let Self {
            listener,
            state: expected,
            verifier,
            redirect,
            ..
        } = self;
        let accepted = tokio::time::timeout(wait, listener.accept()).await;
        // One request only: the port closes before anything else happens (D28).
        drop(listener);
        let (mut socket, _) = accepted
            .map_err(|_| store_err(ErrorClass::Auth, "login timed out"))?
            .map_err(|e| store_err(ErrorClass::Permanent, &e.to_string()))?;
        let mut buf = Zeroizing::new(Vec::new());
        let mut chunk = Zeroizing::new([0u8; 1024]);
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < MAX_REQUEST {
            let n = tokio::time::timeout(READ_TIMEOUT, socket.read(&mut chunk[..]))
                .await
                .map_err(|_| store_err(ErrorClass::Auth, "login redirect timed out"))?
                .map_err(|e| store_err(ErrorClass::Permanent, &e.to_string()))?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let _ = socket.write_all(DONE_PAGE.as_bytes()).await;
        let _ = socket.shutdown().await;
        drop(socket);
        let code = parse_redirect(&buf, &redirect, &expected)?;
        let mut form = vec![
            ("code", code.as_str()),
            ("client_id", client.client_id.as_str()),
            ("code_verifier", verifier.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect.as_str()),
        ];
        if let Some(secret) = &client.client_secret {
            form.push(("client_secret", secret.as_str()));
        }
        let response = post_token(http, &form).await?;
        let refresh = response
            .refresh_token
            .ok_or_else(|| store_err(ErrorClass::Auth, "Google returned no refresh token"))?;
        Ok(Tokens {
            refresh_token: Zeroizing::new(refresh),
            access: AccessToken {
                token: Zeroizing::new(response.access_token),
                expires_at_ms: expires_at(now_ms(), response.expires_in),
            },
        })
    }
}

/// The authorization code from the redirect, once its `state` matches exactly.
fn parse_redirect(
    request: &[u8],
    redirect: &str,
    expected_state: &str,
) -> Result<Zeroizing<String>, StoreError> {
    let unexpected = || store_err(ErrorClass::Auth, "unexpected login redirect");
    let request = Zeroizing::new(String::from_utf8_lossy(request).into_owned());
    let target = request
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("GET /"))
        .and_then(|l| l.split(' ').next())
        .ok_or_else(unexpected)?;
    let url = url::Url::parse(&format!("{redirect}/{target}")).map_err(|_| unexpected())?;
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(Zeroizing::new(v.into_owned())),
            "state" => state = Some(v.into_owned()),
            "error" => error = Some(v.into_owned()),
            _ => {}
        }
    }
    if state.as_deref() != Some(expected_state) {
        return Err(store_err(ErrorClass::Auth, "login state didn't match"));
    }
    if let Some(error) = error {
        return Err(store_err(
            ErrorClass::Auth,
            &format!("Google refused the login: {error}"),
        ));
    }
    code.ok_or_else(|| store_err(ErrorClass::Auth, "login redirect had no code"))
}

/// A fresh access token from the refresh token. The secrets travel only in the POST body.
///
/// # Errors
/// `Auth` when Google rejects the refresh token (`invalid_grant`, e.g. the 7-day Testing expiry);
/// `Transient` for network trouble, 429 and 5xx; else `Permanent`.
pub async fn refresh(
    client: &OAuthClient,
    http: &reqwest::Client,
    refresh_token: &str,
) -> Result<AccessToken, StoreError> {
    let mut form = vec![
        ("refresh_token", refresh_token),
        ("client_id", client.client_id.as_str()),
        ("grant_type", "refresh_token"),
    ];
    if let Some(secret) = &client.client_secret {
        form.push(("client_secret", secret.as_str()));
    }
    let response = post_token(http, &form).await?;
    Ok(AccessToken {
        token: Zeroizing::new(response.access_token),
        expires_at_ms: expires_at(now_ms(), response.expires_in),
    })
}

async fn post_token(
    http: &reqwest::Client,
    form: &[(&str, &str)],
) -> Result<TokenResponse, StoreError> {
    let response = http.post(TOKEN_URL).form(form).send().await.map_err(|e| {
        let class = if e.is_timeout() || e.is_connect() {
            ErrorClass::Transient
        } else {
            ErrorClass::Permanent
        };
        store_err(class, &e.to_string())
    })?;
    let status = response.status().as_u16();
    let body = Zeroizing::new(
        response
            .text()
            .await
            .map_err(|e| store_err(ErrorClass::Transient, &e.to_string()))?,
    );
    token_response(status, &body)
}

/// Classifies the token endpoint's answer. Error text is redacted by `StoreError::new`.
fn token_response(status: u16, body: &str) -> Result<TokenResponse, StoreError> {
    if !(200..300).contains(&status) {
        let class = match status {
            400 | 401 => ErrorClass::Auth,
            429 | 500..=599 => ErrorClass::Transient,
            _ => ErrorClass::Permanent,
        };
        return Err(store_err(
            class,
            &format!("token endpoint returned {status}: {body}"),
        ));
    }
    serde_json::from_str(body).map_err(|_| {
        store_err(
            ErrorClass::Permanent,
            "token endpoint returned unexpected JSON",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> OAuthClient {
        OAuthClient {
            client_id: "id.apps.googleusercontent.com".into(),
            client_secret: Some(Zeroizing::new("s3cret".into())),
        }
    }

    /// Sends one raw HTTP request to the pending login's loopback port and reads the reply.
    fn hit(redirect: &str, request: &'static [u8]) -> tokio::task::JoinHandle<Vec<u8>> {
        let url = url::Url::parse(redirect).unwrap();
        let addr = (url.host_str().unwrap().to_owned(), url.port().unwrap());
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
            s.write_all(request).await.unwrap();
            let mut buf = Vec::new();
            let _ = s.read_to_end(&mut buf).await;
            buf
        })
    }

    #[test]
    fn pkce_challenge_is_s256_of_the_verifier() {
        // RFC 7636 appendix B.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[tokio::test]
    async fn login_url_asks_for_drive_file_only_and_offline_access() {
        let pending = start_login(&client()).await.unwrap();
        let url = url::Url::parse(pending.url()).unwrap();
        let q: std::collections::BTreeMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["scope"], "https://www.googleapis.com/auth/drive.file");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["access_type"], "offline");
        assert!(q["redirect_uri"].starts_with("http://127.0.0.1:"));
        assert!(
            !pending.url().contains("s3cret"),
            "the secret never goes in the browser URL"
        );
    }

    #[tokio::test]
    async fn the_listener_binds_loopback_only() {
        let pending = start_login(&client()).await.unwrap();
        let addr = pending.listener.local_addr().unwrap();
        assert!(addr.ip().is_loopback(), "{addr}");
        assert_ne!(addr.port(), 0);
        let other = start_login(&client()).await.unwrap();
        assert_ne!(pending.state, other.state, "state is random per login");
        assert_ne!(*pending.verifier, *other.verifier);
    }

    #[tokio::test]
    async fn the_listener_rejects_a_wrong_state() {
        let pending = start_login(&client()).await.unwrap();
        let redirect = pending.redirect.clone();
        let reply = hit(
            &redirect,
            b"GET /?code=abc&state=WRONG HTTP/1.1\r\nHost: x\r\n\r\n",
        );
        let http = crate::http::http_client().unwrap();
        let err = pending.finish(&client(), &http).await.unwrap_err();
        assert_eq!(err.class(), ErrorClass::Auth);
        assert!(!reply.await.unwrap().is_empty(), "the browser gets a page");
        // One request only: the port is closed afterwards.
        let url = url::Url::parse(&redirect).unwrap();
        assert!(
            tokio::net::TcpStream::connect(("127.0.0.1", url.port().unwrap()))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_missing_state_or_an_error_is_auth() {
        for request in [
            &b"GET /?code=abc HTTP/1.1\r\nHost: x\r\n\r\n"[..],
            &b"GET /?error=access_denied HTTP/1.1\r\nHost: x\r\n\r\n"[..],
            &b"POST / HTTP/1.1\r\nHost: x\r\n\r\n"[..],
        ] {
            let pending = start_login(&client()).await.unwrap();
            let reply = hit(&pending.redirect.clone(), request);
            let http = crate::http::http_client().unwrap();
            let err = pending.finish(&client(), &http).await.unwrap_err();
            assert_eq!(err.class(), ErrorClass::Auth, "{err}");
            reply.await.unwrap();
        }
    }

    #[tokio::test]
    async fn a_refusal_with_the_right_state_is_auth() {
        let pending = start_login(&client()).await.unwrap();
        let request = format!(
            "GET /?error=access_denied&state={} HTTP/1.1\r\nHost: x\r\n\r\n",
            pending.state
        );
        let request: &'static [u8] = Box::leak(request.into_bytes().into_boxed_slice());
        let reply = hit(&pending.redirect.clone(), request);
        let http = crate::http::http_client().unwrap();
        let err = pending.finish(&client(), &http).await.unwrap_err();
        assert_eq!(err.class(), ErrorClass::Auth);
        assert!(err.message().contains("refused"), "{err}");
        reply.await.unwrap();
    }

    #[tokio::test]
    async fn the_login_times_out_and_closes_the_port() {
        let pending = start_login(&client()).await.unwrap();
        let port = pending.listener.local_addr().unwrap().port();
        let http = crate::http::http_client().unwrap();
        let err = pending
            .finish_within(&client(), &http, Duration::from_millis(50))
            .await
            .unwrap_err();
        assert_eq!(err.class(), ErrorClass::Auth);
        assert!(err.message().contains("timed out"), "{err}");
        assert!(
            tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .is_err()
        );
    }

    #[test]
    fn the_real_login_waits_five_minutes() {
        assert_eq!(LOGIN_TIMEOUT, Duration::from_secs(300));
    }

    #[test]
    fn token_responses_are_classified_and_redacted() {
        let ok = token_response(200, r#"{"access_token":"ya29.x","expires_in":3599,"refresh_token":"1//r","token_type":"Bearer"}"#).unwrap();
        assert_eq!(ok.access_token, "ya29.x");
        assert_eq!(ok.expires_in, 3599);
        assert_eq!(ok.refresh_token.as_deref(), Some("1//r"));

        let bad = token_response(400, r#"{"error":"invalid_grant","refresh_token":"LEAKED"}"#)
            .err()
            .unwrap();
        assert_eq!(bad.class(), ErrorClass::Auth);
        assert!(bad.message().contains("invalid_grant"), "{bad}");
        assert!(!bad.message().contains("LEAKED"), "{bad}");

        assert_eq!(
            token_response(503, "busy").err().unwrap().class(),
            ErrorClass::Transient
        );
        assert_eq!(
            token_response(429, "slow down").err().unwrap().class(),
            ErrorClass::Transient
        );
        assert_eq!(
            token_response(403, "no").err().unwrap().class(),
            ErrorClass::Permanent
        );
        let garbage = token_response(200, r#"{"access_token":"SECRET"#)
            .err()
            .unwrap();
        assert_eq!(garbage.class(), ErrorClass::Permanent);
        assert!(!garbage.message().contains("SECRET"));
    }

    #[test]
    fn expiry_never_overflows() {
        assert_eq!(expires_at(u64::MAX - 5, u64::MAX), u64::MAX);
        assert_eq!(expires_at(1_000, 2), 3_000);
    }

    #[test]
    fn debug_never_shows_secrets() {
        let text = format!("{:?}", client());
        assert!(!text.contains("s3cret"));
        let tokens = Tokens {
            refresh_token: Zeroizing::new("1//REFRESH".into()),
            access: AccessToken {
                token: Zeroizing::new("ya29.ACCESS".into()),
                expires_at_ms: 7,
            },
        };
        let text = format!("{tokens:?}");
        assert!(
            !text.contains("REFRESH") && !text.contains("ACCESS"),
            "{text}"
        );
    }
}
