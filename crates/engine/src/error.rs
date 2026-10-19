//! Engine errors. Applications map `code()` to localised text (architecture §Engine internals).

use leemusync_store::{ErrorClass, StoreError};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("{0}")]
    Usage(String),
    #[error("invalid config: {0}")]
    Config(String),
    #[error("storage: {0}")]
    Store(#[from] StoreError),
    #[error("file system: {0}")]
    Io(String),
    #[error("crypto: {0}")]
    Crypto(#[from] leemusync_crypto::CryptoError),
    #[error("wrong passphrase or recovery key")]
    Unlock,
    #[error("another LeemuSync process is using {0}")]
    Locked(String),
    #[error("{0} is running; close it first")]
    EmulatorRunning(String),
    #[error("{0} is outside every granted folder")]
    NotGranted(String),
    #[error("not set up yet: {0}")]
    NotSetUp(&'static str),
    #[error("keystore: {0}")]
    Secrets(String),
    #[error("invalid data in the repo: {0}")]
    Data(String),
    #[error("the first sync on this device needs confirmation")]
    NeedsConfirmation,
    #[error("{0} changed while it was being read; try again when the emulator is idle")]
    Busy(String),
    #[error("fault injected at {0}")]
    Injected(&'static str),
}

impl EngineError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Usage(_) => "usage",
            Self::Config(_) => "config",
            Self::Store(e) if e.class() == ErrorClass::Auth => "backend_auth",
            Self::Store(_) => "backend",
            Self::Io(_) => "io",
            Self::Crypto(_) => "crypto",
            Self::Unlock => "unlock",
            Self::Locked(_) => "locked",
            Self::EmulatorRunning(_) => "emulator_running",
            Self::NotGranted(_) => "not_granted",
            Self::NotSetUp(_) => "not_set_up",
            Self::Secrets(_) => "secrets",
            Self::Data(_) => "data",
            Self::NeedsConfirmation => "needs_confirmation",
            Self::Busy(_) => "busy",
            Self::Injected(_) => "injected",
        }
    }

    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) | Self::NotSetUp(_) | Self::NeedsConfirmation => 2,
            Self::Config(_) => 3,
            Self::Store(e) if matches!(e.class(), ErrorClass::Auth | ErrorClass::Transient) => 6,
            _ => 1,
        }
    }
}

impl From<std::io::Error> for EngineError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<rusqlite::Error> for EngineError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Io(format!("state database: {e}"))
    }
}
