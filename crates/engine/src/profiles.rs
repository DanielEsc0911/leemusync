//! Bundled profiles (D8) and where an emulator is installed on this machine.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use leemusync_api::{Warning, WarningCode};
use leemusync_core::profile::{Profile, expand};

use crate::{EngineError, Locations};

const BUNDLED: [&str; 1] = [include_str!("../../../profiles/cemu.toml")];
const MAX_CONFIG_FILE: u64 = 1024 * 1024;

pub struct Install {
    pub emulator: String,
    pub base: PathBuf,
    pub config_dir: PathBuf,
    pub saves_root: PathBuf,
    pub accounts_root: Option<PathBuf>,
    pub warnings: Vec<Warning>,
    /// True when the emulator's config file exists but could not be read because it isn't
    /// granted, so the paths above come from profile defaults and may be stale. Sync must refuse
    /// provisional installs (Task 15); scan only reports them.
    #[allow(dead_code)] // used by Task 15
    pub provisional: bool,
}

/// # Errors
/// `Config` if a bundled profile is invalid (a release bug).
pub fn bundled() -> Result<Vec<Profile>, EngineError> {
    BUNDLED
        .iter()
        .map(|t| Profile::from_toml(t).map_err(|e| EngineError::Config(e.to_string())))
        .collect()
}

fn config_err(e: &impl ToString) -> EngineError {
    EngineError::Config(e.to_string())
}

/// A Flatpak app id is a path component built from reverse-DNS names: nothing else is allowed.
fn valid_app_id(app: &str) -> bool {
    !app.is_empty()
        && app != "."
        && app != ".."
        && app
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn vars(profile: &Profile, loc: &Locations) -> Result<BTreeMap<String, String>, EngineError> {
    let mut v = BTreeMap::new();
    v.insert("home".to_owned(), loc.home.display().to_string());
    v.insert("xdg_data".to_owned(), loc.xdg_data.display().to_string());
    v.insert(
        "xdg_config".to_owned(),
        loc.xdg_config.display().to_string(),
    );
    if let Some(linux) = profile.platforms.get("linux") {
        for t in linux
            .roots
            .iter()
            .chain(linux.config_roots.iter().flatten())
        {
            if let Some(app) = t
                .strip_prefix("{flatpak:")
                .and_then(|r| r.split('}').next())
            {
                if !valid_app_id(app) {
                    return Err(EngineError::Config("invalid_flatpak_app_id".into()));
                }
                v.insert(
                    format!("flatpak:{app}"),
                    loc.home.join(".var/app").join(app).display().to_string(),
                );
            }
        }
    }
    Ok(v)
}

/// First existing root in the profile's order (or the user's override). `granted` gates every read.
/// A path read from the emulator's own config is never trusted as granted: callers check it again.
///
/// # Errors
/// `Config` for template errors in the profile.
pub fn resolve(
    profile: &Profile,
    loc: &Locations,
    override_root: Option<&Path>,
    granted: &dyn Fn(&Path) -> bool,
) -> Result<Option<Install>, EngineError> {
    let Some(linux) = profile.platforms.get("linux") else {
        return Ok(None);
    };
    let mut vars = vars(profile, loc)?;
    let mut found = None;
    for (i, root) in linux.roots.iter().enumerate() {
        let Ok(base) = expand(root, &vars) else {
            continue;
        };
        let base = PathBuf::from(base);
        let wanted = override_root.is_none_or(|o| o == base);
        if wanted && base.is_dir() {
            let config = linux
                .config_roots
                .as_ref()
                .and_then(|c| c.get(i))
                .map_or(Ok(base.display().to_string()), |c| expand(c, &vars))
                .map_err(|e| config_err(&e))?;
            found = Some((base, PathBuf::from(config)));
            break;
        }
    }
    if found.is_none()
        && let Some(o) = override_root.filter(|o| o.is_dir())
    {
        found = Some((o.to_path_buf(), o.to_path_buf()));
    }
    let Some((base, config_dir)) = found else {
        return Ok(None);
    };
    vars.insert("base".to_owned(), base.display().to_string());
    let mut warnings = Vec::new();
    let mut provisional = false;
    for key in &profile.config_keys {
        let file = config_dir.join(&key.file);
        let value = if granted(&file) {
            read_xml_key(&file, &key.key)
        } else {
            if file.exists() {
                provisional = true;
                warnings.push(Warning {
                    code: WarningCode::NotGranted,
                    detail: config_dir.display().to_string(),
                });
            }
            None
        };
        let resolved = match value.filter(|v| Path::new(v).is_absolute()) {
            Some(v) => v,
            None => expand(&key.default, &vars).map_err(|e| config_err(&e))?,
        };
        vars.insert(key.var.clone(), resolved);
    }
    let saves_root = PathBuf::from(expand(&profile.saves.root, &vars).map_err(|e| config_err(&e))?);
    let accounts_root = profile
        .accounts
        .as_ref()
        .map(|a| expand(&a.root, &vars).map(PathBuf::from))
        .transpose()
        .map_err(|e| config_err(&e))?;
    Ok(Some(Install {
        emulator: profile.id.clone(),
        base,
        config_dir,
        saves_root,
        accounts_root,
        warnings,
        provisional,
    }))
}

/// `content/mlc_path`-style element path in a small XML file; `None` when missing, empty or
/// unreadable. The file is untrusted: the read is bounded, not just the metadata.
fn read_xml_key(file: &Path, key: &str) -> Option<String> {
    let mut text = String::new();
    std::fs::File::open(file)
        .ok()?
        .take(MAX_CONFIG_FILE + 1)
        .read_to_string(&mut text)
        .ok()?;
    if text.len() as u64 > MAX_CONFIG_FILE {
        return None;
    }
    let doc = roxmltree::Document::parse(&text).ok()?;
    let mut node = doc.root_element();
    let mut parts = key.split('/');
    if parts.next()? != node.tag_name().name() {
        return None;
    }
    for part in parts {
        node = node.children().find(|n| n.has_tag_name(part))?;
    }
    let value = node.text().unwrap_or("").trim().to_owned();
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cemu() -> Profile {
        bundled().unwrap().remove(0)
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn flatpak_wins_over_native_when_both_exist_and_native_is_used_alone() {
        let dir = tempfile::tempdir().unwrap();
        let loc = Locations::under(dir.path());
        let native = loc.xdg_data.join("Cemu");
        std::fs::create_dir_all(&native).unwrap();
        let i = resolve(&cemu(), &loc, None, &|_| true).unwrap().unwrap();
        assert_eq!(i.base, native);
        assert_eq!(i.config_dir, loc.xdg_config.join("Cemu"));
        assert_eq!(i.saves_root, native.join("mlc01/usr/save"));
        let flat = loc.home.join(".var/app/info.cemu.Cemu/data/Cemu");
        std::fs::create_dir_all(&flat).unwrap();
        let i = resolve(&cemu(), &loc, None, &|_| true).unwrap().unwrap();
        assert_eq!(i.base, flat);
        assert_eq!(
            i.config_dir,
            loc.home.join(".var/app/info.cemu.Cemu/config/Cemu")
        );
    }

    #[test]
    fn nothing_installed_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let loc = Locations::under(dir.path());
        assert!(resolve(&cemu(), &loc, None, &|_| true).unwrap().is_none());
    }

    #[test]
    fn settings_xml_mlc_path_is_used_only_when_granted_and_absolute() {
        let dir = tempfile::tempdir().unwrap();
        let loc = Locations::under(dir.path());
        let native = loc.xdg_data.join("Cemu");
        std::fs::create_dir_all(&native).unwrap();
        let settings = loc.xdg_config.join("Cemu/settings.xml");
        write(
            &settings,
            "<content><mlc_path>/elsewhere/mlc</mlc_path></content>",
        );
        let i = resolve(&cemu(), &loc, None, &|_| true).unwrap().unwrap();
        assert_eq!(i.saves_root, Path::new("/elsewhere/mlc/usr/save"));
        let i = resolve(&cemu(), &loc, None, &|_| false).unwrap().unwrap();
        assert_eq!(i.saves_root, native.join("mlc01/usr/save"));
        assert_eq!(i.warnings.len(), 1);
        write(
            &settings,
            "<content><mlc_path>relative/mlc</mlc_path></content>",
        );
        let i = resolve(&cemu(), &loc, None, &|_| true).unwrap().unwrap();
        assert_eq!(i.saves_root, native.join("mlc01/usr/save"));
    }

    #[test]
    fn an_oversized_or_broken_settings_file_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.xml");
        let pad = " ".repeat(usize::try_from(MAX_CONFIG_FILE).unwrap());
        write(
            &file,
            &format!("<content><mlc_path>/x</mlc_path>{pad}</content>"),
        );
        assert_eq!(read_xml_key(&file, "content/mlc_path"), None);
        write(&file, "<content><mlc_path>/x</mlc_path>");
        assert_eq!(read_xml_key(&file, "content/mlc_path"), None);
        write(&file, "<content><mlc_path>/x</mlc_path></content>");
        assert_eq!(
            read_xml_key(&file, "content/mlc_path").as_deref(),
            Some("/x")
        );
    }

    #[test]
    fn an_install_is_provisional_only_when_the_config_exists_but_is_not_granted() {
        let dir = tempfile::tempdir().unwrap();
        let loc = Locations::under(dir.path());
        std::fs::create_dir_all(loc.xdg_data.join("Cemu")).unwrap();
        let none = resolve(&cemu(), &loc, None, &|_| false).unwrap().unwrap();
        assert!(
            !none.provisional,
            "no settings.xml: the default is the answer"
        );
        write(
            &loc.xdg_config.join("Cemu/settings.xml"),
            "<content></content>",
        );
        let denied = resolve(&cemu(), &loc, None, &|_| false).unwrap().unwrap();
        assert!(denied.provisional);
        let granted = resolve(&cemu(), &loc, None, &|_| true).unwrap().unwrap();
        assert!(!granted.provisional);
    }

    #[test]
    fn flatpak_app_ids_are_validated() {
        for ok in ["info.cemu.Cemu", "a-b_c.D1"] {
            assert!(valid_app_id(ok), "{ok}");
        }
        for bad in ["", "..", ".", "a/b", "a\\b", "a b", "../x", "a:b", "é"] {
            assert!(!valid_app_id(bad), "{bad}");
        }
    }
}
