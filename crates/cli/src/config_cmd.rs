//! `lemusync config path | list | get | set | unset` (config-and-cli §Commands).

use clap::Subcommand;
use lemusync_engine::EngineError;
use serde_json::{Value, json};

use crate::daemon_client::Api;
use crate::i18n::{I18n, s};
use crate::output;

#[derive(Subcommand)]
pub enum ConfigCmd {
    Path,
    List,
    Get {
        key: String,
    },
    Set {
        key: String,
        #[arg(required = true, num_args = 1..)]
        values: Vec<String>,
    },
    Unset {
        key: String,
    },
}

/// Characters of one value shown at most.
const SHOWN_MAX: usize = 1000;

/// A value for the terminal: strings bare, the rest as JSON; control characters stripped (B2).
fn shown(v: &Value) -> String {
    let text = match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(SHOWN_MAX)
        .collect()
}

/// # Errors
/// What the engine returns for the key.
pub async fn run(eng: &mut Api, cmd: &ConfigCmd, t: &I18n, json: bool) -> Result<(), EngineError> {
    let api = lemusync_api::API_VERSION;
    match cmd {
        ConfigCmd::Path => {
            let path = eng.config_path().await?.display().to_string();
            if json {
                output::json(&json!({ "api_version": api, "path": path }));
            } else {
                println!("{}", shown(&Value::String(path)));
            }
        }
        ConfigCmd::List => {
            let values = eng.config_values().await?;
            if json {
                output::json(&json!({ "api_version": api, "values": values }));
            } else {
                for (k, v) in &values {
                    println!("{} = {}", shown(&Value::String(k.clone())), shown(v));
                }
            }
        }
        ConfigCmd::Get { key } => {
            let value = eng.config_get(key).await?;
            if json {
                output::json(&json!({ "api_version": api, "key": key, "value": value }));
            } else if value.is_null() {
                output::line(t, "config-unset-value", &[]);
            } else {
                println!("{}", shown(&value));
            }
        }
        ConfigCmd::Set { key, values } => {
            eng.config_set(key, values).await?;
            let value = eng.config_get(key).await?;
            if json {
                output::json(&json!({ "api_version": api, "key": key, "value": value }));
            } else {
                output::line(
                    t,
                    "config-set-done",
                    &[
                        ("key", s(shown(&Value::String(key.clone())))),
                        ("value", s(shown(&value))),
                    ],
                );
            }
        }
        ConfigCmd::Unset { key } => {
            eng.config_unset(key).await?;
            if json {
                output::json(
                    &json!({ "api_version": api, "key": key, "value": eng.config_get(key).await? }),
                );
            } else {
                output::line(
                    t,
                    "config-unset-done",
                    &[("key", s(shown(&Value::String(key.clone()))))],
                );
            }
        }
    }
    Ok(())
}
