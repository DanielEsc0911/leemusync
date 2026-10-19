//! One-time login for the live Drive test. Stores the refresh token and client secret in the OS
//! keystore (service `leemusync-test`). It never prints them. Usage:
//!   `LEEMUSYNC_TEST_GDRIVE_CLIENT_ID=<id> cargo run -p leemusync-store --example gdrive_login`

use std::io::BufRead as _;

use leemusync_store::oauth::{self, OAuthClient};
use zeroize::Zeroizing;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client_id = std::env::var("LEEMUSYNC_TEST_GDRIVE_CLIENT_ID")?;
    eprintln!("Paste the OAuth client secret and press Enter:");
    let mut secret = Zeroizing::new(String::new());
    std::io::stdin().lock().read_line(&mut secret)?;
    let secret = Zeroizing::new(secret.trim().to_owned());
    let client = OAuthClient {
        client_id,
        client_secret: Some(secret.clone()),
    };
    let http = leemusync_store::http::http_client()?;
    let pending = oauth::start_login(&client).await?;
    eprintln!("Open this URL in your browser:\n{}", pending.url());
    let tokens = pending.finish(&client, &http).await?;
    keyring::Entry::new("leemusync-test", "gdrive-refresh")?.set_password(&tokens.refresh_token)?;
    keyring::Entry::new("leemusync-test", "gdrive-client-secret")?.set_password(&secret)?;
    eprintln!(
        "Saved to the keystore. Run: LEEMUSYNC_TEST_GDRIVE_CLIENT_ID=<id> cargo test -p leemusync-store --test conformance -- --ignored"
    );
    Ok(())
}
