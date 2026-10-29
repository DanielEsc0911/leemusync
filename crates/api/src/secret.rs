//! A secret that crosses the local socket: passphrase, recovery key, S3 secret key
//! (security §Local IPC, §Credentials). Wiped on drop; `Debug` never shows it.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroizing;

/// See the module docs. Serialized as a plain JSON string.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    /// Wraps `s`.
    #[must_use]
    pub fn new(s: Zeroizing<String>) -> Self {
        Self(s)
    }

    /// The secret text.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The wiped-on-drop text.
    #[must_use]
    pub fn into_inner(self) -> Zeroizing<String> {
        self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

impl Serialize for Secret {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(|s| Self(Zeroizing::new(s)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_the_secret_and_json_is_a_string() {
        let s = Secret::new(Zeroizing::new("hunter2".into()));
        assert_eq!(format!("{s:?}"), "Secret(..)");
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#""hunter2""#);
        assert_eq!(serde_json::from_str::<Secret>(&json).unwrap(), s);
    }
}
