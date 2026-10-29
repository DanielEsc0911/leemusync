//! The engine calls behind the CLI's requests when it talks to the daemon (D35): one engine
//! call per request, the same calls the CLI makes in-process, so both paths give the same
//! results and the same errors. The service runs them one at a time.

use std::ffi::OsString;
use std::path::Path;

use lemusync_api::{
    ConfigOp, GameName, GameRef, JoinWire, MapChoiceWire, MapResult, Request, Response,
    WithWarnings,
};

use crate::{Engine, EngineError, JoinSecret, MapChoice, MapOutcome, S3Backend, VersionId};

/// True when the request may change what `status` reports or which folders are watched
/// (the service refreshes both after it).
#[must_use]
pub(crate) fn changes_setup(r: &Request) -> bool {
    matches!(
        r,
        Request::Init { .. }
            | Request::Join { .. }
            | Request::BackendAddFs { .. }
            | Request::BackendAddS3 { .. }
            | Request::GrantAdd { .. }
            | Request::PlayersAdd { .. }
            | Request::Scan
            | Request::AccountsMap { .. }
            | Request::AccountsCreate { .. }
            | Request::AccountsRecover { .. }
            | Request::ConfirmFirstSync
            | Request::Restore { .. }
            | Request::Resolve { .. }
            | Request::LeaseRelease { .. }
            | Request::AliasAdd { .. }
            | Request::AliasRemove { .. }
            | Request::Config(ConfigOp::Set { .. } | ConfigOp::Unset { .. })
    )
}

/// The `history` steps of the CLI: resolve the game, then the given player or the only one.
///
/// # Errors
/// What the engine returns; `Usage("player_needed")` when the player is ambiguous.
pub(crate) async fn history(
    engine: &mut Engine,
    game: &str,
    emu: Option<&str>,
    player: Option<String>,
) -> Result<Response, EngineError> {
    let (emulator, game) = engine.resolve_game(game, emu).await?;
    let player = match player {
        Some(p) => p,
        None => only_player(engine.players_list().await?)?,
    };
    Ok(Response::History(
        engine.history(&player, &emulator, &game).await?,
    ))
}

/// The only player in the repo; "which player?" otherwise (the CLI's rule).
///
/// # Errors
/// `Usage("player_needed")`.
pub fn only_player(mut players: Vec<String>) -> Result<String, EngineError> {
    if players.len() == 1 {
        Ok(players.remove(0))
    } else {
        Err(EngineError::Usage("player_needed".into()))
    }
}

fn config(engine: &mut Engine, op: ConfigOp) -> Result<Response, EngineError> {
    Ok(match op {
        ConfigOp::Path => Response::Path(engine.config_path().display().to_string()),
        ConfigOp::List => Response::ConfigValues(engine.config_values()?),
        ConfigOp::Get { key } => Response::ConfigValue(engine.config_get(&key)?),
        ConfigOp::Set { key, values } => {
            engine.config_set(&key, &values)?;
            Response::Done
        }
        ConfigOp::Unset { key } => {
            engine.config_unset(&key)?;
            Response::Done
        }
    })
}

/// Runs one CLI request that isn't answered by the service itself. `None` for the requests
/// the service handles (status, sync, pause, play, …).
#[allow(clippy::too_many_lines)] // one arm per request, each one engine call
pub(crate) async fn run(engine: &mut Engine, r: Request) -> Option<Result<Response, EngineError>> {
    let out = match r {
        Request::Init { passphrase } => {
            engine
                .init(passphrase.expose())
                .await
                .map(|(report, recovery)| Response::Init {
                    report,
                    recovery: lemusync_api::Secret::new(recovery),
                })
        }
        Request::Join { secret } => {
            let secret = match secret {
                JoinWire::Passphrase(s) => JoinSecret::Passphrase(s.expose().to_owned()),
                JoinWire::Recovery(s) => JoinSecret::Recovery(s.expose().to_owned()),
            };
            engine.join(secret).await.map(Response::Join)
        }
        Request::BackendAddFs { path } => engine
            .backend_add_fs(Path::new(&path))
            .map(|()| Response::Done),
        Request::BackendAddS3 {
            endpoint,
            bucket,
            region,
            access_key_id,
            secret,
        } => {
            let backend = S3Backend {
                endpoint,
                bucket,
                region,
                access_key_id,
            };
            engine
                .backend_add_s3(backend, secret.expose())
                .await
                .map(|()| Response::Done)
        }
        Request::GrantAdd { path } => engine
            .grant_add(Path::new(&path))
            .map(|p| Response::Path(p.display().to_string())),
        Request::GrantList => Ok(Response::Names(
            engine
                .grant_list()
                .iter()
                .map(|g| g.display().to_string())
                .collect(),
        )),
        Request::PlayersAdd { name } => engine.players_add(&name).await.map(Response::Warnings),
        Request::PlayersList => engine.players_list().await.map(Response::Names),
        Request::Scan => engine.scan().await.map(Response::Scan),
        Request::AccountsMap {
            player,
            emulator,
            account,
            choice,
        } => {
            let choice = match choice {
                MapChoiceWire::Ask => MapChoice::Ask,
                MapChoiceWire::Keep => MapChoice::Keep,
                MapChoiceWire::Replace => MapChoice::Replace,
            };
            engine
                .accounts_map(&player, &emulator, &account, choice)
                .await
                .map(|o| {
                    Response::Map(match o {
                        MapOutcome::Mapped => MapResult::Mapped,
                        MapOutcome::NeedsChoice { games } => MapResult::NeedsChoice { games },
                    })
                })
        }
        Request::AccountsCreate {
            player,
            emulator,
            name,
        } => engine
            .create_account(&player, &emulator, name.as_deref())
            .await
            .map(Response::Created),
        Request::AccountsRecover {
            emulator,
            account,
            player,
        } => engine
            .accounts_recover(&emulator, &account, &player)
            .await
            .map(Response::Stream),
        Request::ConfirmFirstSync => engine.confirm_first_sync().map(|()| Response::Done),
        Request::ResolveGame { game, emu } => engine
            .resolve_game(&game, emu.as_deref())
            .await
            .map(|(emulator, game)| Response::Game(GameRef { emulator, game })),
        Request::GameNames => Ok(Response::GameNames(
            engine
                .game_names()
                .into_iter()
                .map(|((emulator, key), name)| GameName {
                    emulator,
                    key,
                    name,
                })
                .collect(),
        )),
        Request::Restore {
            player,
            emulator,
            game,
            what,
        } => engine
            .restore(&player, &emulator, &game, &what)
            .await
            .map(Response::Stream),
        Request::Resolve {
            player,
            emulator,
            game,
            keep,
        } => match keep.parse::<VersionId>() {
            Ok(keep) => engine
                .resolve(&player, &emulator, &game, keep)
                .await
                .map(|(value, warnings)| Response::Resolved(WithWarnings { value, warnings })),
            Err(_) => Err(EngineError::Usage(format!("version_invalid:{keep}"))),
        },
        Request::LeaseRelease { game } => engine
            .lease_release_game(&game)
            .await
            .map(Response::Released),
        Request::AliasAdd { game, emu, alias } => engine
            .alias_add(&game, emu.as_deref(), &alias)
            .await
            .map(|(emulator, game)| Response::Game(GameRef { emulator, game })),
        Request::AliasRemove { alias } => engine.alias_remove(&alias).map(|()| Response::Done),
        Request::Config(op) => config(engine, op),
        Request::Games => engine.games_report().map(Response::Games),
        Request::History { game, emu, player } => {
            history(engine, &game, emu.as_deref(), player).await
        }
        Request::Conflicts => engine
            .conflicts()
            .await
            .map(|(value, warnings)| Response::Conflicts(WithWarnings { value, warnings })),
        Request::LeaseStatus => engine
            .lease_status()
            .await
            .map(|(value, warnings)| Response::LeaseStatus(WithWarnings { value, warnings })),
        Request::Doctor => engine.doctor().await.map(Response::Doctor),
        Request::Status
        | Request::Sync { .. }
        | Request::Pause { .. }
        | Request::Resume
        | Request::Subscribe
        | Request::Shutdown
        | Request::Play { .. }
        | Request::Answer(_) => return None,
    };
    Some(out)
}

/// Play arguments as the engine takes them.
pub(crate) fn os_args(args: &[String]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_only_player_or_which_player() {
        assert_eq!(only_player(vec!["Ana".into()]).unwrap(), "Ana");
        assert!(matches!(
            only_player(vec![]),
            Err(EngineError::Usage(k)) if k == "player_needed"
        ));
    }

    #[test]
    fn setup_changes_refresh_and_reads_do_not() {
        assert!(changes_setup(&Request::GrantAdd { path: "/x".into() }));
        assert!(changes_setup(&Request::Config(ConfigOp::Set {
            key: "k".into(),
            values: vec![]
        })));
        assert!(!changes_setup(&Request::Config(ConfigOp::Get {
            key: "k".into()
        })));
        assert!(!changes_setup(&Request::Games));
    }
}
