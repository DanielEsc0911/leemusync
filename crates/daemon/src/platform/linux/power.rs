//! Sleep and idle hooks (reliability §Sleep and resume):
//! - per `IdleGuard`: logind `Inhibit("idle", …, "block")`, plus
//!   `org.freedesktop.PowerManagement.Inhibit` on the session bus when that service exists
//!   (XFCE ignores logind idle locks); released on drop or after `IDLE_BLOCK_MAX`.
//! - while running: a logind `Inhibit("sleep", …, "delay")` lock, closed on
//!   `PrepareForSleep(true)` once the daemon calls `ack_suspend` (or the delay is nearly up),
//!   retaken on `PrepareForSleep(false)`.
//!
//! Never a `block` lock on `sleep`: a suspend the user asks for always wins.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::{Notify, mpsc, oneshot};
use zbus::zvariant::OwnedFd;
use zbus::{Connection, MatchRule, MessageStream, message};

use crate::platform::{IDLE_BLOCK_MAX, IdleGuard, Power, SleepEdge};

const WHO: &str = "LemuSync";
/// logind's default `InhibitDelayMaxUSec` is 5 s; close the lock before it.
const ACK_WAIT: Duration = Duration::from_secs(4);

struct LinuxPower {
    edges: Mutex<Option<mpsc::Receiver<SleepEdge>>>,
    ack: Arc<Notify>,
}

impl Power for LinuxPower {
    fn block_idle(&self, reason: &str) -> IdleGuard {
        let (release, released) = oneshot::channel::<()>();
        let reason = reason.to_owned();
        tokio::spawn(async move {
            let logind = logind_inhibit("idle", &reason, "block").await;
            let desktop = desktop_inhibit(&reason).await;
            // Guard dropped (also before the locks were taken) or the cap reached.
            let _ = tokio::time::timeout(IDLE_BLOCK_MAX, released).await;
            drop(logind);
            if let Some((conn, cookie)) = desktop {
                let _ = conn
                    .call_method(
                        Some("org.freedesktop.PowerManagement"),
                        "/org/freedesktop/PowerManagement/Inhibit",
                        Some("org.freedesktop.PowerManagement.Inhibit"),
                        "UnInhibit",
                        &(cookie,),
                    )
                    .await;
            }
        });
        IdleGuard::new(move || {
            let _ = release.send(());
        })
    }

    fn sleep_events(&self) -> mpsc::Receiver<SleepEdge> {
        self.edges
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .unwrap_or_else(|| mpsc::channel(1).1)
    }

    fn ack_suspend(&self) {
        // Only a suspend in progress is acked; no stored permit for the next one.
        self.ack.notify_waiters();
    }
}

/// A logind inhibitor fd; the lock lives as long as the fd.
async fn logind_inhibit(what: &str, why: &str, mode: &str) -> Option<OwnedFd> {
    let conn = Connection::system().await.ok()?;
    logind_inhibit_on(&conn, what, why, mode).await
}

async fn logind_inhibit_on(
    conn: &Connection,
    what: &str,
    why: &str,
    mode: &str,
) -> Option<OwnedFd> {
    let reply = conn
        .call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            "Inhibit",
            &(what, WHO, why, mode),
        )
        .await
        .ok()?;
    reply.body().deserialize::<OwnedFd>().ok()
}

/// The XFCE power manager's own inhibit list. The lock is tied to this connection.
async fn desktop_inhibit(why: &str) -> Option<(Connection, u32)> {
    let conn = Connection::session().await.ok()?;
    let reply = conn
        .call_method(
            Some("org.freedesktop.PowerManagement"),
            "/org/freedesktop/PowerManagement/Inhibit",
            Some("org.freedesktop.PowerManagement.Inhibit"),
            "Inhibit",
            &(WHO, why),
        )
        .await
        .ok()?;
    let cookie = reply.body().deserialize::<u32>().ok()?;
    Some((conn, cookie))
}

async fn sleep_loop(tx: mpsc::Sender<SleepEdge>, ack: Arc<Notify>) -> Option<()> {
    let conn = Connection::system().await.ok()?;
    let rule = MatchRule::builder()
        .msg_type(message::Type::Signal)
        .sender("org.freedesktop.login1")
        .ok()?
        .interface("org.freedesktop.login1.Manager")
        .ok()?
        .member("PrepareForSleep")
        .ok()?
        .build();
    let mut signals = MessageStream::for_match_rule(rule, &conn, Some(8))
        .await
        .ok()?;
    let why = "Checkpoint saves before suspend";
    let mut lock = logind_inhibit_on(&conn, "sleep", why, "delay").await;
    while let Some(Ok(msg)) = signals.next().await {
        let Ok(start) = msg.body().deserialize::<bool>() else {
            continue;
        };
        if start {
            // Registered before the edge goes out, so a fast ack isn't missed.
            let acked = ack.notified();
            tokio::pin!(acked);
            acked.as_mut().enable();
            if tx.send(SleepEdge::Suspending).await.is_err() {
                return None;
            }
            let _ = tokio::time::timeout(ACK_WAIT, acked).await;
            lock = None;
        } else {
            if lock.is_none() {
                lock = logind_inhibit_on(&conn, "sleep", why, "delay").await;
            }
            if tx.send(SleepEdge::Resumed).await.is_err() {
                return None;
            }
        }
    }
    drop(lock);
    Some(())
}

/// Start the power hooks and take the sleep delay lock. Must be called inside a tokio runtime.
#[must_use]
pub fn start_power() -> Box<dyn Power> {
    let (tx, rx) = mpsc::channel(8);
    let ack = Arc::new(Notify::new());
    tokio::spawn(sleep_loop(tx, Arc::clone(&ack)));
    Box::new(LinuxPower {
        edges: Mutex::new(Some(rx)),
        ack,
    })
}
