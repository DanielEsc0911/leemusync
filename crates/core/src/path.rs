//! Validated relative paths. Spec: docs/specs/security.md (File-system confinement).

use serde::{Deserialize, Deserializer, Serialize, Serializer};

const MAX_PATH: usize = 1024;
const MAX_COMPONENT: usize = 255;
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PathError {
    #[error("path is empty")]
    Empty,
    #[error("path is longer than 1024 bytes")]
    TooLong,
    #[error("path component is empty, `.` or `..`")]
    BadComponent,
    #[error("path component is longer than 255 bytes")]
    ComponentTooLong,
    #[error("path contains a forbidden character")]
    BadChar,
    #[error("path uses a name Windows reserves")]
    Reserved,
}

/// A relative, `/`-separated path that can't escape its root on any OS.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelPath(String);

impl RelPath {
    /// # Errors
    /// A `PathError` naming the first rule `s` breaks.
    pub fn parse(s: &str) -> Result<Self, PathError> {
        if s.is_empty() {
            return Err(PathError::Empty);
        }
        if s.len() > MAX_PATH {
            return Err(PathError::TooLong);
        }
        for component in s.split('/') {
            check_component(component)?;
        }
        Ok(Self(s.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

fn check_component(c: &str) -> Result<(), PathError> {
    if c.is_empty() || c == "." || c == ".." {
        return Err(PathError::BadComponent);
    }
    if c.len() > MAX_COMPONENT {
        return Err(PathError::ComponentTooLong);
    }
    if c.chars()
        .any(|ch| ch.is_control() || matches!(ch, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Err(PathError::BadChar);
    }
    if c.ends_with('.') || c.ends_with(' ') {
        return Err(PathError::BadChar);
    }
    let stem = c.split('.').next().unwrap_or(c);
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return Err(PathError::Reserved);
    }
    Ok(())
}

impl Serialize for RelPath {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RelPath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_save_paths() {
        for ok in [
            "option.sav",
            "0/game_data.sav",
            "album/a b.jpg",
            "pict_book/x.sav",
        ] {
            assert_eq!(RelPath::parse(ok).unwrap().as_str(), ok);
        }
    }

    #[test]
    fn rejects_escapes_and_absolute_paths() {
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "a/../b",
            "a/./b",
            "./a",
            "a//b",
            "a/",
            "C:/x",
            "a\\b",
            "a:b",
        ] {
            assert!(RelPath::parse(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn rejects_control_characters_and_nul() {
        assert!(RelPath::parse("a\0b").is_err());
        assert!(RelPath::parse("a\nb").is_err());
    }

    #[test]
    fn rejects_windows_reserved_names_and_trailing_dots() {
        for bad in [
            "CON", "con.txt", "a/NUL", "Com1.sav", "lpt9", "a/b.", "a/b ",
        ] {
            assert!(RelPath::parse(bad).is_err(), "{bad:?} must be rejected");
        }
        assert!(
            RelPath::parse("console.sav").is_ok(),
            "only exact reserved stems"
        );
    }

    #[test]
    fn enforces_length_limits() {
        assert!(RelPath::parse(&"a".repeat(256)).is_err());
        let deep = vec!["d"; 600].join("/");
        assert!(RelPath::parse(&deep).is_err(), "over 1024 bytes");
    }

    #[test]
    fn splits_components() {
        let p = RelPath::parse("0/game_data.sav").unwrap();
        assert_eq!(p.components().collect::<Vec<_>>(), ["0", "game_data.sav"]);
    }
}
