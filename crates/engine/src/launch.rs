//! The argv that starts a game: the user's executable prefix, then the profile's `launch.args`
//! (emulator-profiles §Schema, config-and-cli §Config file). argv only, never a shell (B2).

use std::ffi::OsString;

use crate::{Engine, EngineError, profiles};

impl Engine {
    /// `exe` from `[emulators.<emulator>]`, then the profile's `launch.args` with `{game_path}`
    /// set to `games."<title id>"` and `{account}` to the player's `account` on this device,
    /// each as one argument. Nothing is joined into a string.
    ///
    /// # Errors
    /// `Usage` for an unknown emulator or one whose profile can't launch; `NotSetUp` when `exe`
    /// or the game's path isn't configured; `Config` for an empty `exe` element or a relative
    /// game path (it could be read as an option).
    pub fn launch_argv(
        &self,
        emulator: &str,
        game: &str,
        account: &str,
    ) -> Result<Vec<OsString>, EngineError> {
        let profile = profiles::bundled()?
            .into_iter()
            .find(|p| p.id == emulator)
            .ok_or_else(|| EngineError::Usage(format!("unknown emulator {emulator}")))?;
        let settings = self.config.emulators.get(emulator);
        let exe = settings.map(|s| s.exe.as_slice()).unwrap_or_default();
        if exe.is_empty() {
            return Err(EngineError::NotSetUp(
                "emulator executable ([emulators.<id>] exe)",
            ));
        }
        if exe.iter().any(String::is_empty) {
            return Err(EngineError::Config(format!(
                "emulators.{emulator}.exe has an empty element"
            )));
        }
        let path = settings
            .and_then(|s| s.games.get(&game.to_ascii_lowercase()))
            .ok_or(EngineError::NotSetUp("game path ([emulators.<id>] games)"))?;
        if !path.is_absolute() {
            return Err(EngineError::Config(format!(
                "emulators.{emulator}.games.{game} must be an absolute path"
            )));
        }
        let args = profile
            .launch_args(path.as_os_str(), account)
            .ok_or_else(|| EngineError::Usage(format!("{emulator} can't launch games")))?;
        Ok(exe.iter().map(OsString::from).chain(args).collect())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::tests::test_env;
    use crate::{EmulatorOverride, Engine, EngineError};

    fn engine(dir: &std::path::Path, exe: &[&str], games: &[(&str, &str)]) -> Engine {
        let mut e = Engine::open(test_env(dir)).unwrap();
        e.config.emulators.insert(
            "cemu".into(),
            EmulatorOverride {
                root: None,
                exe: exe.iter().map(|s| (*s).to_owned()).collect(),
                games: games
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), PathBuf::from(v)))
                    .collect(),
            },
        );
        e
    }

    #[test]
    fn builds_argv_with_the_game_path_as_one_element() {
        let dir = tempfile::tempdir().unwrap();
        // Absolute on every OS: a Unix-style `/games/…` isn't absolute on Windows.
        let game = dir.path().join("Zelda BotW; echo pwned").join("game.wux");
        let game = game.to_str().unwrap();
        let e = engine(
            dir.path(),
            &["flatpak", "run", "info.cemu.Cemu"],
            &[("101c9500", game)],
        );
        let argv = e.launch_argv("cemu", "101C9500", "80000002").unwrap();
        assert_eq!(
            argv,
            [
                "flatpak",
                "run",
                "info.cemu.Cemu",
                "-g",
                game,
                "-a",
                "80000002"
            ]
        );
    }

    #[test]
    fn refuses_a_missing_or_empty_exe_and_a_missing_or_relative_game() {
        let dir = tempfile::tempdir().unwrap();
        let games = [("101c9500", "/g/game.wux"), ("101c9400", "-h")];
        let none = engine(dir.path(), &[], &games);
        assert!(matches!(
            none.launch_argv("cemu", "101c9500", "80000001"),
            Err(EngineError::NotSetUp(_))
        ));
        drop(none);
        let blank = engine(dir.path(), &[""], &games);
        assert!(matches!(
            blank.launch_argv("cemu", "101c9500", "80000001"),
            Err(EngineError::Config(_))
        ));
        drop(blank);
        let e = engine(dir.path(), &["/usr/bin/cemu"], &games);
        assert!(matches!(
            e.launch_argv("cemu", "101c9300", "80000001"),
            Err(EngineError::NotSetUp(_))
        ));
        assert!(matches!(
            e.launch_argv("cemu", "101c9400", "80000001"),
            Err(EngineError::Config(_))
        ));
        assert!(matches!(
            e.launch_argv("dolphin", "x", "80000001"),
            Err(EngineError::Usage(_))
        ));
    }
}
