//! Line-based `key=value` account files and Cemu's `MiiName` encoding.
//! Spec: docs/specs/players-and-accounts.md (Cemu accounts).

/// Value of the first `key=value` line whose key is exactly `key`.
#[must_use]
pub fn key_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k == key).then(|| v.trim_end_matches('\r'))
    })
}

/// Cemu `MiiName`: exactly 44 hex digits = 11 UTF-16 code units (big-endian), cut at the first NUL.
#[must_use]
pub fn decode_mii_name(hex: &str) -> Option<String> {
    if hex.len() != 44 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let units: Vec<u16> = (0..11)
        .map(|i| u16::from_str_radix(&hex[i * 4..i * 4 + 4], 16))
        .collect::<Result<_, _>>()
        .ok()?;
    let end = units.iter().position(|u| *u == 0).unwrap_or(units.len());
    String::from_utf16(&units[..end]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!(
        "../../../profiles/fixtures/cemu/flatpak-botw-two-accounts/data/Cemu/mlc01/usr/save/system/act/80000001/account.dat"
    );

    #[test]
    fn reads_the_fixture_name() {
        assert!(FIXTURE.starts_with("AccountInstance_20120705"));
        let hex = key_value(FIXTURE, "MiiName").unwrap();
        assert_eq!(decode_mii_name(hex).as_deref(), Some("PlayerA"));
    }

    #[test]
    fn key_value_matches_whole_keys_only() {
        let text = "AccountInstance_20120705\nMiiNameX=1\nMiiName=2\r\n";
        assert_eq!(key_value(text, "MiiName"), Some("2"));
        assert_eq!(key_value(text, "Missing"), None);
    }

    #[test]
    fn rejects_bad_mii_names() {
        assert_eq!(decode_mii_name("0041"), None, "wrong length");
        assert_eq!(decode_mii_name(&"zz".repeat(22)), None, "not hex");
        assert_eq!(decode_mii_name(&"0".repeat(44)), Some(String::new()));
        assert_eq!(
            decode_mii_name(&format!("D800{}", "0".repeat(40))),
            None,
            "lone surrogate"
        );
    }
}
