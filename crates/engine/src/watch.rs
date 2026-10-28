//! Save-folder watcher: tells the daemon which stream changed (architecture §Engine internals,
//! reliability §Always on). Missed events are covered by the scheduler's rescans.
//!
//! Each existing save folder is watched recursively without following symlinks, and every event
//! path is checked again after canonicalising: anything that resolves outside the folder is
//! dropped (B2). A folder that doesn't exist yet is watched through its nearest existing parent,
//! non-recursively, and promoted (and reported) once it appears.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::{Duration, Instant};

use lemusync_core::{PlayerId, StreamId};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc::Sender;

use crate::{Engine, EngineError, repo_config};

/// Events for one stream closer together than this are sent once.
pub const BURST: Duration = Duration::from_millis(250);

struct Target {
    stream: StreamId,
    folder: PathBuf,
    /// The canonical folder while it exists and is watched recursively.
    canon: Option<PathBuf>,
    last_sent: Option<Instant>,
}

struct Shared {
    watcher: Option<RecommendedWatcher>,
    targets: Vec<Target>,
    watched: BTreeMap<PathBuf, RecursiveMode>,
    tx: Sender<StreamId>,
}

/// Watches save folders and sends a stream id on any change below its folder.
pub struct SaveWatcher {
    shared: Arc<Mutex<Shared>>,
}

impl SaveWatcher {
    /// # Errors
    /// `Io` when the OS watcher can't start (e.g. the inotify limit is reached).
    pub fn start(
        folders: Vec<(StreamId, PathBuf)>,
        tx: Sender<StreamId>,
    ) -> Result<SaveWatcher, EngineError> {
        let (raw_tx, raw_rx) = mpsc::channel::<notify::Result<Event>>();
        let watcher = RecommendedWatcher::new(
            move |e| {
                let _ = raw_tx.send(e);
            },
            notify::Config::default().with_follow_symlinks(false),
        )
        .map_err(|e| EngineError::Io(format!("watch: {e}")))?;
        let shared = Arc::new(Mutex::new(Shared {
            watcher: Some(watcher),
            targets: Vec::new(),
            watched: BTreeMap::new(),
            tx,
        }));
        lock(&shared).replace(folders);
        // Watch changes happen here, never in the notify callback (some backends deadlock).
        let worker = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("lemusync-watch".into())
            .spawn(move || {
                // Ends when `Drop` drops the watcher, and with it the callback's sender.
                for event in raw_rx.into_iter().flatten() {
                    lock(&worker).on_event(&event);
                }
            })
            .map_err(|e| EngineError::Io(format!("watch thread: {e}")))?;
        Ok(SaveWatcher { shared })
    }

    /// Replaces the watched set (grants or mappings changed).
    pub fn set_folders(&mut self, folders: Vec<(StreamId, PathBuf)>) {
        lock(&self.shared).replace(folders);
    }
}

impl Drop for SaveWatcher {
    fn drop(&mut self) {
        lock(&self.shared).watcher = None;
    }
}

fn lock(m: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The deepest existing directory at or above `p`.
fn nearest_existing(p: &Path) -> Option<&Path> {
    p.ancestors().find(|a| a.is_dir())
}

/// `p` resolved through symlinks; for a path that's gone, its parent's resolution plus its name.
fn resolve(p: &Path) -> Option<PathBuf> {
    p.canonicalize().ok().or_else(|| {
        let parent = p.parent()?.canonicalize().ok()?;
        Some(parent.join(p.file_name()?))
    })
}

impl Shared {
    fn replace(&mut self, folders: Vec<(StreamId, PathBuf)>) {
        self.targets = folders
            .into_iter()
            .map(|(stream, folder)| Target {
                stream,
                folder,
                canon: None,
                last_sent: None,
            })
            .collect();
        let _ = self.reconcile();
    }

    /// Brings the OS watches in line with the targets. Returns the streams whose folder just
    /// became watchable (it appeared). Repeats until stable: a directory created while a watch
    /// was being added sends no event, so the next pass finds it.
    fn reconcile(&mut self) -> Vec<StreamId> {
        let mut appeared = Vec::new();
        for _ in 0..64 {
            let (new, moved) = self.reconcile_once();
            appeared.extend(new);
            if !moved {
                break;
            }
        }
        appeared
    }

    /// One pass; also says whether any watch changed.
    fn reconcile_once(&mut self) -> (Vec<StreamId>, bool) {
        let mut want: BTreeMap<PathBuf, RecursiveMode> = BTreeMap::new();
        let mut appeared = Vec::new();
        for t in &mut self.targets {
            if t.folder.is_dir() {
                if t.canon.is_none() {
                    t.canon = t.folder.canonicalize().ok();
                    appeared.push(t.stream);
                }
                want.insert(t.folder.clone(), RecursiveMode::Recursive);
            } else {
                t.canon = None;
                if let Some(a) = nearest_existing(&t.folder) {
                    want.entry(a.to_path_buf())
                        .or_insert(RecursiveMode::NonRecursive);
                }
            }
        }
        let Some(w) = self.watcher.as_mut() else {
            return (Vec::new(), false);
        };
        let moved = self.watched != want;
        for (p, mode) in &self.watched {
            if want.get(p) != Some(mode) {
                let _ = w.unwatch(p);
            }
        }
        self.watched.retain(|p, mode| want.get(p) == Some(mode));
        for (p, mode) in want {
            if !self.watched.contains_key(&p) && w.watch(&p, mode).is_ok() {
                self.watched.insert(p, mode);
            }
        }
        (appeared, moved)
    }

    fn on_event(&mut self, event: &Event) {
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        let pending = self.targets.iter().any(|t| t.canon.is_none());
        let mut changed: Vec<StreamId> = Vec::new();
        if pending || matches!(event.kind, EventKind::Remove(_) | EventKind::Create(_)) {
            changed.extend(self.reconcile());
        }
        for p in &event.paths {
            let Some(real) = resolve(p) else { continue };
            for t in &self.targets {
                if t.canon.as_ref().is_some_and(|c| real.starts_with(c)) {
                    changed.push(t.stream);
                }
            }
        }
        changed.sort_unstable();
        changed.dedup();
        let now = Instant::now();
        for stream in changed {
            for t in self.targets.iter_mut().filter(|t| t.stream == stream) {
                if t.last_sent.is_some_and(|l| now.duration_since(l) < BURST) {
                    continue;
                }
                // A full channel means the daemon already has work queued: retry on the next event.
                if self.tx.try_send(stream).is_ok() {
                    t.last_sent = Some(now);
                }
                break;
            }
        }
    }
}

/// One stream the daemon watches: its save folder and who it belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchTarget {
    pub stream: StreamId,
    pub player: PlayerId,
    /// Player name as the user sees it.
    pub player_name: String,
    pub emulator: String,
    pub game: String,
    pub folder: PathBuf,
}

impl Engine {
    /// The save folders this device syncs, one per stream, from the same data `sync` uses.
    /// Folders whose account isn't mapped here are left out. Reads the repo config (cached
    /// versions come from the state DB), but no saves.
    ///
    /// # Errors
    /// `NotSetUp` without a repo; storage errors while listing the config.
    pub async fn watch_targets(&mut self) -> Result<Vec<(StreamId, PathBuf)>, EngineError> {
        Ok(self
            .watch_plan()
            .await?
            .into_iter()
            .map(|t| (t.stream, t.folder))
            .collect())
    }

    /// As [`Engine::watch_targets`], with each stream's player, emulator and game.
    ///
    /// # Errors
    /// As `watch_targets`.
    pub async fn watch_plan(&mut self) -> Result<Vec<WatchTarget>, EngineError> {
        self.ensure_repo().await?;
        let device = self.device_id();
        let config = repo_config::load(self.repo_ref()?, &self.state)
            .await?
            .config;
        let mut warnings = Vec::new();
        let mut out = Vec::new();
        for (profile, install, keys) in self.synced_installs(&config, &mut warnings)? {
            for (player, game) in keys {
                let Some(account) = config.account_of(device, player, &profile.id) else {
                    continue;
                };
                let Ok(folder) = crate::discover::save_folder(&profile, &install, &game, account)
                else {
                    continue;
                };
                out.push(WatchTarget {
                    stream: self.repo_ref()?.stream_id(player, &profile.id, &game),
                    player,
                    player_name: config
                        .players
                        .get(&player)
                        .map(|p| p.name.clone())
                        .unwrap_or_default(),
                    emulator: profile.id.clone(),
                    game,
                    folder,
                });
            }
        }
        Ok(out)
    }

    /// Process names per emulator with a granted install here, for `ProcessPoller`. Local
    /// only: works while the storage is unreachable.
    ///
    /// # Errors
    /// `Config` for a broken bundled profile; `Io` while discovering installs.
    pub fn process_names_by_emulator(&self) -> Result<Vec<(String, Vec<String>)>, EngineError> {
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        for (profile, _, discovery) in self.installs()? {
            if discovery.is_none() || out.iter().any(|(id, _)| *id == profile.id) {
                continue;
            }
            let names = crate::sync::process_names(&profile);
            if !names.is_empty() {
                out.push((profile.id.clone(), names));
            }
        }
        Ok(out)
    }

    /// The process probe this engine was opened with (the daemon's poller shares it).
    #[must_use]
    pub fn process_probe(&self) -> std::sync::Arc<dyn crate::ProcessProbe> {
        std::sync::Arc::clone(&self.env.processes)
    }
}
