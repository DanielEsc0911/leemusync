//! `accounts.create`: a new emulator account described as data (P3, D32). Pure: the caller passes
//! the random bytes, the id counter and the existing ids.
//! Spec: docs/specs/players-and-accounts.md (Creating a Cemu account), emulator-profiles.md (Schema).
//!
//! Value templates are text with `{…}` calls from a closed set (B2: data, never code):
//! `{id}`, `{name}`, `{<var>}`, `{random:N}`, `{slice:<var>:A:B}`, `{utf16hex:<var>:UNITS}`.
//! Anything else fails profile validation.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Deserialize;

/// Largest byte block or random request a profile may ask for.
const MAX_BYTES: usize = 4096;
/// Counter values tried by a CRC fix-up before giving up.
const MAX_CRC_TRIES: u64 = 1 << 24;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TemplateError {
    #[error("unknown template function or variable `{0}`")]
    Unknown(String),
    #[error("malformed account template: {0}")]
    Malformed(&'static str),
    #[error("account name is empty or contains NUL")]
    BadName,
    #[error("random input has the wrong length")]
    RandomLength,
    #[error("no free account id")]
    NoFreeId,
    #[error("the emulator allows at most {0} accounts")]
    TooManyAccounts(u32),
    #[error("CRC fix-up found no counter value")]
    CrcUnreachable,
}

/// `[accounts.create]`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateTemplate {
    pub id: IdRule,
    pub max_accounts: u32,
    pub name_max_utf16: usize,
    /// First line of the file.
    pub header: String,
    /// File name inside the new `<root>/<id>/` folder.
    pub file: String,
    /// Computed before any key is written, so a key may use one that a later key also uses.
    /// Their templates may only use `{id}`, `{name}` and `{random:N}`.
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
    /// In file order: one `key=value` line each.
    pub keys: Vec<KeyTemplate>,
}

/// `id = { format, min, counter_file, counter_key }`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdRule {
    /// Only `hex8`: 8 lowercase hex digits.
    pub format: String,
    pub min: String,
    /// File next to the account folders holding the emulator's counter. Read only, never written.
    pub counter_file: String,
    pub counter_key: String,
}

/// One `key=value` line: a text template (`value`) or a byte block written as hex (`bytes`).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyTemplate {
    pub key: String,
    pub value: Option<String>,
    /// Fixed block, hex.
    pub bytes: Option<String>,
    pub put_utf16le: Option<PutUtf16>,
    pub crc16: Option<Crc16Fix>,
}

/// Writes a variable as UTF-16LE units at byte `at`, zero-filled to `units`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PutUtf16 {
    pub at: usize,
    pub units: usize,
    pub from: String,
}

/// Raises a big-endian counter in bytes `vary[0]..vary[1]` from 0 until the CRC-16 (MSB first,
/// initial 0) of the block equals `target`. The block as given is tried first.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Crc16Fix {
    pub poly: u16,
    pub vary: [usize; 2],
    pub target: u16,
}

#[derive(Debug, PartialEq, Eq)]
enum Part {
    Lit(String),
    Var(String),
    Random(usize),
    Slice(String, usize, usize),
    Utf16Hex(String, usize),
}

fn parse(t: &str) -> Result<Vec<Part>, TemplateError> {
    let mut out = Vec::new();
    let mut rest = t;
    while let Some(start) = rest.find('{') {
        if start > 0 {
            out.push(Part::Lit(rest[..start].to_owned()));
        }
        let after = &rest[start + 1..];
        let end = after.find('}').ok_or(TemplateError::Malformed("brace"))?;
        let call = &after[..end];
        if call.contains('{') {
            return Err(TemplateError::Malformed("brace"));
        }
        out.push(parse_call(call)?);
        rest = &after[end + 1..];
    }
    if rest.contains('}') {
        return Err(TemplateError::Malformed("brace"));
    }
    if !rest.is_empty() {
        out.push(Part::Lit(rest.to_owned()));
    }
    Ok(out)
}

fn num(s: &str) -> Result<usize, TemplateError> {
    s.parse::<usize>()
        .ok()
        .filter(|n| *n <= MAX_BYTES * 2)
        .ok_or(TemplateError::Malformed("number"))
}

fn is_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn parse_call(call: &str) -> Result<Part, TemplateError> {
    let args: Vec<&str> = call.split(':').collect();
    let unknown = || TemplateError::Unknown(call.to_owned());
    match args.as_slice() {
        ["random", n] => Ok(Part::Random(num(n)?)),
        ["slice", v, a, b] if is_name(v) => {
            let (a, b) = (num(a)?, num(b)?);
            if a >= b {
                return Err(TemplateError::Malformed("slice"));
            }
            Ok(Part::Slice((*v).to_owned(), a, b))
        }
        ["utf16hex", v, n] if is_name(v) => Ok(Part::Utf16Hex((*v).to_owned(), num(n)?)),
        [v] if is_name(v) && !matches!(*v, "random" | "slice" | "utf16hex") => {
            Ok(Part::Var((*v).to_owned()))
        }
        _ => Err(unknown()),
    }
}

impl CreateTemplate {
    /// Profile-load checks: every template parses, uses only known names and fits its block.
    ///
    /// # Errors
    /// `Unknown` for a function or variable outside the closed set, else `Malformed`.
    pub fn validate(&self) -> Result<(), TemplateError> {
        if self.id.format != "hex8" || parse_hex8(&self.id.min).is_none() {
            return Err(TemplateError::Malformed("id"));
        }
        for f in [&self.file, &self.id.counter_file] {
            if f.is_empty() || f.contains(['/', '\\', '\0']) || f.starts_with('.') {
                return Err(TemplateError::Malformed("file"));
            }
        }
        if self.max_accounts == 0
            || self.name_max_utf16 == 0
            || self.header.contains('\n')
            || self.keys.is_empty()
        {
            return Err(TemplateError::Malformed("limits"));
        }
        for (name, t) in &self.vars {
            if !is_name(name) || matches!(name.as_str(), "id" | "name") {
                return Err(TemplateError::Malformed("vars"));
            }
            for part in parse(t)? {
                match part {
                    Part::Var(v) if v == "id" || v == "name" => {}
                    Part::Lit(_) | Part::Random(_) => {}
                    Part::Var(v) | Part::Slice(v, ..) | Part::Utf16Hex(v, _) => {
                        return Err(TemplateError::Unknown(v));
                    }
                }
            }
        }
        let known = |v: &str| v == "id" || v == "name" || self.vars.contains_key(v);
        for k in &self.keys {
            if !is_key(&k.key) {
                return Err(TemplateError::Malformed("key"));
            }
            match (&k.value, &k.bytes) {
                (Some(t), None) if k.put_utf16le.is_none() && k.crc16.is_none() => {
                    for part in parse(t)? {
                        match part {
                            Part::Lit(l) if l.contains(['\n', '\r']) => {
                                return Err(TemplateError::Malformed("value"));
                            }
                            Part::Random(_) => return Err(TemplateError::Unknown("random".into())),
                            Part::Var(v) | Part::Slice(v, ..) | Part::Utf16Hex(v, _)
                                if !known(&v) =>
                            {
                                return Err(TemplateError::Unknown(v));
                            }
                            _ => {}
                        }
                    }
                }
                (None, Some(hex)) => {
                    let len = from_hex(hex)
                        .filter(|b| b.len() <= MAX_BYTES)
                        .ok_or(TemplateError::Malformed("bytes"))?
                        .len();
                    if let Some(p) = &k.put_utf16le {
                        if !known(&p.from) {
                            return Err(TemplateError::Unknown(p.from.clone()));
                        }
                        if p.at.checked_add(p.units * 2).is_none_or(|end| end > len) {
                            return Err(TemplateError::Malformed("put_utf16le"));
                        }
                    }
                    if let Some(c) = &k.crc16 {
                        let [a, b] = c.vary;
                        if a >= b || b > len || b - a > 8 {
                            return Err(TemplateError::Malformed("crc16"));
                        }
                    }
                }
                _ => return Err(TemplateError::Malformed("key value")),
            }
        }
        Ok(())
    }

    /// How many random bytes `render` needs.
    #[must_use]
    pub fn random_len(&self) -> usize {
        self.vars
            .values()
            .flat_map(|t| parse(t).unwrap_or_default())
            .map(|p| if let Part::Random(n) = p { n } else { 0 })
            .sum()
    }

    /// The smallest id the rule allows.
    #[must_use]
    pub fn min_id(&self) -> u32 {
        parse_hex8(&self.id.min).unwrap_or(1)
    }
}

fn is_key(k: &str) -> bool {
    !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// 8 hex digits.
#[must_use]
pub fn parse_hex8(s: &str) -> Option<u32> {
    (s.len() == 8 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| u32::from_str_radix(s, 16).ok())
        .flatten()
}

/// Next free id: max(counter + 1, highest existing + 1, `min`).
///
/// # Errors
/// `TooManyAccounts` when `existing` already has `max_accounts`; `NoFreeId` on overflow.
pub fn next_id(
    t: &CreateTemplate,
    counter: Option<u32>,
    existing: &[u32],
) -> Result<u32, TemplateError> {
    let min = t.min_id();
    let existing: Vec<u32> = existing.iter().copied().filter(|i| *i >= min).collect();
    if existing.len() >= t.max_accounts as usize {
        return Err(TemplateError::TooManyAccounts(t.max_accounts));
    }
    let after = |v: Option<u32>| match v {
        Some(v) => v.checked_add(1).ok_or(TemplateError::NoFreeId),
        None => Ok(min),
    };
    let id = after(counter)?
        .max(after(existing.iter().max().copied())?)
        .max(min);
    Ok(id)
}

/// The name cut to at most `max` UTF-16 units, never splitting a surrogate pair.
///
/// # Errors
/// `BadName` when the result is empty or the name contains NUL.
pub fn fit_name(name: &str, max: usize) -> Result<String, TemplateError> {
    if name.contains('\0') {
        return Err(TemplateError::BadName);
    }
    let mut units = 0;
    let out: String = name
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= max
        })
        .collect();
    if out.is_empty() {
        return Err(TemplateError::BadName);
    }
    Ok(out)
}

/// The whole file: header, then one `key=value` line per key, each ending in `\n`.
///
/// # Errors
/// `RandomLength` when `random` isn't exactly `random_len()` bytes, `BadName`, `CrcUnreachable`,
/// or a template error (only for a template that skipped `validate`).
pub fn render(
    t: &CreateTemplate,
    id: u32,
    name: &str,
    random: &[u8],
) -> Result<String, TemplateError> {
    if random.len() != t.random_len() {
        return Err(TemplateError::RandomLength);
    }
    let name = fit_name(name, t.name_max_utf16)?;
    let mut vars = BTreeMap::from([
        ("id".to_owned(), format!("{id:08x}")),
        ("name".to_owned(), name),
    ]);
    let mut cursor = 0;
    let mut computed = Vec::new();
    for (k, tmpl) in &t.vars {
        computed.push((k.clone(), expand(tmpl, &vars, random, &mut cursor)?));
    }
    vars.extend(computed);
    let mut out = format!("{}\n", t.header);
    for k in &t.keys {
        let value = match (&k.value, &k.bytes) {
            (Some(tmpl), _) => expand(tmpl, &vars, &[], &mut 0)?,
            (None, Some(hex)) => to_hex(&block(k, hex, &vars)?),
            (None, None) => return Err(TemplateError::Malformed("key value")),
        };
        let _ = writeln!(out, "{}={value}", k.key);
    }
    Ok(out)
}

fn block(
    k: &KeyTemplate,
    hex: &str,
    vars: &BTreeMap<String, String>,
) -> Result<Vec<u8>, TemplateError> {
    let mut data = from_hex(hex).ok_or(TemplateError::Malformed("bytes"))?;
    if let Some(p) = &k.put_utf16le {
        let v = vars
            .get(&p.from)
            .ok_or_else(|| TemplateError::Unknown(p.from.clone()))?;
        let end = p.at + p.units * 2;
        let region = data
            .get_mut(p.at..end)
            .ok_or(TemplateError::Malformed("put_utf16le"))?;
        region.fill(0);
        for (i, u) in v.encode_utf16().take(p.units).enumerate() {
            region[i * 2..i * 2 + 2].copy_from_slice(&u.to_le_bytes());
        }
    }
    if let Some(c) = &k.crc16 {
        let [a, b] = c.vary;
        if a >= b || b > data.len() || b - a > 8 {
            return Err(TemplateError::Malformed("crc16"));
        }
        let mut counter: u64 = 0;
        while crc16(c.poly, &data) != c.target {
            if counter >= MAX_CRC_TRIES {
                return Err(TemplateError::CrcUnreachable);
            }
            data[a..b].copy_from_slice(&counter.to_be_bytes()[8 - (b - a)..]);
            counter += 1;
        }
    }
    Ok(data)
}

fn expand(
    t: &str,
    vars: &BTreeMap<String, String>,
    random: &[u8],
    cursor: &mut usize,
) -> Result<String, TemplateError> {
    let get = |v: &str| {
        vars.get(v)
            .ok_or_else(|| TemplateError::Unknown(v.to_owned()))
    };
    let mut out = String::new();
    for part in parse(t)? {
        match part {
            Part::Lit(l) => out.push_str(&l),
            Part::Var(v) => out.push_str(get(&v)?),
            Part::Random(n) => {
                let bytes = random
                    .get(*cursor..*cursor + n)
                    .ok_or(TemplateError::RandomLength)?;
                *cursor += n;
                out.push_str(&to_hex(bytes));
            }
            Part::Slice(v, a, b) => out.push_str(
                get(&v)?
                    .get(a..b)
                    .ok_or(TemplateError::Malformed("slice"))?,
            ),
            Part::Utf16Hex(v, n) => {
                let units: Vec<u16> = get(&v)?.encode_utf16().collect();
                if units.len() > n {
                    return Err(TemplateError::Malformed("utf16hex"));
                }
                for u in units.iter().copied().chain(std::iter::repeat(0)).take(n) {
                    let _ = write!(out, "{u:04x}");
                }
            }
        }
    }
    Ok(out)
}

/// Cemu's `FFLCalculateCRC16`: for each byte, shift the high byte out through `poly` eight
/// times, then xor the byte into the low bits. Initial value 0, no final xor.
#[must_use]
pub fn crc16(poly: u16, data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        let mut high = crc & 0xFF00;
        for _ in 0..8 {
            high = if high & 0x8000 == 0 {
                high << 1
            } else {
                (high << 1) ^ poly
            };
        }
        crc = ((crc << 8) ^ high) ^ u16::from(byte);
    }
    crc
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Profile;

    const CEMU: &str = include_str!("../../../profiles/cemu.toml");
    const SP8_TEST: &str =
        include_str!("../../../profiles/fixtures/cemu/account-create/80000002-sp8-test.dat");
    const SP8CEMU: &str =
        include_str!("../../../profiles/fixtures/cemu/account-create/80000003-sp8cemu.dat");
    /// `MiiData` of the account Cemu 2.6 itself created as `sp8cemu` (SP8, 2026-10-20, scratch
    /// tree, since deleted): it matched the throwaway script's output byte for byte. The fixture
    /// files are hand-built in `Account::Save()` order around it; V-ACC-4 replaces them with a
    /// Cemu-written file.
    const SP8CEMU_MII: &str = "01000110000045bd030034330100010001000100010001000100730070003800630065006d0075000000000000000100010001000100010001000106010001000100010001000100010001000100010001000100010001000100010001000100";

    fn template() -> CreateTemplate {
        Profile::from_toml(CEMU)
            .unwrap()
            .accounts
            .unwrap()
            .create
            .unwrap()
    }

    fn value<'a>(file: &'a str, key: &str) -> &'a str {
        crate::account::key_value(file, key).unwrap()
    }

    #[test]
    fn reproduces_the_golden_files_byte_for_byte() {
        let t = template();
        for (file, id, name) in [
            (SP8_TEST, 0x8000_0002, "SP8 Test"),
            (SP8CEMU, 0x8000_0003, "sp8cemu"),
        ] {
            let uuid = from_hex(value(file, "Uuid")).unwrap();
            assert_eq!(render(&t, id, name, &uuid).unwrap(), file, "{name}");
            let mii = from_hex(value(file, "MiiData")).unwrap();
            assert_eq!(mii.len(), 96);
            assert_eq!(crc16(0x1021, &mii), 0, "{name}");
        }
        assert_eq!(value(SP8CEMU, "MiiData"), SP8CEMU_MII);
    }

    #[test]
    fn same_name_gives_the_same_mii_data() {
        let t = template();
        let a = render(&t, 0x8000_0005, "sp8cemu", &[1; 16]).unwrap();
        let b = render(&t, 0x8000_0009, "sp8cemu", &[7; 16]).unwrap();
        assert_eq!(value(&a, "MiiData"), SP8CEMU_MII);
        assert_eq!(value(&a, "MiiData"), value(&b, "MiiData"));
        assert_ne!(value(&a, "Uuid"), value(&b, "Uuid"));
        assert!(value(&b, "TransferableIdBase").ends_with("07070707"));
    }

    #[test]
    fn next_id_is_after_the_counter_and_the_highest_account() {
        let t = template();
        assert_eq!(next_id(&t, None, &[]), Ok(0x8000_0001));
        assert_eq!(
            next_id(&t, Some(0x8000_0003), &[0x8000_0001]),
            Ok(0x8000_0004)
        );
        assert_eq!(
            next_id(&t, Some(0x8000_0001), &[0x8000_0001, 0x8000_0007]),
            Ok(0x8000_0008)
        );
        assert_eq!(next_id(&t, Some(5), &[]), Ok(0x8000_0001), "at least min");
        let twelve: Vec<u32> = (1..=12).map(|i| 0x8000_0000 + i).collect();
        assert_eq!(
            next_id(&t, None, &twelve),
            Err(TemplateError::TooManyAccounts(12))
        );
        assert_eq!(
            next_id(&t, Some(u32::MAX), &[]),
            Err(TemplateError::NoFreeId)
        );
    }

    #[test]
    fn long_names_are_cut_not_rejected() {
        let t = template();
        let out = render(&t, 0x8000_0002, "Abcdefghijklmnop", &[0; 16]).unwrap();
        let name = crate::account::decode_mii_name(value(&out, "MiiName")).unwrap();
        assert_eq!(name, "Abcdefghij");
        // A surrogate pair is never split: 9 units + a 2-unit emoji is cut before the emoji.
        assert_eq!(fit_name("abcdefghi\u{1F600}", 10).unwrap(), "abcdefghi");
        assert_eq!(
            fit_name("abcdefgh\u{1F600}x", 10).unwrap(),
            "abcdefgh\u{1F600}"
        );
        assert_eq!(fit_name("", 10), Err(TemplateError::BadName));
        assert_eq!(fit_name("a\0b", 10), Err(TemplateError::BadName));
        assert_eq!(
            render(&t, 1, "x", &[0; 15]),
            Err(TemplateError::RandomLength)
        );
    }

    #[test]
    fn unknown_template_functions_fail_profile_validation() {
        for (from, to) in [
            ("2000004{slice:uuid:24:32}", "{exec:sh}"),
            ("value = \"{uuid}\"", "value = \"{home}\""),
            ("uuid = \"{random:16}\"", "uuid = \"{slice:name:0:2}\""),
            ("value = \"{id}\"", "value = \"{random:4}\""),
        ] {
            let text = CEMU.replace(from, to);
            assert_ne!(text, CEMU, "{from}");
            assert!(
                matches!(
                    Profile::from_toml(&text),
                    Err(crate::profile::ProfileError::UnknownVar(_))
                ),
                "{to}"
            );
        }
        let extra = CEMU.replace("max_accounts = 12", "max_accounts = 12\nscript = \"x\"");
        assert!(matches!(
            Profile::from_toml(&extra),
            Err(crate::profile::ProfileError::Toml(_))
        ));
    }
}
