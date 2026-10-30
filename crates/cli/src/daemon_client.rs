//! Where a command runs (D35): through the daemon when one answers on the socket, else the
//! engine in this process, as in Phase 1. Both sides of [`Api`] make the same engine calls
//! and return the same types, so every command prints the same text and the same JSON
//! either way. Secrets go over the same-user socket as `Secret`s (wiped on drop, never in
//! `Debug`, frame buffers wiped too: security §Local IPC).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lemusync_api::{
    ConfigOp, ConflictInfo, CreatedAccount, DoctorReport, GamesReport, HistoryReport, InitReport,
    JoinWire, LeaseAnswer, LeaseInfo, MapChoiceWire, MapResult, PlayAnswer, PlayQuestion,
    PlayReport, Request, Response, ScanReport, Secret, StatusReport, StreamReport, SyncReport,
    Warning,
};
use lemusync_engine::{
    ConnectionSink, Engine, EngineError, JoinSecret, LeaseChoice, MapChoice, MapOutcome,
    PlayPrompt, ProgressSink, S3Backend, SyncOptions, VersionId,
};
use lemusync_ipc::{Client, IpcError};
use zeroize::Zeroizing;

/// The client name in the handshake.
const CLIENT: &str = "lemusync-cli";

/// The engine, here or in the daemon.
pub enum Api {
    Local(Box<Engine>),
    Daemon(Box<Daemon>),
}

/// A connection to the running daemon.
pub struct Daemon {
    client: Client,
}

/// The daemon's connection, or `None` when no daemon answers (nothing listens, no runtime
/// dir, or this OS has no IPC yet): the caller then runs the engine itself.
///
/// # Errors
/// The daemon refused the handshake (another `api_version`): never silently in-process,
/// since that daemon still holds the engine lock.
pub async fn connect() -> Result<Option<Client>, EngineError> {
    let Ok(path) = lemusync_ipc::socket::default_path() else {
        return Ok(None);
    };
    match Client::connect(&path, CLIENT).await {
        Ok(c) => Ok(Some(c)),
        Err(IpcError::Rejected(body)) => Err(EngineError::from_body(&body)),
        Err(_) => Ok(None),
    }
}

fn unexpected(r: &Response) -> EngineError {
    match r {
        Response::Error(body) => EngineError::from_body(body),
        other => EngineError::Daemon(format!("unexpected answer {other:?}")),
    }
}

fn ipc(e: &IpcError) -> EngineError {
    EngineError::Daemon(e.to_string())
}

/// `daemon.call(request)`, matched against one response shape.
macro_rules! call {
    ($d:expr, $req:expr, $pat:pat => $out:expr) => {
        match $d.call($req).await? {
            $pat => Ok($out),
            other => Err(unexpected(&other)),
        }
    };
}

impl Daemon {
    async fn call(&mut self, r: Request) -> Result<Response, EngineError> {
        self.client.call(r).await.map_err(|e| ipc(&e))
    }

    /// The daemon's version.
    pub fn version(&self) -> &str {
        &self.client.daemon().daemon_version
    }

    /// Pauses until `until_ms` (`None`: until resume).
    ///
    /// # Errors
    /// The daemon's error, or the connection's.
    pub async fn pause(&mut self, until_ms: Option<u64>) -> Result<(), EngineError> {
        call!(self, Request::Pause { until_ms }, Response::Paused => ())
    }

    /// Ends a pause.
    ///
    /// # Errors
    /// The daemon's error, or the connection's.
    pub async fn resume(&mut self) -> Result<(), EngineError> {
        call!(self, Request::Resume, Response::Resumed => ())
    }

    /// `play` through the daemon: steps and questions come back as frames; `prompt`
    /// answers. A question the prompt refuses is answered `Refuse` (exit 5 or 4).
    async fn play(
        &mut self,
        request: Request,
        prompt: &dyn PlayPrompt,
    ) -> Result<PlayReport, EngineError> {
        self.client.send(&request).await.map_err(|e| ipc(&e))?;
        loop {
            match self.client.recv().await.map_err(|e| ipc(&e))? {
                Response::PlayEvent(e) => prompt.event(e),
                Response::Prompt(q) => {
                    let a = answer(prompt, &q);
                    self.client
                        .send(&Request::Answer(a))
                        .await
                        .map_err(|e| ipc(&e))?;
                }
                Response::Play(report) => return Ok(report),
                other => return Err(unexpected(&other)),
            }
        }
    }

    /// Feeds the daemon's transfer progress and connection events to the CLI's live line
    /// and connection notice, on a second connection, until the process ends.
    pub async fn watch_progress(&self, progress: ProgressSink, conn: Option<ConnectionSink>) {
        use futures_util::StreamExt as _;
        let Ok(Some(client)) = connect().await else {
            return;
        };
        let Ok(stream) = client.subscribe().await else {
            return;
        };
        tokio::spawn(async move {
            tokio::pin!(stream);
            while let Some(Ok(e)) = stream.next().await {
                match e {
                    lemusync_api::Event::TransferProgress(p) => progress(&p),
                    lemusync_api::Event::Connection(c) => {
                        if let Some(sink) = &conn {
                            sink(&c);
                        }
                    }
                    _ => {}
                }
            }
        });
    }
}

/// The player's answer to a daemon question, through the same prompt as in-process.
fn answer(prompt: &dyn PlayPrompt, q: &PlayQuestion) -> PlayAnswer {
    match q {
        PlayQuestion::Lease { by, ago_s, stale } => match prompt.lease_held(by, *ago_s, *stale) {
            Ok(choice) => PlayAnswer::Lease {
                choice: match choice {
                    LeaseChoice::Wait => LeaseAnswer::Wait,
                    LeaseChoice::PlayAnyway => LeaseAnswer::PlayAnyway,
                    LeaseChoice::TakeOver => LeaseAnswer::TakeOver,
                },
            },
            Err(_) => PlayAnswer::Refuse,
        },
        PlayQuestion::Conflict { conflict } => match prompt.conflict(conflict) {
            Ok(v) => PlayAnswer::Keep {
                version: v.map(|v| v.to_string()),
            },
            Err(_) => PlayAnswer::Refuse,
        },
    }
}

fn secret(s: &str) -> Secret {
    Secret::new(Zeroizing::new(s.to_owned()))
}

/// One method per engine call the CLI makes, same name and result on both sides.
impl Api {
    /// Connects to the daemon, or opens the engine here.
    ///
    /// # Errors
    /// As [`connect`] and `Engine::open`.
    pub async fn open(
        env: impl FnOnce() -> Result<lemusync_engine::EngineEnv, EngineError>,
    ) -> Result<Api, EngineError> {
        Ok(match connect().await? {
            Some(client) => Api::Daemon(Box::new(Daemon { client })),
            None => Api::Local(Box::new(Engine::open(env()?)?)),
        })
    }

    pub fn daemon(&mut self) -> Option<&mut Daemon> {
        match self {
            Api::Daemon(d) => Some(d),
            Api::Local(_) => None,
        }
    }

    pub async fn init(
        &mut self,
        pass: &str,
    ) -> Result<(InitReport, Zeroizing<String>), EngineError> {
        match self {
            Api::Local(e) => e.init(pass).await,
            Api::Daemon(d) => call!(d, Request::Init { passphrase: secret(pass) },
                Response::Init { report, recovery } => (report, recovery.into_inner())),
        }
    }

    pub async fn join(&mut self, s: JoinSecret) -> Result<InitReport, EngineError> {
        match self {
            Api::Local(e) => e.join(s).await,
            Api::Daemon(d) => {
                let wire = match &s {
                    JoinSecret::Passphrase(p) => JoinWire::Passphrase(secret(p)),
                    JoinSecret::Recovery(r) => JoinWire::Recovery(secret(r)),
                };
                call!(d, Request::Join { secret: wire }, Response::Join(r) => r)
            }
        }
    }

    pub async fn backend_add_fs(&mut self, path: &Path) -> Result<(), EngineError> {
        match self {
            Api::Local(e) => e.backend_add_fs(path),
            Api::Daemon(d) => {
                call!(d, Request::BackendAddFs { path: path_str(&absolute(path)?)? }, Response::Done => ())
            }
        }
    }

    pub async fn backend_add_s3(&mut self, b: S3Backend, key: &str) -> Result<(), EngineError> {
        match self {
            Api::Local(e) => e.backend_add_s3(b, key).await,
            Api::Daemon(d) => call!(d, Request::BackendAddS3 {
                endpoint: b.endpoint,
                bucket: b.bucket,
                region: b.region,
                access_key_id: b.access_key_id,
                secret: secret(key),
            }, Response::Done => ()),
        }
    }

    pub async fn grant_add(&mut self, path: &Path) -> Result<PathBuf, EngineError> {
        match self {
            // Relative paths are the CLI's working dir, not the daemon's.
            Api::Local(e) => e.grant_add(path),
            Api::Daemon(d) => call!(d, Request::GrantAdd { path: path_str(&absolute(path)?)? },
                Response::Path(p) => PathBuf::from(p)),
        }
    }

    pub async fn grant_list(&mut self) -> Result<Vec<String>, EngineError> {
        match self {
            Api::Local(e) => Ok(e
                .grant_list()
                .iter()
                .map(|g| g.display().to_string())
                .collect()),
            Api::Daemon(d) => call!(d, Request::GrantList, Response::Names(n) => n),
        }
    }

    pub async fn players_add(&mut self, name: &str) -> Result<Vec<Warning>, EngineError> {
        match self {
            Api::Local(e) => e.players_add(name).await,
            Api::Daemon(d) => {
                call!(d, Request::PlayersAdd { name: name.into() }, Response::Warnings(w) => w)
            }
        }
    }

    pub async fn players_list(&mut self) -> Result<Vec<String>, EngineError> {
        match self {
            Api::Local(e) => e.players_list().await,
            Api::Daemon(d) => call!(d, Request::PlayersList, Response::Names(n) => n),
        }
    }

    pub async fn scan(&mut self) -> Result<ScanReport, EngineError> {
        match self {
            Api::Local(e) => e.scan().await,
            Api::Daemon(d) => call!(d, Request::Scan, Response::Scan(r) => r),
        }
    }

    pub async fn accounts_map(
        &mut self,
        player: &str,
        emulator: &str,
        account: &str,
        choice: MapChoice,
    ) -> Result<MapOutcome, EngineError> {
        match self {
            Api::Local(e) => e.accounts_map(player, emulator, account, choice).await,
            Api::Daemon(d) => {
                let choice = match choice {
                    MapChoice::Ask => MapChoiceWire::Ask,
                    MapChoice::Keep => MapChoiceWire::Keep,
                    MapChoice::Replace => MapChoiceWire::Replace,
                };
                let r = Request::AccountsMap {
                    player: player.into(),
                    emulator: emulator.into(),
                    account: account.into(),
                    choice,
                };
                call!(d, r, Response::Map(m) => match m {
                    MapResult::Mapped => MapOutcome::Mapped,
                    MapResult::NeedsChoice { games } => MapOutcome::NeedsChoice { games },
                })
            }
        }
    }

    pub async fn create_account(
        &mut self,
        player: &str,
        emulator: &str,
        name: Option<&str>,
    ) -> Result<CreatedAccount, EngineError> {
        match self {
            Api::Local(e) => e.create_account(player, emulator, name).await,
            Api::Daemon(d) => call!(d, Request::AccountsCreate {
                player: player.into(),
                emulator: emulator.into(),
                name: name.map(Into::into),
            }, Response::Created(c) => c),
        }
    }

    pub async fn accounts_recover(
        &mut self,
        emulator: &str,
        account: &str,
        player: &str,
    ) -> Result<StreamReport, EngineError> {
        match self {
            Api::Local(e) => e.accounts_recover(emulator, account, player).await,
            Api::Daemon(d) => call!(d, Request::AccountsRecover {
                emulator: emulator.into(),
                account: account.into(),
                player: player.into(),
            }, Response::Stream(r) => r),
        }
    }

    pub async fn sync(&mut self, opts: SyncOptions) -> Result<SyncReport, EngineError> {
        match self {
            Api::Local(e) => e.sync(opts).await,
            Api::Daemon(d) => {
                call!(d, Request::Sync { dry_run: opts.dry_run }, Response::Sync(r) => r)
            }
        }
    }

    pub async fn confirm_first_sync(&mut self) -> Result<(), EngineError> {
        match self {
            Api::Local(e) => e.confirm_first_sync(),
            Api::Daemon(d) => call!(d, Request::ConfirmFirstSync, Response::Done => ()),
        }
    }

    pub async fn games_report(&mut self) -> Result<GamesReport, EngineError> {
        match self {
            Api::Local(e) => e.games_report(),
            Api::Daemon(d) => call!(d, Request::Games, Response::Games(r) => r),
        }
    }

    pub async fn game_names(&mut self) -> BTreeMap<(String, String), String> {
        match self {
            Api::Local(e) => e.game_names(),
            Api::Daemon(d) => match d.call(Request::GameNames).await {
                Ok(Response::GameNames(n)) => n
                    .into_iter()
                    .map(|g| ((g.emulator, g.key), g.name))
                    .collect(),
                _ => BTreeMap::new(),
            },
        }
    }

    pub async fn alias_add(
        &mut self,
        game: &str,
        emu: Option<&str>,
        alias: &str,
    ) -> Result<(String, String), EngineError> {
        match self {
            Api::Local(e) => e.alias_add(game, emu, alias).await,
            Api::Daemon(d) => call!(d, Request::AliasAdd {
                game: game.into(),
                emu: emu.map(Into::into),
                alias: alias.into(),
            }, Response::Game(g) => (g.emulator, g.game)),
        }
    }

    pub async fn alias_remove(&mut self, alias: &str) -> Result<(), EngineError> {
        match self {
            Api::Local(e) => e.alias_remove(alias),
            Api::Daemon(d) => {
                call!(d, Request::AliasRemove { alias: alias.into() }, Response::Done => ())
            }
        }
    }

    pub async fn status(&mut self) -> Result<StatusReport, EngineError> {
        match self {
            Api::Local(e) => e.status().await,
            Api::Daemon(d) => call!(d, Request::Status, Response::Status(r) => r),
        }
    }

    pub async fn resolve_game(
        &mut self,
        game: &str,
        emu: Option<&str>,
    ) -> Result<(String, String), EngineError> {
        match self {
            Api::Local(e) => e.resolve_game(game, emu).await,
            Api::Daemon(d) => {
                call!(d, Request::ResolveGame { game: game.into(), emu: emu.map(Into::into) },
                Response::Game(g) => (g.emulator, g.game))
            }
        }
    }

    pub async fn history(
        &mut self,
        player: &str,
        emulator: &str,
        game: &str,
    ) -> Result<HistoryReport, EngineError> {
        match self {
            Api::Local(e) => e.history(player, emulator, game).await,
            Api::Daemon(d) => call!(d, Request::History {
                game: game.into(),
                emu: Some(emulator.into()),
                player: Some(player.into()),
            }, Response::History(r) => r),
        }
    }

    pub async fn restore(
        &mut self,
        player: &str,
        emulator: &str,
        game: &str,
        what: &str,
    ) -> Result<StreamReport, EngineError> {
        match self {
            Api::Local(e) => e.restore(player, emulator, game, what).await,
            Api::Daemon(d) => call!(d, Request::Restore {
                player: player.into(),
                emulator: emulator.into(),
                game: game.into(),
                what: what.into(),
            }, Response::Stream(r) => r),
        }
    }

    pub async fn doctor(&mut self) -> Result<DoctorReport, EngineError> {
        match self {
            Api::Local(e) => e.doctor().await,
            Api::Daemon(d) => call!(d, Request::Doctor, Response::Doctor(r) => r),
        }
    }

    pub async fn conflicts(&mut self) -> Result<(Vec<ConflictInfo>, Vec<Warning>), EngineError> {
        match self {
            Api::Local(e) => e.conflicts().await,
            Api::Daemon(d) => {
                call!(d, Request::Conflicts, Response::Conflicts(c) => (c.value, c.warnings))
            }
        }
    }

    pub async fn resolve(
        &mut self,
        player: &str,
        emulator: &str,
        game: &str,
        keep: VersionId,
    ) -> Result<(StreamReport, Vec<Warning>), EngineError> {
        match self {
            Api::Local(e) => e.resolve(player, emulator, game, keep).await,
            Api::Daemon(d) => call!(d, Request::Resolve {
                player: player.into(),
                emulator: emulator.into(),
                game: game.into(),
                keep: keep.to_string(),
            }, Response::Resolved(r) => (r.value, r.warnings)),
        }
    }

    pub async fn lease_status(&mut self) -> Result<(Vec<LeaseInfo>, Vec<Warning>), EngineError> {
        match self {
            Api::Local(e) => e.lease_status().await,
            Api::Daemon(d) => {
                call!(d, Request::LeaseStatus, Response::LeaseStatus(l) => (l.value, l.warnings))
            }
        }
    }

    pub async fn lease_release_game(&mut self, game: &str) -> Result<u32, EngineError> {
        match self {
            Api::Local(e) => e.lease_release_game(game).await,
            Api::Daemon(d) => {
                call!(d, Request::LeaseRelease { game: game.into() }, Response::Released(n) => n)
            }
        }
    }

    pub async fn config_path(&mut self) -> Result<PathBuf, EngineError> {
        match self {
            Api::Local(e) => Ok(e.config_path()),
            Api::Daemon(d) => {
                call!(d, Request::Config(ConfigOp::Path), Response::Path(p) => PathBuf::from(p))
            }
        }
    }

    pub async fn config_values(
        &mut self,
    ) -> Result<BTreeMap<String, serde_json::Value>, EngineError> {
        match self {
            Api::Local(e) => e.config_values(),
            Api::Daemon(d) => {
                call!(d, Request::Config(ConfigOp::List), Response::ConfigValues(v) => v)
            }
        }
    }

    pub async fn config_get(&mut self, key: &str) -> Result<serde_json::Value, EngineError> {
        match self {
            Api::Local(e) => e.config_get(key),
            Api::Daemon(d) => {
                call!(d, Request::Config(ConfigOp::Get { key: key.into() }), Response::ConfigValue(v) => v)
            }
        }
    }

    pub async fn config_set(&mut self, key: &str, values: &[String]) -> Result<(), EngineError> {
        match self {
            Api::Local(e) => e.config_set(key, values),
            Api::Daemon(d) => {
                call!(d, Request::Config(ConfigOp::Set { key: key.into(), values: values.to_vec() }), Response::Done => ())
            }
        }
    }

    pub async fn config_unset(&mut self, key: &str) -> Result<(), EngineError> {
        match self {
            Api::Local(e) => e.config_unset(key),
            Api::Daemon(d) => {
                call!(d, Request::Config(ConfigOp::Unset { key: key.into() }), Response::Done => ())
            }
        }
    }

    pub async fn play(
        &mut self,
        player: &str,
        emulator: &str,
        game: &str,
        args: &[std::ffi::OsString],
        prompt: &dyn PlayPrompt,
    ) -> Result<PlayReport, EngineError> {
        match self {
            Api::Local(e) => {
                e.play(
                    player,
                    emulator,
                    game,
                    args,
                    prompt,
                    &lemusync_engine::OsLauncher,
                )
                .await
            }
            Api::Daemon(d) => {
                let args = args
                    .iter()
                    .map(|a| {
                        a.to_str().map(str::to_owned).ok_or_else(|| {
                            EngineError::Usage(format!("play_arg_not_utf8:{}", a.to_string_lossy()))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let r = Request::Play {
                    player: player.into(),
                    emulator: emulator.into(),
                    game: game.into(),
                    args,
                };
                d.play(r, prompt).await
            }
        }
    }
}

fn path_str(p: &Path) -> Result<String, EngineError> {
    p.to_str()
        .map(str::to_owned)
        .ok_or_else(|| EngineError::Usage(format!("path_not_utf8:{}", p.display())))
}

fn absolute(p: &Path) -> Result<PathBuf, EngineError> {
    Ok(std::path::absolute(p)?)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::play::TermPrompt;
    use lemusync_api::ErrorBody;
    use lemusync_ipc::Server;

    /// A daemon that holds the lease: it asks, and turns a refusal into `lease_held`.
    async fn fake_daemon(server: Server) -> Option<PlayAnswer> {
        let mut conn = server.accept().await.ok()?.handshake().await.ok()??;
        let Some(Request::Play { .. }) = conn.recv().await else {
            return None;
        };
        let q = PlayQuestion::Lease {
            by: "deck".into(),
            ago_s: 5,
            stale: false,
        };
        conn.send(&Response::Prompt(q)).await.ok()?;
        let Some(Request::Answer(a)) = conn.recv().await else {
            return None;
        };
        let body = ErrorBody {
            code: "lease_held".into(),
            message: "held".into(),
            detail: Some("deck".into()),
        };
        conn.send(&Response::Error(body)).await.ok()?;
        Some(a)
    }

    #[tokio::test]
    async fn play_with_a_held_lease_and_no_terminal_exits_5() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run").join("daemon.sock");
        let server = Server::bind(&path).unwrap();
        let daemon = tokio::spawn(fake_daemon(server));
        let client = Client::connect(&path, CLIENT).await.unwrap();
        let mut d = Daemon { client };
        let prompt = TermPrompt {
            lang: "en",
            ask: false,
            events: false,
            emulator: "cemu".into(),
        };
        let r = Request::Play {
            player: "P".into(),
            emulator: "cemu".into(),
            game: "g".into(),
            args: vec![],
        };
        let err = d.play(r, &prompt).await.unwrap_err();
        assert_eq!(err.exit_code(), 5, "{err:?}");
        assert_eq!(daemon.await.unwrap(), Some(PlayAnswer::Refuse));
    }
}
