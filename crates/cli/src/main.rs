//! `lemusync`: command-line interface. Spec: docs/specs/config-and-cli.md.

mod config_cmd;
mod connection;
mod games_cmd;
mod i18n;
mod output;
mod play;
mod progress;
mod prompt;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{CommandFactory as _, FromArgMatches as _, Parser, Subcommand};
use lemusync_api::StreamAction;
use lemusync_engine::{
    BackendConfig, Engine, EngineEnv, EngineError, JoinSecret, Locations, MapChoice, MapOutcome,
    S3Backend, SyncOptions, VersionId,
};

use crate::i18n::{I18n, s};

#[derive(Parser)]
#[command(name = "lemusync", version)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Init,
    Join {
        #[arg(long)]
        recovery: bool,
    },
    Backend {
        #[command(subcommand)]
        cmd: BackendCmd,
    },
    Grant {
        #[command(subcommand)]
        cmd: GrantCmd,
    },
    Players {
        #[command(subcommand)]
        cmd: PlayersCmd,
    },
    Scan,
    Games {
        #[command(subcommand)]
        cmd: Option<GamesCmd>,
    },
    Accounts {
        #[command(subcommand)]
        cmd: AccountsCmd,
    },
    Sync {
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
    Status,
    History {
        player: String,
        emulator: String,
        game: String,
    },
    Restore {
        player: String,
        emulator: String,
        game: String,
        what: String,
        #[arg(long)]
        yes: bool,
    },
    Doctor,
    Play {
        emulator: String,
        game: String,
        #[arg(long = "as")]
        player: Option<String>,
        #[arg(last = true)]
        args: Vec<std::ffi::OsString>,
    },
    Conflicts,
    Resolve {
        game: String,
        version: String,
        #[arg(long = "as")]
        player: Option<String>,
    },
    Lease {
        #[command(subcommand)]
        cmd: LeaseCmd,
    },
    Config {
        #[command(subcommand)]
        cmd: config_cmd::ConfigCmd,
    },
}

#[derive(Subcommand)]
enum GamesCmd {
    Alias {
        game: String,
        alias: String,
        #[arg(long)]
        emu: Option<String>,
    },
    Unalias {
        alias: String,
    },
}

#[derive(Subcommand)]
enum LeaseCmd {
    Status,
    Release {
        #[arg(long, required = true)]
        force: bool,
        game: String,
    },
}

#[derive(Subcommand)]
enum BackendCmd {
    Add {
        #[command(subcommand)]
        kind: BackendKind,
    },
    Login,
}

#[derive(Subcommand)]
enum BackendKind {
    Gdrive {
        #[arg(long)]
        client_id: String,
    },
    S3 {
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        bucket: String,
        #[arg(long)]
        access_key_id: String,
        #[arg(long, default_value = "auto")]
        region: String,
    },
    Fs {
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum GrantCmd {
    Add { path: PathBuf },
    List,
}

#[derive(Subcommand)]
enum PlayersCmd {
    Add { name: String },
    List,
}

#[derive(Subcommand)]
enum AccountsCmd {
    Map {
        player: String,
        emulator: String,
        account: String,
        #[arg(long, conflicts_with = "replace")]
        keep: bool,
        #[arg(long)]
        replace: bool,
    },
    Recover {
        emulator: String,
        account: String,
        #[arg(long)]
        player: String,
    },
    Create {
        player: String,
        emulator: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        yes: bool,
    },
}

/// Sets every command's and argument's help from `help-<path>[-<arg id>]` (P6).
/// clap's own `Usage:`/`Options:` headings and the built-in -h/-V texts stay English (i18n Open questions).
fn localize(cmd: clap::Command, path: &str, t: &I18n) -> clap::Command {
    let key = if path.is_empty() {
        "help-about".to_owned()
    } else {
        format!("help-{path}")
    };
    let mut cmd = match t.get(&key, &[]) {
        Some(text) => cmd.about(text),
        None => cmd,
    };
    let args: Vec<String> = cmd
        .get_arguments()
        .map(|a| a.get_id().to_string())
        .collect();
    for id in args {
        if let Some(text) = t.get(&format!("{key}-{id}"), &[]) {
            cmd = cmd.mut_arg(id, |a| a.help(text));
        }
    }
    let subs: Vec<String> = cmd
        .get_subcommands()
        .map(|c| c.get_name().to_owned())
        .collect();
    for name in subs {
        let sub_path = if path.is_empty() {
            name.clone()
        } else {
            format!("{path}-{name}")
        };
        cmd = cmd.mut_subcommand(&name, |c| localize(c, &sub_path, t));
    }
    cmd
}

fn env() -> Result<EngineEnv, EngineError> {
    let mut env = EngineEnv::system()?;
    if let Some(root) = std::env::var_os("LEMUSYNC_TEST_ROOT") {
        env.locations = Locations::under(Path::new(&root));
    }
    Ok(env)
}

fn main() -> ExitCode {
    let t = I18n::from_env();
    let matches = localize(Cli::command(), "", &t).get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            output::error(&t, cli.json, &EngineError::Io(e.to_string()));
            return ExitCode::from(1);
        }
    };
    match runtime.block_on(run(&cli, &t)) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            output::error(&t, cli.json, &e);
            ExitCode::from(e.exit_code())
        }
    }
}

async fn gdrive_login(engine: &mut Engine, t: &I18n, client_id: &str) -> Result<(), EngineError> {
    let secret = prompt::secret(t, "prompt-client-secret")?;
    let login = engine.gdrive_login_start(client_id, &secret).await?;
    eprintln!("{}\n\n    {}\n", t.t("login-open-url", &[]), login.url());
    eprintln!("{}", t.t("login-waiting", &[]));
    engine.gdrive_login_finish(login).await?;
    eprintln!("{}", t.t("login-done", &[]));
    Ok(())
}

/// `--as <player>`, or the only player in the repo.
async fn player_of(engine: &mut Engine, given: Option<&String>) -> Result<String, EngineError> {
    if let Some(p) = given {
        return Ok(p.clone());
    }
    let mut players = engine.players_list().await?;
    if players.len() == 1 {
        Ok(players.remove(0))
    } else {
        Err(EngineError::Usage("player_needed".into()))
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// `play`, `conflicts`, `resolve` and `lease` (sync-model §Launcher mode, §Conflicts, §Leases).
#[allow(clippy::too_many_lines)] // one match arm per command, each a few lines
async fn run_play(engine: &mut Engine, cmd: &Cmd, t: &I18n, json: bool) -> Result<u8, EngineError> {
    match cmd {
        Cmd::Play {
            emulator,
            game,
            player,
            args,
        } => {
            let player = player_of(engine, player.as_ref()).await?;
            let p = play::TermPrompt {
                lang: I18n::env_lang(),
                ask: !json && prompt::interactive(),
                events: !json,
                emulator: emulator.clone(),
            };
            if !json {
                eprintln!(
                    "{}",
                    t.t(
                        "play-starting",
                        &[("game", s(games_cmd::label_of(engine, emulator, game)))]
                    )
                );
            }
            let guard = play::hold_ctrl_c(t, !json);
            let report = engine
                .play(
                    &player,
                    emulator,
                    game,
                    args,
                    &p,
                    &lemusync_engine::OsLauncher,
                )
                .await;
            guard.abort();
            let report = report?;
            if json {
                output::json(&report);
            } else {
                let label = games_cmd::label_of(engine, emulator, game);
                output::play(t, emulator, &report, &label, output::color_on());
            }
        }
        Cmd::Conflicts => {
            let (list, warnings) = engine.conflicts().await?;
            if json {
                output::json(&serde_json::json!({
                    "api_version": lemusync_api::API_VERSION,
                    "conflicts": list,
                    "warnings": warnings,
                }));
            } else {
                output::conflicts(t, &list);
                output::warnings(t, &warnings);
            }
        }
        Cmd::Resolve {
            game,
            version,
            player,
        } => {
            let keep: VersionId = version
                .parse()
                .map_err(|_| EngineError::Usage(format!("version_invalid:{version}")))?;
            let game = game.to_lowercase();
            let (list, _) = engine.conflicts().await?;
            let mut found: Vec<_> = list
                .iter()
                .filter(|c| c.game == game && player.as_ref().is_none_or(|p| *p == c.player))
                .collect();
            let c = match found.len() {
                0 => return Err(EngineError::Usage(format!("no_conflict:{game}"))),
                1 => found.remove(0),
                _ => return Err(EngineError::Usage("player_needed".into())),
            };
            let (report, warnings) = engine.resolve(&c.player, &c.emulator, &game, keep).await?;
            if json {
                output::json(&report);
            } else {
                output::line(
                    t,
                    "resolve-done",
                    &[
                        ("player", s(c.player.clone())),
                        ("game", s(game.clone())),
                        ("version", s(version.clone())),
                    ],
                );
                output::warnings(t, &warnings);
            }
        }
        Cmd::Lease {
            cmd: LeaseCmd::Status,
        } => {
            let (list, warnings) = engine.lease_status().await?;
            if json {
                output::json(&serde_json::json!({
                    "api_version": lemusync_api::API_VERSION,
                    "leases": list,
                    "warnings": warnings,
                }));
            } else {
                output::leases(t, &list, now_ms());
                output::warnings(t, &warnings);
            }
        }
        Cmd::Lease {
            cmd: LeaseCmd::Release { game, .. },
        } => {
            let released = engine.lease_release_game(game).await?;
            if json {
                output::json(&serde_json::json!({
                    "api_version": lemusync_api::API_VERSION,
                    "released": released,
                }));
            } else {
                output::line(
                    t,
                    "lease-released",
                    &[
                        ("game", s(game.to_lowercase())),
                        ("count", fluent_bundle::FluentValue::from(released)),
                    ],
                );
            }
        }
        _ => {}
    }
    Ok(0)
}

#[allow(clippy::too_many_lines)] // one match arm per command, each a few lines
async fn run(cli: &Cli, t: &I18n) -> Result<u8, EngineError> {
    let mut engine = Engine::open(env()?)?;
    let json = cli.json;
    let mut live = progress::Live::new(json);
    engine.set_progress(live.sink());
    if !json {
        engine.set_connection_sink(connection::sink(I18n::env_lang(), live.on()));
    }
    match &cli.command {
        cmd @ (Cmd::Play { .. } | Cmd::Conflicts | Cmd::Resolve { .. } | Cmd::Lease { .. }) => {
            return run_play(&mut engine, cmd, t, json).await;
        }
        Cmd::Config { cmd } => config_cmd::run(&mut engine, cmd, t, json)?,
        Cmd::Init => {
            let pass = prompt::new_passphrase(t)?;
            let (report, recovery) = engine.init(&pass).await?;
            eprintln!(
                "{}\n\n    {}\n",
                t.t("init-recovery", &[]),
                recovery.as_str()
            );
            if json {
                output::json(&report);
            } else {
                output::line(
                    t,
                    "init-done",
                    &[
                        ("repo", s(report.repo.clone())),
                        ("device", s(report.device.clone())),
                    ],
                );
                // `SetupIncomplete`: the repo exists but this device must run `join` to finish.
                output::warnings(t, &report.warnings);
            }
        }
        Cmd::Join { recovery } => {
            let secret = if *recovery {
                JoinSecret::Recovery(prompt::secret(t, "prompt-recovery")?.to_string())
            } else {
                JoinSecret::Passphrase(prompt::secret(t, "prompt-passphrase")?.to_string())
            };
            let report = engine.join(secret).await?;
            if json {
                output::json(&report);
            } else {
                output::line(
                    t,
                    "join-done",
                    &[
                        ("device", s(report.device.clone())),
                        ("repo", s(report.repo.clone())),
                    ],
                );
            }
        }
        Cmd::Backend {
            cmd: BackendCmd::Add {
                kind: BackendKind::Fs { path },
            },
        } => {
            engine.backend_add_fs(path)?;
            output::line(
                t,
                "backend-fs-done",
                &[("path", s(path.display().to_string()))],
            );
        }
        Cmd::Backend {
            cmd:
                BackendCmd::Add {
                    kind: BackendKind::Gdrive { client_id },
                },
        } => {
            gdrive_login(&mut engine, t, client_id).await?;
        }
        Cmd::Backend {
            cmd:
                BackendCmd::Add {
                    kind:
                        BackendKind::S3 {
                            endpoint,
                            bucket,
                            access_key_id,
                            region,
                        },
                },
        } => {
            let secret = prompt::secret(t, "prompt-s3-secret")?;
            let backend = S3Backend {
                endpoint: endpoint.clone(),
                bucket: bucket.clone(),
                region: region.clone(),
                access_key_id: access_key_id.clone(),
            };
            engine.backend_add_s3(backend, &secret).await?;
            output::line(
                t,
                "backend-s3-done",
                &[
                    ("bucket", s(bucket.clone())),
                    ("endpoint", s(endpoint.clone())),
                ],
            );
        }
        Cmd::Backend {
            cmd: BackendCmd::Login,
        } => {
            let Some(BackendConfig::Gdrive { client_id, .. }) =
                engine.local_config().backend.clone()
            else {
                return Err(EngineError::NotSetUp("gdrive_backend"));
            };
            gdrive_login(&mut engine, t, &client_id).await?;
        }
        Cmd::Grant {
            cmd: GrantCmd::Add { path },
        } => {
            let granted = engine.grant_add(path)?;
            output::line(
                t,
                "grant-done",
                &[("path", s(granted.display().to_string()))],
            );
        }
        Cmd::Grant {
            cmd: GrantCmd::List,
        } => {
            let grants: Vec<String> = engine
                .grant_list()
                .iter()
                .map(|g| g.display().to_string())
                .collect();
            if json {
                output::json(&grants);
            } else if grants.is_empty() {
                output::line(t, "grant-none", &[]);
            } else {
                for g in grants {
                    output::line(t, "grant-done", &[("path", s(g))]);
                }
            }
        }
        Cmd::Players {
            cmd: PlayersCmd::Add { name },
        } => {
            let warnings = engine.players_add(name).await?;
            output::line(t, "players-added", &[("name", s(name.clone()))]);
            output::warnings(t, &warnings);
        }
        Cmd::Players {
            cmd: PlayersCmd::List,
        } => {
            let players = engine.players_list().await?;
            if json {
                output::json(&players);
            } else if players.is_empty() {
                output::line(t, "players-none", &[]);
            } else {
                for p in players {
                    println!("{p}");
                }
            }
        }
        Cmd::Scan => {
            let report = engine.scan().await?;
            if json {
                output::json(&report);
            } else {
                output::scan(t, &report);
            }
        }
        Cmd::Accounts {
            cmd:
                AccountsCmd::Map {
                    player,
                    emulator,
                    account,
                    keep,
                    replace,
                },
        } => {
            let choice = if *replace {
                MapChoice::Replace
            } else if *keep {
                MapChoice::Keep
            } else {
                MapChoice::Ask
            };
            let mut outcome = engine
                .accounts_map(player, emulator, account, choice)
                .await?;
            if let MapOutcome::NeedsChoice { games } = &outcome {
                let games = games.join(", ");
                if json || !prompt::interactive() {
                    return Err(EngineError::Usage(
                        t.t("error-needs-choice", &[("games", s(games))]),
                    ));
                }
                let question = t.t(
                    "prompt-keep-or-replace",
                    &[
                        ("account", s(account.clone())),
                        ("games", s(games.clone())),
                        ("player", s(player.clone())),
                    ],
                );
                let choice = match prompt::keep_or_replace(t, &question)? {
                    Some(true) => MapChoice::Replace,
                    Some(false) => MapChoice::Keep,
                    None => {
                        return Err(EngineError::Usage(
                            t.t("error-needs-choice", &[("games", s(games))]),
                        ));
                    }
                };
                outcome = engine
                    .accounts_map(player, emulator, account, choice)
                    .await?;
            }
            if matches!(outcome, MapOutcome::Mapped) {
                output::line(
                    t,
                    "map-done",
                    &[
                        ("player", s(player.clone())),
                        ("emulator", s(emulator.clone())),
                        ("account", s(account.clone())),
                    ],
                );
            }
        }
        Cmd::Accounts {
            cmd:
                AccountsCmd::Create {
                    player,
                    emulator,
                    name,
                    yes,
                },
        } => {
            if !*yes {
                let shown = name.clone().unwrap_or_else(|| player.clone());
                let question = t.t(
                    "prompt-create-account",
                    &[
                        ("emulator", s(emulator.clone())),
                        ("name", s(shown)),
                        ("player", s(player.clone())),
                    ],
                );
                if json || !prompt::yes(t, &question)? {
                    return Err(EngineError::Usage(
                        t.t("error-needs-tty", &[("what", s(question))]),
                    ));
                }
            }
            let created = live
                .run(t, engine.create_account(player, emulator, name.as_deref()))
                .await?;
            if json {
                output::json(&created);
            } else {
                output::line(
                    t,
                    "create-done",
                    &[
                        ("emulator", s(emulator.clone())),
                        ("name", s(created.name.clone())),
                        ("account", s(created.account.clone())),
                        ("player", s(player.clone())),
                    ],
                );
            }
        }
        Cmd::Accounts {
            cmd:
                AccountsCmd::Recover {
                    emulator,
                    account,
                    player,
                },
        } => {
            let report = live
                .run(t, engine.accounts_recover(emulator, account, player))
                .await?;
            if json {
                output::json(&report);
            } else {
                output::line(
                    t,
                    "recover-done",
                    &[
                        ("player", s(player.clone())),
                        ("game", s(report.game.clone())),
                    ],
                );
            }
        }
        Cmd::Sync { dry_run, yes } => {
            let opts = || SyncOptions { dry_run: *dry_run };
            let started = std::time::Instant::now();
            let report = match live.run(t, engine.sync(opts())).await {
                Err(EngineError::NeedsConfirmation) => {
                    let ok = *yes
                        || (!json
                            && prompt::interactive()
                            && prompt::yes(t, &t.t("prompt-first-sync", &[]))?);
                    if !ok {
                        return Err(EngineError::NeedsConfirmation);
                    }
                    engine.confirm_first_sync()?;
                    live.run(t, engine.sync(opts())).await?
                }
                other => other?,
            };
            let conflicts = report
                .streams
                .iter()
                .filter(|st| {
                    matches!(
                        st.action,
                        StreamAction::Conflict | StreamAction::UploadThenConflict
                    )
                })
                .count();
            if json {
                output::json(&report);
            } else {
                output::sync(t, &report, output::color_on(), Some(started.elapsed()));
                if conflicts > 0 {
                    output::line(
                        t,
                        "sync-conflicts",
                        &[("count", fluent_bundle::FluentValue::from(conflicts))],
                    );
                }
            }
            if conflicts > 0 {
                return Ok(4);
            }
        }
        Cmd::Games { cmd: None } => {
            let report = engine.games_report()?;
            if json {
                output::json(&report);
            } else {
                games_cmd::print(t, &report);
            }
        }
        Cmd::Games {
            cmd: Some(GamesCmd::Alias { game, alias, emu }),
        } => {
            let (emulator, key) = engine.alias_add(game, emu.as_deref(), alias).await?;
            if json {
                output::json(&serde_json::json!({
                    "api_version": lemusync_api::API_VERSION,
                    "alias": alias,
                    "emulator": emulator,
                    "key": key,
                }));
            } else {
                output::line(
                    t,
                    "games-alias-done",
                    &[
                        ("alias", s(alias.clone())),
                        ("game", s(games_cmd::label_of(&engine, &emulator, &key))),
                    ],
                );
            }
        }
        Cmd::Games {
            cmd: Some(GamesCmd::Unalias { alias }),
        } => {
            engine.alias_remove(alias)?;
            if json {
                output::json(&serde_json::json!({
                    "api_version": lemusync_api::API_VERSION,
                    "removed": alias,
                }));
            } else {
                output::line(t, "games-unalias-done", &[("alias", s(alias.clone()))]);
            }
        }
        Cmd::Status => {
            let report = engine.status().await?;
            if json {
                output::json(&report);
            } else {
                output::status(t, &report);
            }
        }
        Cmd::History {
            player,
            emulator,
            game,
        } => {
            let report = engine.history(player, emulator, game).await?;
            if json {
                output::json(&report);
            } else {
                output::history(t, &report, &game.to_lowercase());
            }
        }
        Cmd::Restore {
            player,
            emulator,
            game,
            what,
            yes,
        } => {
            if !*yes {
                let question = t.t(
                    "prompt-restore",
                    &[("player", s(player.clone())), ("what", s(what.clone()))],
                );
                if json || !prompt::yes(t, &question)? {
                    return Err(EngineError::Usage(
                        t.t("error-needs-tty", &[("what", s(question))]),
                    ));
                }
            }
            let report = live
                .run(t, engine.restore(player, emulator, game, what))
                .await?;
            if json {
                output::json(&report);
            } else {
                output::line(
                    t,
                    "restore-done",
                    &[("what", s(what.clone())), ("player", s(player.clone()))],
                );
            }
        }
        Cmd::Doctor => {
            let report = engine.doctor().await?;
            if json {
                output::json(&report);
            } else {
                output::doctor(t, &report);
            }
            if report.checks.iter().any(|c| !c.ok) {
                return Ok(1);
            }
        }
    }
    Ok(0)
}
