//! Manual desktop check (plan Phase 2 Task 6, Step 3): shows the tray and swaps its state,
//! sends a notification with two actions, holds an idle block, and prints what comes back.
//! `cargo run -p lemusync-daemon --example desktop_check [seconds]`. Never suspends anything.

use std::time::Duration;

use lemusync_daemon::i18n::I18n;
use lemusync_daemon::notices::{NoticeEvent, notice};
use lemusync_daemon::platform::{start_notifier, start_power, start_tray};
use lemusync_daemon::view::{ErrorKind, StatusInput, ViewState, tray_view};
use tokio::sync::mpsc;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let secs = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20u64);
    let t = I18n::from_env();
    let (atx, mut arx) = mpsc::channel(8);
    let (ntx, mut nrx) = mpsc::channel(8);
    let tray = start_tray(atx);
    let notifier = start_notifier(ntx);
    let power = start_power();
    let mut edges = power.sleep_events();
    let guard = power.block_idle("desktop_check: holding an idle block");
    let id = notifier.notify(notice(
        &NoticeEvent::ForeignLease {
            game: "BotW".into(),
            device: "Deck".into(),
        },
        &t,
    ));
    println!("notice sent: {id:?}");
    let states = [
        ViewState::Synced,
        ViewState::Syncing,
        ViewState::Error(ErrorKind::Auth),
    ];
    let deadline = tokio::time::sleep(Duration::from_secs(secs));
    tokio::pin!(deadline);
    let mut tick = tokio::time::interval(Duration::from_secs(3));
    let mut i = 0;
    loop {
        tokio::select! {
            () = &mut deadline => break,
            _ = tick.tick() => {
                let st = StatusInput { uploads: 2, ..StatusInput::new(states[i % states.len()]) };
                let view = tray_view(&st, None, &t);
                println!("tray: {:?} {}", view.icon, view.status_line);
                tray.show(&view);
                i += 1;
            }
            Some(a) = arx.recv() => println!("tray action: {a:?}"),
            Some((id, a)) = nrx.recv() => println!("notice action: {id:?} {a}"),
            Some(e) = edges.recv() => { println!("sleep edge: {e:?}"); power.ack_suspend(); }
        }
    }
    drop(guard);
    println!("idle block released");
    tokio::time::sleep(Duration::from_millis(300)).await;
}
