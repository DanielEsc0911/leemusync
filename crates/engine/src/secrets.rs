//! Secrets live in the OS keystore only (security §Credentials).

use std::collections::BTreeMap;
use std::sync::Mutex;

use leemusync_core::DeviceId;
use zeroize::Zeroizing;

use crate::EngineError;

const SERVICE: &str = "leemusync";

pub trait SecretStore: Send + Sync {
    /// # Errors
    /// `Secrets` when the keystore is unavailable or locked.
    fn get(&self, name: &str) -> Result<Option<Zeroizing<String>>, EngineError>;
    /// # Errors
    /// `Secrets` when the keystore is unavailable or locked.
    fn set(&self, name: &str, value: &str) -> Result<(), EngineError>;
    /// Idempotent.
    ///
    /// # Errors
    /// `Secrets` when the keystore is unavailable or locked.
    fn delete(&self, name: &str) -> Result<(), EngineError>;
}

#[must_use]
#[allow(dead_code)] // used by Task 11
pub fn secret_master(repo: &str) -> String {
    format!("repo-{repo}-master-key")
}
#[must_use]
#[allow(dead_code)] // used by Task 11
pub fn secret_gdrive_refresh(device: DeviceId) -> String {
    format!("device-{device}-gdrive-refresh-token")
}
#[must_use]
#[allow(dead_code)] // used by Task 11
pub fn secret_gdrive_client(device: DeviceId) -> String {
    format!("device-{device}-gdrive-client-secret")
}

/// Secret Service on Linux (`KWallet` or GNOME Keyring), Keychain, Credential Manager.
pub struct KeyringSecrets;

fn keyring_err(e: &keyring::Error) -> EngineError {
    EngineError::Secrets(leemusync_store::redact(&e.to_string()))
}

impl SecretStore for KeyringSecrets {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<String>>, EngineError> {
        let entry = keyring::Entry::new(SERVICE, name).map_err(|e| keyring_err(&e))?;
        match entry.get_password() {
            Ok(v) => Ok(Some(Zeroizing::new(v))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keyring_err(&e)),
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<(), EngineError> {
        keyring::Entry::new(SERVICE, name)
            .and_then(|e| e.set_password(value))
            .map_err(|e| keyring_err(&e))
    }

    fn delete(&self, name: &str) -> Result<(), EngineError> {
        let entry = keyring::Entry::new(SERVICE, name).map_err(|e| keyring_err(&e))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keyring_err(&e)),
        }
    }
}

/// In-memory keystore for tests.
#[derive(Default)]
pub struct MemorySecrets(Mutex<BTreeMap<String, String>>);

impl SecretStore for MemorySecrets {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<String>>, EngineError> {
        let map = self
            .0
            .lock()
            .map_err(|_| EngineError::Secrets("poisoned".into()))?;
        Ok(map.get(name).map(|v| Zeroizing::new(v.clone())))
    }

    fn set(&self, name: &str, value: &str) -> Result<(), EngineError> {
        let mut map = self
            .0
            .lock()
            .map_err(|_| EngineError::Secrets("poisoned".into()))?;
        map.insert(name.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), EngineError> {
        let mut map = self
            .0
            .lock()
            .map_err(|_| EngineError::Secrets("poisoned".into()))?;
        map.remove(name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_secrets_round_trip() {
        let s = MemorySecrets::default();
        assert!(s.get("a").unwrap().is_none());
        s.set("a", "b").unwrap();
        assert_eq!(
            s.get("a").unwrap().as_deref().map(String::as_str),
            Some("b")
        );
        s.delete("a").unwrap();
        s.delete("a").unwrap();
        assert!(s.get("a").unwrap().is_none());
    }

    #[test]
    #[ignore = "uses the real OS keystore"]
    fn keyring_round_trip() {
        let s = KeyringSecrets;
        s.set("leemusync-selftest", "x").unwrap();
        assert_eq!(
            s.get("leemusync-selftest")
                .unwrap()
                .as_deref()
                .map(String::as_str),
            Some("x")
        );
        s.delete("leemusync-selftest").unwrap();
    }
}
