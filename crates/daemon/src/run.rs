//! The daemon's run loop around the engine service: the IPC server, the tray, notifications
//! and sleep hooks (architecture §Process model, reliability §Sleep and resume).
//!
//! The engine runs on its own thread (`Service::spawn`); everything here talks to it through
//! a [`ServiceHandle`]. This loop maps the status model onto the tray view and events onto
//! coalesced notices, turns tray clicks into requests, and acknowledges a suspend only after
//! the service checkpointed (within 4 s).

use std::future::Future;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lemusync_api::{ErrorKind as ApiErrorKind, Event, Request, Session, Status, StatusState};
use lemusync_engine::ServiceHandle;
use lemusync_ipc::{Incoming, Server};
use tokio::sync::{broadcast, mpsc};

use crate::i18n::I18n;
use crate::notices::{Coalescer, NoticeEvent, notice};
use crate::platform::{
    IdleGuard, NoticeId, Notifier, Power, SleepEdge, Tray, TrayAction, start_notifier, start_power,
    start_tray,
};
use crate::view::{ErrorKind, SessionInput, StatusInput, ViewState, tray_view};

/// The whole suspend checkpoint, ack included (reliability §Sleep and resume: logind's
/// `InhibitDelayMaxUSec` is 5 s by default).
pub const SUSPEND_BUDGET: Duration = Duration::from_secs(4);

/// Desktop presence. `None` in `--headless` mode.
pub struct Desktop {
    pub tray: Box<dyn Tray>,
    pub tray_actions: mpsc::Receiver<TrayAction>,
    pub notifier: Box<dyn Notifier>,
    pub notice_actions: mpsc::Receiver<(NoticeId, String)>,
    pub power: Box<dyn Power>,
}

impl Desktop {
    /// The platform's tray, notifier and power hooks. Must run inside a tokio runtime.
    #[must_use]
    pub fn start() -> Self {
        let (tray_tx, tray_actions) = mpsc::channel(16);
        let (notice_tx, notice_actions) = mpsc::channel(16);
        Self {
            tray: start_tray(tray_tx),
            tray_actions,
            notifier: start_notifier(notice_tx),
            notice_actions,
            power: start_power(),
        }
    }
}

/// `lemusync_api::Status` → the tray's local input (`view` doesn't know the wire types).
#[must_use]
pub fn status_input(s: &Status) -> StatusInput {
    let state = match s.state {
        StatusState::Synced => ViewState::Synced,
        StatusState::Scanning => ViewState::Scanning,
        StatusState::Syncing => ViewState::Syncing,
        StatusState::Paused => ViewState::Paused,
        StatusState::Offline => ViewState::Offline,
        StatusState::Attention => ViewState::Attention,
        StatusState::Error => ViewState::Error(match s.error {
            Some(ApiErrorKind::Auth) => ErrorKind::Auth,
            Some(ApiErrorKind::Quota) => ErrorKind::Quota,
            Some(ApiErrorKind::Permission) => ErrorKind::Permission,
            Some(ApiErrorKind::Config) | None => ErrorKind::Config,
        }),
    };
    let mut input = StatusInput::new(state);
    input.uploads = s.uploads;
    input.downloads = s.downloads;
    input.queued = s.queued;
    input.conflicts = s.conflicts;
    input.foreign_leases = s.foreign_leases;
    input.paused = state == ViewState::Paused || s.paused_until_ms.is_some();
    input
}

/// `lemusync_api::Session` → the tray's session overlay.
#[must_use]
pub fn session_input(s: &Session) -> SessionInput {
    SessionInput {
        game: s.name.clone().unwrap_or_else(|| s.game.clone()),
        player: s.player.clone(),
    }
}

/// What an event asks the player, with its coalescing key and kind.
#[must_use]
pub fn notice_for(e: &Event, last: Option<&Status>) -> Option<(String, &'static str, NoticeEvent)> {
    let stream = |p: &str, e: &str, g: &str| format!("{p}/{e}/{g}");
    match e {
        Event::ConflictDetected(c) => Some((
            stream(&c.player, &c.emulator, &c.game),
            "conflict",
            NoticeEvent::Conflict {
                game: c.name.clone().unwrap_or_else(|| c.game.clone()),
            },
        )),
        Event::LeaseWarning(l) if l.live && !l.ours => Some((
            stream(&l.player, &l.emulator, &l.game),
            "lease",
            NoticeEvent::ForeignLease {
                game: l.game.clone(),
                device: l.device.clone(),
            },
        )),
        Event::RemoteNewer(n) => Some((
            stream(&n.player, &n.emulator, &n.game),
            "remote_newer",
            NoticeEvent::RemoteNewer {
                game: n.name.clone().unwrap_or_else(|| n.game.clone()),
                device: n.device.clone(),
                emulator: n.emulator.clone(),
            },
        )),
        Event::StatusChanged(s) if s.error != last.and_then(|l| l.error) => match s.error {
            Some(ApiErrorKind::Auth) => Some((String::new(), "auth", NoticeEvent::AuthError)),
            Some(ApiErrorKind::Quota) => Some((String::new(), "quota", NoticeEvent::QuotaError)),
            _ => None,
        },
        _ => None,
    }
}

/// The request a tray click means; `None` for `Quit` and the items with no target yet.
#[must_use]
pub fn tray_request(a: &TrayAction, now_ms: u64) -> Option<Request> {
    match a {
        TrayAction::SyncNow => Some(Request::Sync { dry_run: false }),
        TrayAction::Pause1h => Some(Request::Pause {
            until_ms: Some(now_ms.saturating_add(60 * 60_000)),
        }),
        TrayAction::PauseUntilResume => Some(Request::Pause { until_ms: None }),
        TrayAction::Resume => Some(Request::Resume),
        TrayAction::Play(_) | TrayAction::Open | TrayAction::Quit => None,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Serves IPC clients until the task is aborted. `quit` hears a `Shutdown` request.
async fn serve(server: Server, handle: ServiceHandle, quit: mpsc::Sender<()>) {
    loop {
        match server.accept().await {
            Ok(incoming) => {
                tokio::spawn(connection(incoming, handle.clone(), quit.clone()));
            }
            Err(e) => {
                tracing::warn!("ipc accept failed: {e}");
                // A broken listener must not spin.
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

async fn connection(incoming: Incoming, handle: ServiceHandle, quit: mpsc::Sender<()>) {
    let mut conn = match incoming.handshake().await {
        Ok(Some(c)) => c,
        Ok(None) => return,
        Err(e) => {
            tracing::info!("ipc handshake failed: {e}");
            return;
        }
    };
    tracing::debug!("ipc client {}", conn.client());
    while let Some(request) = conn.recv().await {
        match request {
            Request::Subscribe => {
                let mut rx = handle.subscribe();
                if conn
                    .send(&lemusync_api::Response::Subscribed)
                    .await
                    .is_err()
                    || conn
                        .event(&Event::StatusChanged(handle.status()))
                        .await
                        .is_err()
                {
                    return;
                }
                loop {
                    match rx.recv().await {
                        Ok(e) => {
                            if conn.event(&e).await.is_err() {
                                return;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::info!("ipc subscriber lagged by {n} events");
                        }
                        Err(broadcast::error::RecvError::Closed) => return,
                    }
                }
            }
            Request::Shutdown => {
                let r = handle.call(Request::Shutdown).await;
                let _ = conn.send(&r).await;
                let _ = quit.send(()).await;
                return;
            }
            other => {
                let r = handle.call(other).await;
                if conn.send(&r).await.is_err() {
                    return;
                }
            }
        }
    }
}

/// Desktop state the loop keeps between events.
struct Presence {
    tray: Box<dyn Tray>,
    notifier: Box<dyn Notifier>,
    power: Box<dyn Power>,
    i18n: I18n,
    status: Status,
    session: Option<Session>,
    coalesce: Coalescer,
    idle: Option<IdleGuard>,
}

impl Presence {
    fn show(&self) {
        let session = self.session.as_ref().map(session_input);
        let view = tray_view(&status_input(&self.status), session.as_ref(), &self.i18n);
        self.tray.show(&view);
    }

    fn on_event(&mut self, e: &Event) {
        if let Some((stream, kind, ev)) = notice_for(e, Some(&self.status))
            && self.coalesce.allow(&stream, kind, now_ms())
        {
            self.notifier.notify(notice(&ev, &self.i18n));
        }
        match e {
            Event::StatusChanged(s) => {
                self.status = s.clone();
                // Transfers keep the machine from idle-sleeping (never a user suspend).
                if s.state == StatusState::Syncing {
                    if self.idle.is_none() {
                        self.idle = Some(self.power.block_idle("Syncing saves"));
                    }
                } else {
                    self.idle = None;
                }
                self.show();
            }
            Event::SessionChanged(s) => {
                self.session.clone_from(s);
                self.show();
            }
            _ => {}
        }
    }
}

/// Runs the daemon until `shutdown` resolves, a client sends `Shutdown` or the tray's Quit
/// is clicked; then stops the service and drops the server (which unlinks the socket).
pub async fn run(
    handle: ServiceHandle,
    server: Server,
    desktop: Option<Desktop>,
    i18n: I18n,
    shutdown: impl Future<Output = ()>,
) {
    let (quit_tx, mut quit_rx) = mpsc::channel(1);
    let ipc = tokio::spawn(serve(server, handle.clone(), quit_tx));
    let mut events = handle.subscribe();
    let (mut tray_rx, mut notice_rx) = (None, None);
    let mut presence = desktop.map(|d| {
        tray_rx = Some(d.tray_actions);
        notice_rx = Some(d.notice_actions);
        Presence {
            tray: d.tray,
            notifier: d.notifier,
            power: d.power,
            i18n,
            status: handle.status(),
            session: None,
            coalesce: Coalescer::default(),
            idle: None,
        }
    });
    let mut sleep_rx = presence.as_ref().map(|p| p.power.sleep_events());
    if let Some(p) = &presence {
        p.show();
    }
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => break,
            _ = quit_rx.recv() => break,
            e = events.recv() => match e {
                Ok(e) => {
                    if let Some(p) = &mut presence {
                        p.on_event(&e);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
            Some(a) = recv_opt(tray_rx.as_mut()) => {
                if a == TrayAction::Quit {
                    break;
                }
                if let Some(r) = tray_request(&a, now_ms()) {
                    let h = handle.clone();
                    tokio::spawn(async move { h.call(r).await });
                } else {
                    tracing::info!("tray action {a:?} has no target yet");
                }
            }
            Some((id, action)) = recv_opt(notice_rx.as_mut()) => {
                tracing::info!("notification {} action {action}", id.0);
            }
            Some(edge) = recv_opt(sleep_rx.as_mut()) => match edge {
                SleepEdge::Suspending => {
                    if tokio::time::timeout(SUSPEND_BUDGET, handle.suspend()).await.is_err() {
                        tracing::warn!("suspend checkpoint ran over {SUSPEND_BUDGET:?}");
                    }
                    if let Some(p) = &presence {
                        p.power.ack_suspend();
                    }
                }
                SleepEdge::Resumed => handle.resumed().await,
            },
        }
    }
    tracing::info!("stopping");
    handle.stop().await;
    ipc.abort();
    let _ = ipc.await; // the server (and its socket file) is dropped by now
}

/// `rx.recv()`, or never when there is no receiver (headless).
async fn recv_opt<T>(rx: Option<&mut mpsc::Receiver<T>>) -> Option<T> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lemusync_api::{ConflictInfo, LeaseInfo, StatusFacts};

    #[test]
    fn every_status_state_maps_onto_the_view() {
        let table = [
            (StatusFacts::default(), ViewState::Synced),
            (
                StatusFacts {
                    scanning: true,
                    ..Default::default()
                },
                ViewState::Scanning,
            ),
            (
                StatusFacts {
                    syncing: true,
                    uploads: 2,
                    ..Default::default()
                },
                ViewState::Syncing,
            ),
            (
                StatusFacts {
                    paused: true,
                    ..Default::default()
                },
                ViewState::Paused,
            ),
            (
                StatusFacts {
                    offline: true,
                    queued: 3,
                    ..Default::default()
                },
                ViewState::Offline,
            ),
            (
                StatusFacts {
                    conflicts: 1,
                    ..Default::default()
                },
                ViewState::Attention,
            ),
            (
                StatusFacts {
                    error: Some(ApiErrorKind::Quota),
                    ..Default::default()
                },
                ViewState::Error(ErrorKind::Quota),
            ),
        ];
        for (facts, want) in table {
            let input = status_input(&Status::from_facts(&facts));
            assert_eq!(input.state, want);
            assert_eq!(input.uploads, facts.uploads);
            assert_eq!(input.queued, facts.queued);
        }
        let t = I18n::new("en");
        let s = Status::from_facts(&StatusFacts {
            syncing: true,
            uploads: 3,
            ..Default::default()
        });
        assert_eq!(
            tray_view(&status_input(&s), None, &t).status_line,
            "Uploading 3 changes"
        );
    }

    #[test]
    fn events_become_notices_only_when_they_need_the_player() {
        let conflict = Event::ConflictDetected(ConflictInfo {
            player: "A".into(),
            emulator: "cemu".into(),
            game: "g".into(),
            name: Some("BotW".into()),
            heads: vec![],
        });
        let (key, kind, ev) = notice_for(&conflict, None).unwrap();
        assert_eq!((key.as_str(), kind), ("A/cemu/g", "conflict"));
        assert_eq!(
            ev,
            NoticeEvent::Conflict {
                game: "BotW".into()
            }
        );
        let lease = |live| {
            Event::LeaseWarning(LeaseInfo {
                player: "A".into(),
                emulator: "cemu".into(),
                game: "g".into(),
                device_id: "d".into(),
                device: "Deck".into(),
                renewed_at_ms: 0,
                live,
                ours: false,
            })
        };
        assert!(notice_for(&lease(true), None).is_some());
        assert!(notice_for(&lease(false), None).is_none());
        let auth = Status::from_facts(&StatusFacts {
            error: Some(ApiErrorKind::Auth),
            ..Default::default()
        });
        let ev = Event::StatusChanged(auth.clone());
        assert!(notice_for(&ev, None).is_some());
        // Still the same error: no new notice.
        assert!(notice_for(&ev, Some(&auth)).is_none());
        let synced = Event::StatusChanged(Status::from_facts(&StatusFacts::default()));
        assert!(notice_for(&synced, None).is_none());
    }

    #[test]
    fn tray_clicks_become_requests() {
        assert_eq!(
            tray_request(&TrayAction::Pause1h, 1_000),
            Some(Request::Pause {
                until_ms: Some(1_000 + 3_600_000)
            })
        );
        assert_eq!(
            tray_request(&TrayAction::SyncNow, 0),
            Some(Request::Sync { dry_run: false })
        );
        assert_eq!(tray_request(&TrayAction::Quit, 0), None);
    }
}
