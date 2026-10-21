//! Identifiers. Spec: docs/specs/sync-model.md (Versions, Streams).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdError {
    #[error("an id must be 26 Crockford base32 characters")]
    BadVersionId,
    #[error("a hash must be 64 lowercase hex characters")]
    BadHash,
}

/// Time-ordered unique id: 48-bit Unix milliseconds + 80 random bits (ULID layout).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VersionId(u128);

impl VersionId {
    /// Builds an id from a clock reading and 10 random bytes (callers supply both: `core` has no I/O).
    #[must_use]
    pub fn from_parts(unix_ms: u64, random: [u8; 10]) -> Self {
        let time = u128::from(unix_ms & 0xFFFF_FFFF_FFFF) << 80;
        let rand = random
            .iter()
            .fold(0u128, |acc, b| (acc << 8) | u128::from(*b));
        Self(time | rand)
    }

    #[must_use]
    pub fn unix_ms(self) -> u64 {
        u64::try_from(self.0 >> 80).unwrap_or(u64::MAX)
    }
}

impl fmt::Display for VersionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = [0u8; 26];
        for (i, slot) in out.iter_mut().enumerate() {
            let shift = 125 - 5 * i;
            let index = usize::try_from((self.0 >> shift) & 31).unwrap_or(0);
            *slot = CROCKFORD[index];
        }
        f.write_str(std::str::from_utf8(&out).map_err(|_| fmt::Error)?)
    }
}

impl FromStr for VersionId {
    type Err = IdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 26 {
            return Err(IdError::BadVersionId);
        }
        let mut value: u128 = 0;
        for (i, c) in s.bytes().enumerate() {
            let upper = c.to_ascii_uppercase();
            let digit = CROCKFORD
                .iter()
                .position(|d| *d == upper)
                .ok_or(IdError::BadVersionId)?;
            if i == 0 && digit > 7 {
                return Err(IdError::BadVersionId);
            }
            value =
                (value << 5) | u128::from(u8::try_from(digit).map_err(|_| IdError::BadVersionId)?);
        }
        Ok(Self(value))
    }
}

/// A 32-byte keyed hash, written as 64 lowercase hex characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hash32(pub [u8; 32]);

impl Hash32 {
    #[must_use]
    pub fn to_hex(&self) -> String {
        use fmt::Write as _;
        self.0.iter().fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
    }

    /// # Errors
    /// `IdError::BadHash` unless `s` is exactly 64 lowercase hex characters.
    pub fn from_hex(s: &str) -> Result<Self, IdError> {
        let bytes = s.as_bytes();
        if bytes.len() != 64 {
            return Err(IdError::BadHash);
        }
        let mut out = [0u8; 32];
        for (i, pair) in bytes.chunks(2).enumerate() {
            let hi = hex_digit(pair[0])?;
            let lo = hex_digit(pair[1])?;
            out[i] = (hi << 4) | lo;
        }
        Ok(Self(out))
    }
}

fn hex_digit(c: u8) -> Result<u8, IdError> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        _ => Err(IdError::BadHash),
    }
}

macro_rules! string_serde {
    ($ty:ty, $to:expr, $from:expr) => {
        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&$to(self))
            }
        }
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let text = String::deserialize(d)?;
                $from(&text).map_err(serde::de::Error::custom)
            }
        }
    };
}

/// Content id of a blob: keyed hash of its plaintext.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlobId(pub Hash32);
/// Opaque stream id: keyed hash of the canonical stream tuple.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamId(pub Hash32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlayerId(pub VersionId);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(pub VersionId);
/// Random id of one play session (one lease acquisition).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(pub VersionId);

string_serde!(VersionId, |v: &VersionId| v.to_string(), |t: &str| {
    t.parse::<VersionId>()
});
string_serde!(Hash32, Hash32::to_hex, Hash32::from_hex);
string_serde!(BlobId, |v: &BlobId| v.0.to_hex(), |t: &str| {
    Hash32::from_hex(t).map(BlobId)
});
string_serde!(StreamId, |v: &StreamId| v.0.to_hex(), |t: &str| {
    Hash32::from_hex(t).map(StreamId)
});
string_serde!(PlayerId, |v: &PlayerId| v.0.to_string(), |t: &str| t
    .parse()
    .map(PlayerId));
string_serde!(DeviceId, |v: &DeviceId| v.0.to_string(), |t: &str| t
    .parse()
    .map(DeviceId));
string_serde!(SessionId, |v: &SessionId| v.0.to_string(), |t: &str| t
    .parse()
    .map(SessionId));

impl fmt::Display for BlobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.to_hex())
    }
}
impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.to_hex())
    }
}
impl fmt::Display for PlayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_id_round_trips_and_sorts_by_time() {
        let a = VersionId::from_parts(1_000, [0xff; 10]);
        let b = VersionId::from_parts(1_001, [0x00; 10]);
        assert!(a < b);
        assert_eq!(a.unix_ms(), 1_000);
        let text = a.to_string();
        assert_eq!(text.len(), 26);
        assert_eq!(text.parse::<VersionId>().unwrap(), a);
        assert!(
            b.to_string() > a.to_string(),
            "text order matches time order"
        );
    }

    #[test]
    fn version_id_keeps_only_48_bits_of_time() {
        let id = VersionId::from_parts(u64::MAX, [0; 10]);
        assert_eq!(id.unix_ms(), 0xFFFF_FFFF_FFFF);
    }

    #[test]
    fn version_id_rejects_bad_text() {
        assert!("".parse::<VersionId>().is_err());
        assert!(
            "0123456789ABCDEFGHJKMNPQRSTV".parse::<VersionId>().is_err(),
            "too long"
        );
        assert!(
            "8ZZZZZZZZZZZZZZZZZZZZZZZZZ".parse::<VersionId>().is_err(),
            "overflows 128 bits"
        );
        assert!(
            "0000000000000000000000000U".parse::<VersionId>().is_err(),
            "U is not Crockford"
        );
    }

    #[test]
    fn version_id_parses_lowercase() {
        let id = VersionId::from_parts(42, [7; 10]);
        assert_eq!(
            id.to_string().to_lowercase().parse::<VersionId>().unwrap(),
            id
        );
    }

    #[test]
    fn hash_hex_round_trips() {
        let h = Hash32([0xab; 32]);
        assert_eq!(h.to_hex(), "ab".repeat(32));
        assert_eq!(Hash32::from_hex(&h.to_hex()).unwrap(), h);
        assert!(Hash32::from_hex("AB").is_err());
        assert!(
            Hash32::from_hex(&"AB".repeat(32)).is_err(),
            "uppercase is not canonical"
        );
    }

    #[test]
    fn ids_serialise_as_strings() {
        let blob = BlobId(Hash32([1; 32]));
        let json = serde_json::to_string(&blob).unwrap();
        assert_eq!(json, format!("\"{}\"", "01".repeat(32)));
        assert_eq!(serde_json::from_str::<BlobId>(&json).unwrap(), blob);
        let player = PlayerId(VersionId::from_parts(5, [9; 10]));
        let json = serde_json::to_string(&player).unwrap();
        assert_eq!(serde_json::from_str::<PlayerId>(&json).unwrap(), player);
    }
}
