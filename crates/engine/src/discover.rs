//! Accounts, save folders and orphaned saves of one install (players-and-accounts §Discovery).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use leemusync_api::{Warning, WarningCode};
use leemusync_core::account::{decode_mii_name, key_value};
use leemusync_core::profile::{PatternPart, Profile, parse_pattern};

use crate::profiles::Install;
use crate::{EngineError, fsx};

const MAX_ACCOUNT_FILE: u64 = 64 * 1024;
const TITLE_ID: &str = "[0-9a-f]{8}";
/// Used when a profile has no `[accounts]`: any single path component.
const ANY_COMPONENT: &str = r"[^/\\]+";

pub struct Account {
    pub id: String,
    pub name: Option<String>,
}

pub struct SaveFolder {
    pub game: String,
    pub account: String,
    pub path: PathBuf,
    pub orphaned: bool,
}

pub struct Discovery {
    pub accounts: Vec<Account>,
    pub saves: Vec<SaveFolder>,
    pub shared_not_empty: Vec<PathBuf>,
    /// Folders skipped because they are reached through a symlink (`UnreadableFolder`).
    pub warnings: Vec<Warning>,
}

/// Compiles `pattern` anchored, so it must match a whole folder name.
fn regex(pattern: &str) -> Result<regex::Regex, EngineError> {
    regex::Regex::new(&format!("^(?:{pattern})$"))
        .map_err(|_| EngineError::Config("invalid_regex".into()))
}

fn account_regex(profile: &Profile) -> Result<regex::Regex, EngineError> {
    regex(
        profile
            .accounts
            .as_ref()
            .map_or(ANY_COMPONENT, |a| a.id_regex.as_str()),
    )
}

fn safe_component(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\'])
}

fn dirs(path: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| safe_component(n))
        .collect();
    out.sort();
    out
}

/// Reads `name_file` if it is a small regular file (never through a symlink).
fn read_account_file(file: &Path) -> Option<String> {
    let meta = std::fs::symlink_metadata(file).ok()?;
    if !meta.is_file() || meta.len() > MAX_ACCOUNT_FILE {
        return None;
    }
    std::fs::read_to_string(file).ok()
}

fn unsupported_capture() -> EngineError {
    EngineError::Config("unsupported_capture".into())
}

/// # Errors
/// `Config` for an invalid `id_regex`, a bad pattern, or a capture the engine doesn't support.
pub fn discover(profile: &Profile, install: &Install) -> Result<Discovery, EngineError> {
    let account_re = account_regex(profile)?;
    let title_re = regex(TITLE_ID)?;
    let parts =
        parse_pattern(&profile.saves.pattern).map_err(|e| EngineError::Config(e.to_string()))?;
    let mut accounts = Vec::new();
    if let (Some(root), Some(acc)) = (&install.accounts_root, &profile.accounts) {
        for id in dirs(root).into_iter().filter(|d| account_re.is_match(d)) {
            let name = read_account_file(&root.join(&id).join(&acc.name_file))
                .and_then(|t| key_value(&t, &acc.name_key).and_then(decode_mii_name));
            accounts.push(Account { id, name });
        }
    }
    let mut found: Vec<(PathBuf, BTreeMap<String, String>)> =
        vec![(install.saves_root.clone(), BTreeMap::new())];
    for part in &parts {
        let mut next = Vec::new();
        for (path, bound) in found {
            match part {
                PatternPart::Literal(l) => next.push((path.join(l), bound)),
                PatternPart::Var(v) => {
                    let re = match v.as_str() {
                        "title_id" => &title_re,
                        "account" => &account_re,
                        _ => return Err(unsupported_capture()),
                    };
                    for d in dirs(&path).into_iter().filter(|d| re.is_match(d)) {
                        let mut b = bound.clone();
                        b.insert(v.clone(), d.clone());
                        next.push((path.join(&d), b));
                    }
                }
            }
        }
        found = next;
    }
    let mut warnings = Vec::new();
    // Literal pattern components are followed by the file system; nothing below the saves root
    // may be a symlink, or discovery would report folders outside the grant (B2).
    let mut confined = |path: &Path| {
        let ok = fsx::ensure_no_symlinks(&install.saves_root, path).is_ok();
        if !ok {
            warnings.push(Warning {
                code: WarningCode::UnreadableFolder,
                detail: path.display().to_string(),
            });
        }
        ok
    };
    let found: Vec<_> = found.into_iter().filter(|(p, _)| confined(p)).collect();
    let known: Vec<&str> = accounts.iter().map(|a| a.id.as_str()).collect();
    let mut saves: Vec<SaveFolder> = found
        .into_iter()
        .filter(|(p, _)| p.is_dir())
        .filter_map(|(path, b)| {
            let game = b.get("title_id")?.to_lowercase();
            let account = b.get("account")?.clone();
            let orphaned = install.accounts_root.is_some() && !known.contains(&account.as_str());
            Some(SaveFolder {
                game,
                account,
                path,
                orphaned,
            })
        })
        .collect();
    saves.sort_by(|a, b| (&a.game, &a.account).cmp(&(&b.game, &b.account)));
    let mut shared_not_empty = Vec::new();
    for game in saves
        .iter()
        .map(|s| s.game.clone())
        .collect::<BTreeSet<_>>()
    {
        for shared in &profile.saves.shared {
            let rel = shared.replace("{title_id}", &game);
            let path = install.saves_root.join(rel);
            if confined(&path) && std::fs::read_dir(&path).is_ok_and(|mut d| d.next().is_some()) {
                shared_not_empty.push(path);
            }
        }
    }
    Ok(Discovery {
        accounts,
        saves,
        shared_not_empty,
        warnings,
    })
}

/// Where a (game, account) save folder lives, whether or not it exists yet.
///
/// # Errors
/// `Config` for an invalid pattern or regex, or an unsupported capture; `Usage` for a game or
/// account id the profile rejects.
pub fn save_folder(
    profile: &Profile,
    install: &Install,
    game: &str,
    account: &str,
) -> Result<PathBuf, EngineError> {
    let account_re = account_regex(profile)?;
    if !regex(TITLE_ID)?.is_match(game) || !account_re.is_match(account) || !safe_component(account)
    {
        return Err(EngineError::Usage("invalid_game_or_account".into()));
    }
    let parts =
        parse_pattern(&profile.saves.pattern).map_err(|e| EngineError::Config(e.to_string()))?;
    let mut path = install.saves_root.clone();
    for part in &parts {
        match part {
            PatternPart::Literal(l) => path.push(l),
            PatternPart::Var(v) if v == "title_id" => path.push(game),
            PatternPart::Var(v) if v == "account" => path.push(account),
            PatternPart::Var(_) => return Err(unsupported_capture()),
        }
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::bundled;

    fn cemu() -> Profile {
        bundled().unwrap().remove(0)
    }

    fn install(root: &Path) -> Install {
        Install {
            emulator: "cemu".into(),
            base: root.to_path_buf(),
            config_dir: root.to_path_buf(),
            saves_root: root.join("mlc01/usr/save"),
            accounts_root: Some(root.join("mlc01/usr/save/system/act")),
            warnings: Vec::new(),
            provisional: false,
        }
    }

    fn with_pattern(pattern: &str) -> Profile {
        let text = include_str!("../../../profiles/cemu.toml")
            .replace("00050000/{title_id}/user/{account}", pattern);
        Profile::from_toml(&text).unwrap()
    }

    #[test]
    fn save_folder_builds_the_cemu_path() {
        let i = install(Path::new("/b"));
        let p = save_folder(&cemu(), &i, "101c9400", "80000001").unwrap();
        assert_eq!(
            p,
            Path::new("/b/mlc01/usr/save/00050000/101c9400/user/80000001")
        );
    }

    #[test]
    fn save_folder_rejects_ids_the_profile_does_not_allow() {
        let i = install(Path::new("/b"));
        for (g, a) in [
            ("101c9400", ".."),
            ("101c9400", ""),
            ("101c9400", "80000001/x"),
            ("101c9400", "x80000001"),
            ("101c9400", "800000011"),
            ("../c9400", "80000001"),
        ] {
            assert!(
                matches!(save_folder(&cemu(), &i, g, a), Err(EngineError::Usage(_))),
                "{g}/{a}"
            );
        }
    }

    #[test]
    fn an_unsupported_capture_is_a_config_error_not_a_guess() {
        let dir = tempfile::tempdir().unwrap();
        let p = with_pattern("00050000/{title_id}/{game}/{account}");
        let i = install(dir.path());
        std::fs::create_dir_all(i.saves_root.join("00050000/101c9400/user/80000001")).unwrap();
        let key = |e: Option<EngineError>| match e {
            Some(EngineError::Config(k)) => k,
            _ => String::new(),
        };
        let a = save_folder(&p, &i, "101c9400", "80000001").err();
        assert_eq!(key(a), "unsupported_capture");
        assert_eq!(key(discover(&p, &i).err()), "unsupported_capture");
    }

    #[test]
    fn an_invalid_account_regex_is_a_config_error() {
        let dir = tempfile::tempdir().unwrap();
        let text = include_str!("../../../profiles/cemu.toml")
            .replace(r#"id_regex = "^8[0-9a-f]{7}$""#, r#"id_regex = "8[""#);
        let p = Profile::from_toml(&text).unwrap();
        let i = install(dir.path());
        assert!(matches!(discover(&p, &i), Err(EngineError::Config(k)) if k == "invalid_regex"));
    }

    #[test]
    fn account_ids_must_match_the_whole_folder_name() {
        let dir = tempfile::tempdir().unwrap();
        let text = include_str!("../../../profiles/cemu.toml").replace(
            r#"id_regex = "^8[0-9a-f]{7}$""#,
            r#"id_regex = "8[0-9a-f]{7}""#,
        );
        let p = Profile::from_toml(&text).unwrap();
        let i = install(dir.path());
        let act = i.accounts_root.clone().unwrap();
        std::fs::create_dir_all(act.join("80000001")).unwrap();
        std::fs::create_dir_all(act.join("x80000001")).unwrap();
        std::fs::create_dir_all(act.join("800000011")).unwrap();
        let d = discover(&p, &i).unwrap();
        let ids: Vec<&str> = d.accounts.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["80000001"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_account_file_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let i = install(dir.path());
        let acct = i.accounts_root.clone().unwrap().join("80000001");
        std::fs::create_dir_all(&acct).unwrap();
        let outside = dir.path().join("outside.dat");
        std::fs::write(&outside, "MiiName=00500061\n").unwrap();
        std::os::unix::fs::symlink(&outside, acct.join("account.dat")).unwrap();
        let d = discover(&cemu(), &i).unwrap();
        assert_eq!(d.accounts.len(), 1);
        assert_eq!(d.accounts[0].name, None);
    }

    #[test]
    fn an_account_folder_without_account_dat_is_known_and_not_orphaned() {
        let dir = tempfile::tempdir().unwrap();
        let i = install(dir.path());
        std::fs::create_dir_all(i.accounts_root.clone().unwrap().join("80000001")).unwrap();
        std::fs::create_dir_all(i.saves_root.join("00050000/101c9400/user/80000001")).unwrap();
        let d = discover(&cemu(), &i).unwrap();
        assert_eq!(d.accounts.len(), 1);
        assert_eq!(d.accounts[0].name, None);
        assert_eq!(d.saves.len(), 1);
        assert!(!d.saves[0].orphaned);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_literal_component_hides_the_saves_behind_it() {
        let dir = tempfile::tempdir().unwrap();
        let i = install(dir.path());
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(outside.join("101c9400/user/80000001")).unwrap();
        std::fs::create_dir_all(outside.join("101c9400/user/common")).unwrap();
        std::fs::write(outside.join("101c9400/user/common/x"), "x").unwrap();
        std::fs::create_dir_all(&i.saves_root).unwrap();
        std::os::unix::fs::symlink(&outside, i.saves_root.join("00050000")).unwrap();
        let d = discover(&cemu(), &i).unwrap();
        assert_eq!(d.saves.len(), 0);
        assert_eq!(d.shared_not_empty.len(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_literal_below_the_game_is_dropped_with_a_warning() {
        let dir = tempfile::tempdir().unwrap();
        let i = install(dir.path());
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(outside.join("80000001")).unwrap();
        let game = i.saves_root.join("00050000/101c9400");
        std::fs::create_dir_all(&game).unwrap();
        std::os::unix::fs::symlink(&outside, game.join("user")).unwrap();
        let d = discover(&cemu(), &i).unwrap();
        assert_eq!(d.saves.len(), 0);
        assert_eq!(d.warnings.len(), 1);
        assert_eq!(
            d.warnings[0].code,
            leemusync_api::WarningCode::UnreadableFolder
        );
    }
}
