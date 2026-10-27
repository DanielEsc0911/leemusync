//! Emulator profile schema (subset used in Phase 1), templates and save patterns.
//! Spec: docs/specs/emulator-profiles.md (Schema, Templates, Validation).

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

use serde::Deserialize;

const CAPTURES: [&str; 3] = ["title_id", "account", "game"];
pub const ALLOWED_VARS: [&str; 8] = [
    "base",
    "mlc",
    "title_id",
    "account",
    "home",
    "xdg_data",
    "xdg_config",
    "exe_dir",
];
const DEFAULT_MAX_FILE_MB: u32 = 64;
/// The variables `launch.args` may use; each is a whole argument, never part of one.
pub const GAME_PATH_VAR: &str = "{game_path}";
pub const ACCOUNT_VAR: &str = "{account}";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProfileError {
    #[error("profile TOML: {0}")]
    Toml(String),
    #[error("profile field is invalid: {0}")]
    Field(&'static str),
    #[error("profile uses an unsupported value: {0}")]
    Unsupported(String),
    #[error("unknown template variable `{0}`")]
    UnknownVar(String),
    #[error("template variable `{0}` has no value here")]
    MissingVar(String),
    #[error("malformed template or pattern")]
    Malformed,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub systems: Vec<String>,
    pub layout: String,
    pub requires_closed: bool,
    pub platforms: BTreeMap<String, Platform>,
    #[serde(default)]
    pub config_keys: Vec<ConfigKey>,
    pub saves: Saves,
    pub accounts: Option<Accounts>,
    pub game_key: GameKey,
    /// How to start a game. Arguments only: the executable is always the user's (config).
    pub launch: Option<Launch>,
    /// How the emulator lists its installed games (`[games]`, emulator-profiles §Game discovery).
    pub games: Option<Games>,
}

/// `[games]`: where the emulator keeps its game folders and how to read one game.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Games {
    pub dirs: Option<GameDirs>,
    pub scan: Option<GameScan>,
}

/// `games.dirs`: a repeated XML element whose text is one game folder.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameDirs {
    /// File name in the config root, like `config_keys.file`.
    pub file: String,
    /// Element path from the root element, `content/GamePaths/Entry`.
    pub key: String,
}

/// `games.scan`: a folder holding `marker` is one game; `key` and `name` are element paths in it.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameScan {
    /// Relative file path, `meta/meta.xml`.
    pub marker: String,
    pub key: String,
    /// Keep only the last N characters of the key (Cemu: the low 8 hex digits of the title id).
    pub key_last: Option<usize>,
    pub name: String,
    /// The file to launch is the only `*.<launch_ext>` in `<launch_dir>`; else the folder itself.
    pub launch_dir: Option<String>,
    pub launch_ext: Option<String>,
}

/// `a/b/c`: non-empty components of ASCII letters, digits, `_`, `-`, `.`, never `.` or `..`.
fn check_rel(path: &str) -> bool {
    path.len() <= 200
        && path.split('/').all(|c| {
            !c.is_empty()
                && c != "."
                && c != ".."
                && c.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        })
}

impl Games {
    fn validate(&self) -> Result<(), ProfileError> {
        let bad = || ProfileError::Field("games");
        if let Some(d) = &self.dirs
            && (!check_rel(&d.file)
                || d.file.contains('/')
                || d.file.starts_with('.')
                || !check_rel(&d.key))
        {
            return Err(bad());
        }
        if let Some(s) = &self.scan {
            let ok = check_rel(&s.marker)
                && check_rel(&s.key)
                && check_rel(&s.name)
                && s.key_last.is_none_or(|n| (1..=64).contains(&n))
                && s.launch_dir.as_deref().is_none_or(check_rel)
                && s.launch_ext
                    .as_deref()
                    .is_none_or(|e| check_rel(e) && !e.contains(['/', '.']))
                && s.launch_dir.is_some() == s.launch_ext.is_some();
            if !ok {
                return Err(bad());
            }
        }
        Ok(())
    }
}

/// `[launch]`. `deny_unknown_fields` keeps a profile from naming an executable or a shell.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Platform {
    pub roots: Vec<String>,
    pub config_roots: Option<Vec<String>>,
    pub process: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigKey {
    pub file: String,
    pub format: String,
    pub key: String,
    pub var: String,
    pub default: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Saves {
    pub root: String,
    pub pattern: String,
    #[serde(default)]
    pub shared: Vec<String>,
    pub max_file_mb: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accounts {
    pub root: String,
    pub id_regex: String,
    pub name_file: String,
    pub name_key: String,
    /// How to write a new account (`[accounts.create]`, D32).
    pub create: Option<crate::account_template::CreateTemplate>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameKey {
    pub from: String,
    pub normalize: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternPart {
    Literal(String),
    Var(String),
}

impl Profile {
    /// # Errors
    /// `Toml` for syntax or unknown fields, else the first validation rule broken.
    pub fn from_toml(text: &str) -> Result<Self, ProfileError> {
        let p: Self = toml::from_str(text).map_err(|e| ProfileError::Toml(e.to_string()))?;
        p.validate()?;
        Ok(p)
    }

    #[must_use]
    pub fn max_file_bytes(&self) -> u64 {
        u64::from(self.saves.max_file_mb.unwrap_or(DEFAULT_MAX_FILE_MB)) * 1024 * 1024
    }

    fn validate(&self) -> Result<(), ProfileError> {
        if self.schema != 1 {
            return Err(ProfileError::Unsupported(format!("schema {}", self.schema)));
        }
        if self.id.is_empty()
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(ProfileError::Field("id"));
        }
        if self.layout != "per_game_user_dir" {
            return Err(ProfileError::Unsupported(self.layout.clone()));
        }
        if self
            .saves
            .max_file_mb
            .is_some_and(|mb| mb == 0 || mb > 4096)
        {
            return Err(ProfileError::Field("saves.max_file_mb"));
        }
        for platform in self.platforms.values() {
            for t in platform
                .roots
                .iter()
                .chain(platform.config_roots.iter().flatten())
            {
                check_template(t)?;
            }
            if platform
                .config_roots
                .as_ref()
                .is_some_and(|c| c.len() != platform.roots.len())
            {
                return Err(ProfileError::Field("config_roots"));
            }
            if platform
                .process
                .iter()
                .any(|p| p.is_empty() || p.contains(['/', '\\']))
            {
                return Err(ProfileError::Field("process"));
            }
        }
        for key in &self.config_keys {
            check_template(&key.default)?;
            if key.format != "xml" || key.file.contains(['/', '\\']) || key.file.starts_with('.') {
                return Err(ProfileError::Field("config_keys"));
            }
        }
        check_template(&self.saves.root)?;
        let parts = parse_pattern(&self.saves.pattern)?;
        for needed in ["title_id", "account"] {
            if !parts.contains(&PatternPart::Var(needed.into())) {
                return Err(ProfileError::Field("saves.pattern"));
            }
        }
        for shared in &self.saves.shared {
            parse_pattern(shared)?;
        }
        if let Some(acc) = &self.accounts {
            check_template(&acc.root)?;
            if acc.id_regex.len() > 200 || acc.name_file.contains(['/', '\\']) {
                return Err(ProfileError::Field("accounts"));
            }
            if let Some(create) = &acc.create {
                use crate::account_template::TemplateError;
                create.validate().map_err(|e| match e {
                    TemplateError::Unknown(name) => ProfileError::UnknownVar(name),
                    _ => ProfileError::Field("accounts.create"),
                })?;
            }
        }
        if self.game_key.from != "title_id"
            || self.game_key.normalize.iter().any(|n| n != "lowercase")
        {
            return Err(ProfileError::Field("game_key"));
        }
        if let Some(launch) = &self.launch {
            check_launch(&launch.args)?;
        }
        if let Some(games) = &self.games {
            games.validate()?;
        }
        Ok(())
    }

    /// The profile's launch arguments with `{game_path}` replaced by `game_path` and `{account}`
    /// by the player's `account`, each one argument whatever it contains (spaces, quotes, `;`).
    /// `None` when the profile can't launch.
    #[must_use]
    pub fn launch_args(&self, game_path: &OsStr, account: &str) -> Option<Vec<OsString>> {
        let launch = self.launch.as_ref()?;
        Some(
            launch
                .args
                .iter()
                .map(|a| {
                    if a == GAME_PATH_VAR {
                        game_path.to_owned()
                    } else if a == ACCOUNT_VAR {
                        OsString::from(account)
                    } else {
                        OsString::from(a)
                    }
                })
                .collect(),
        )
    }
}

/// Each argument is `{game_path}` or `{account}` exactly, or a literal without braces or NUL.
fn check_launch(args: &[String]) -> Result<(), ProfileError> {
    for a in args
        .iter()
        .filter(|a| *a != GAME_PATH_VAR && *a != ACCOUNT_VAR)
    {
        if let Some(start) = a.find('{') {
            let name = a[start + 1..].split('}').next().unwrap_or_default();
            return Err(ProfileError::UnknownVar(name.to_owned()));
        }
        if a.is_empty() || a.contains(['}', '\0']) || a.len() > 256 {
            return Err(ProfileError::Field("launch.args"));
        }
    }
    if args.len() > 32 || !args.iter().any(|a| a == GAME_PATH_VAR) {
        return Err(ProfileError::Field("launch.args"));
    }
    Ok(())
}

fn is_allowed(name: &str) -> bool {
    ALLOWED_VARS.contains(&name)
        || name.strip_prefix("flatpak:").is_some_and(|app| {
            !app.is_empty()
                && app
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
}

fn check_template(t: &str) -> Result<(), ProfileError> {
    expand_with(t, |_| Ok(String::new())).map(|_| ())
}

/// Replaces every `{name}` in `template` with its value from `vars`.
///
/// # Errors
/// `UnknownVar` for a name outside `ALLOWED_VARS` and `flatpak:<app id>`,
/// `MissingVar` when `vars` has no value, `Malformed` for unbalanced braces.
pub fn expand(template: &str, vars: &BTreeMap<String, String>) -> Result<String, ProfileError> {
    expand_with(template, |name| {
        vars.get(name)
            .cloned()
            .ok_or_else(|| ProfileError::MissingVar(name.to_owned()))
    })
}

fn expand_with(
    template: &str,
    mut value: impl FnMut(&str) -> Result<String, ProfileError>,
) -> Result<String, ProfileError> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('}').ok_or(ProfileError::Malformed)?;
        let name = &after[..end];
        if name.contains('{') {
            return Err(ProfileError::Malformed);
        }
        if !is_allowed(name) {
            return Err(ProfileError::UnknownVar(name.to_owned()));
        }
        out.push_str(&value(name)?);
        rest = &after[end + 1..];
    }
    if rest.contains('}') {
        return Err(ProfileError::Malformed);
    }
    out.push_str(rest);
    Ok(out)
}

/// Splits a relative save pattern into literal and `{var}` components.
///
/// # Errors
/// `Malformed` for absolute paths, `..`, empty components or a variable that isn't a whole component,
/// `UnknownVar` for a variable that is not a capture (`title_id`, `account`, `game`): path variables
/// such as `{home}` expand to absolute paths and would escape the save root.
pub fn parse_pattern(pattern: &str) -> Result<Vec<PatternPart>, ProfileError> {
    pattern
        .split('/')
        .map(|c| {
            if let Some(name) = c.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
                if CAPTURES.contains(&name) {
                    Ok(PatternPart::Var(name.to_owned()))
                } else {
                    Err(ProfileError::UnknownVar(name.to_owned()))
                }
            } else if c.is_empty() || c == "." || c == ".." || c.contains(['{', '}', '\\', ':']) {
                Err(ProfileError::Malformed)
            } else {
                Ok(PatternPart::Literal(c.to_owned()))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CEMU: &str = include_str!("../../../profiles/cemu.toml");
    const LAUNCH: &str = "[launch]\nargs = [\"-g\", \"{game_path}\", \"-a\", \"{account}\"]\n";

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn the_bundled_cemu_profile_is_valid() {
        let p = Profile::from_toml(CEMU).unwrap();
        assert_eq!(p.id, "cemu");
        let linux = &p.platforms["linux"];
        assert_eq!(
            linux.roots.len(),
            linux.config_roots.as_ref().unwrap().len()
        );
        assert!(linux.process.contains(&"cemu".to_owned()));
        assert_eq!(p.max_file_bytes(), 64 * 1024 * 1024);
    }

    #[test]
    fn cemu_launch_args_put_the_game_path_in_one_argument() {
        let p = Profile::from_toml(CEMU).unwrap();
        let args = p
            .launch_args(OsStr::new("/g/My Game; rm -rf ~/x.wux"), "80000001")
            .unwrap();
        assert_eq!(
            args,
            vec![
                OsString::from("-g"),
                OsString::from("/g/My Game; rm -rf ~/x.wux"),
                OsString::from("-a"),
                OsString::from("80000001")
            ]
        );
    }

    #[test]
    fn launch_rejects_other_variables_and_any_executable() {
        let with = |launch: &str| CEMU.to_owned() + "\n" + launch;
        for (launch, want) in [
            (
                "[launch]\nargs = [\"-g\", \"{home}/x\"]",
                ProfileError::UnknownVar("home".into()),
            ),
            (
                "[launch]\nargs = [\"--path={game_path}\"]",
                ProfileError::UnknownVar("game_path".into()),
            ),
            (
                "[launch]\nargs = [\"-a{account}\", \"{game_path}\"]",
                ProfileError::UnknownVar("account".into()),
            ),
            (
                "[launch]\nargs = [\"-g\"]",
                ProfileError::Field("launch.args"),
            ),
            (
                "[launch]\nargs = [\"-a\", \"{account}\"]",
                ProfileError::Field("launch.args"),
            ),
        ] {
            let text = with(launch).replace(LAUNCH, "");
            assert_eq!(Profile::from_toml(&text).unwrap_err(), want, "{launch}");
        }
        for field in ["exe", "command", "shell", "program"] {
            let text = CEMU.replace(
                "[launch]\n",
                &format!("[launch]\n{field} = [\"/bin/sh\", \"-c\"]\n"),
            );
            assert!(
                matches!(Profile::from_toml(&text), Err(ProfileError::Toml(_))),
                "{field}"
            );
        }
    }

    #[test]
    fn a_profile_without_launch_cannot_launch() {
        let text = CEMU.replace(LAUNCH, "");
        assert!(
            Profile::from_toml(&text)
                .unwrap()
                .launch_args(OsStr::new("g"), "80000001")
                .is_none()
        );
    }

    #[test]
    fn expands_known_variables_only() {
        let v = vars(&[
            ("xdg_data", "/home/u/.local/share"),
            ("flatpak:info.cemu.Cemu", "/home/u/.var/app/info.cemu.Cemu"),
        ]);
        assert_eq!(
            expand("{xdg_data}/Cemu", &v).unwrap(),
            "/home/u/.local/share/Cemu"
        );
        assert_eq!(
            expand("{flatpak:info.cemu.Cemu}/data/Cemu", &v).unwrap(),
            "/home/u/.var/app/info.cemu.Cemu/data/Cemu"
        );
        assert_eq!(
            expand("{exe_dir}/portable", &v),
            Err(ProfileError::MissingVar("exe_dir".into()))
        );
        assert_eq!(
            expand("{nope}/x", &v),
            Err(ProfileError::UnknownVar("nope".into()))
        );
        assert!(expand("{xdg_data", &v).is_err());
    }

    #[test]
    fn expands_home_when_supplied() {
        let v = vars(&[("home", "/home/u")]);
        assert_eq!(expand("{home}/x", &v).unwrap(), "/home/u/x");
    }

    #[test]
    fn rejects_a_bad_size_limit() {
        for bad in ["0", "4097"] {
            let text = CEMU.replace("max_file_mb = 64", &format!("max_file_mb = {bad}"));
            assert_eq!(
                Profile::from_toml(&text).unwrap_err(),
                ProfileError::Field("saves.max_file_mb")
            );
        }
        let none = CEMU.replace("max_file_mb = 64\n", "");
        assert_eq!(
            Profile::from_toml(&none).unwrap().max_file_bytes(),
            64 * 1024 * 1024
        );
    }

    #[test]
    fn patterns_only_allow_captures() {
        assert_eq!(
            parse_pattern("{home}/x"),
            Err(ProfileError::UnknownVar("home".into()))
        );
        assert_eq!(
            parse_pattern("00050000/{mlc}/user/{account}"),
            Err(ProfileError::UnknownVar("mlc".into()))
        );
        assert!(parse_pattern("{game}/{account}").is_ok());
        let bad_shared = CEMU.replace("00050000/{title_id}/user/common", "{exe_dir}");
        assert_eq!(
            Profile::from_toml(&bad_shared).unwrap_err(),
            ProfileError::UnknownVar("exe_dir".into())
        );
        let bad_pattern = CEMU.replace(
            "00050000/{title_id}/user/{account}",
            "{home}/{title_id}/{account}",
        );
        assert!(matches!(
            Profile::from_toml(&bad_pattern),
            Err(ProfileError::UnknownVar(_))
        ));
    }

    #[test]
    fn parses_save_patterns() {
        assert_eq!(
            parse_pattern("00050000/{title_id}/user/{account}").unwrap(),
            vec![
                PatternPart::Literal("00050000".into()),
                PatternPart::Var("title_id".into()),
                PatternPart::Literal("user".into()),
                PatternPart::Var("account".into()),
            ]
        );
        assert!(parse_pattern("../{account}").is_err());
        assert!(parse_pattern("/abs/{account}").is_err());
        assert!(
            parse_pattern("a{account}").is_err(),
            "a variable must be a whole component"
        );
    }

    #[test]
    fn games_section_is_optional_and_checked() {
        let p = Profile::from_toml(CEMU).unwrap();
        let g = p.games.unwrap();
        assert_eq!(g.dirs.unwrap().key, "content/GamePaths/Entry");
        assert_eq!(g.scan.unwrap().key_last, Some(8));
        let start = CEMU.find("[games]").unwrap();
        let end = start + CEMU[start..].find("\n\n").unwrap();
        let without = format!("{}{}", &CEMU[..start], &CEMU[end..]);
        assert!(Profile::from_toml(&without).unwrap().games.is_none());
        for (from, to) in [
            ("{ file = \"settings.xml\"", "{ file = \"../settings.xml\""),
            ("{ file = \"settings.xml\"", "{ file = \"a/settings.xml\""),
            ("marker = \"meta/meta.xml\"", "marker = \"../meta.xml\""),
            ("marker = \"meta/meta.xml\"", "marker = \"/etc/passwd\""),
            ("key_last = 8", "key_last = 0"),
            ("launch_ext = \"rpx\"", "launch_ext = \"r/x\""),
            (", launch_ext = \"rpx\"", ""),
        ] {
            let text = CEMU.replace(from, to);
            assert_ne!(text, CEMU, "{from}");
            assert_eq!(
                Profile::from_toml(&text).unwrap_err(),
                ProfileError::Field("games"),
                "{to}"
            );
        }
        let exe = CEMU.replace("[games]\n", "[games]\nexe = \"/bin/sh\"\n");
        assert!(matches!(
            Profile::from_toml(&exe),
            Err(ProfileError::Toml(_))
        ));
    }

    #[test]
    fn rejects_profiles_that_break_the_rules() {
        let bad_layout = CEMU.replace("per_game_user_dir", "container");
        assert!(matches!(
            Profile::from_toml(&bad_layout),
            Err(ProfileError::Unsupported(_))
        ));
        let bad_var = CEMU.replace("{xdg_data}/Cemu", "{nope}/Cemu");
        assert!(matches!(
            Profile::from_toml(&bad_var),
            Err(ProfileError::UnknownVar(_))
        ));
        let extra = format!("{CEMU}\nrun = \"/bin/sh\"\n");
        assert!(
            matches!(Profile::from_toml(&extra), Err(ProfileError::Toml(_))),
            "no unknown fields"
        );
        let bad_process = CEMU.replace("\"cemu\"]", "\"/usr/bin/cemu\"]");
        assert!(matches!(
            Profile::from_toml(&bad_process),
            Err(ProfileError::Field(_))
        ));
    }
}
