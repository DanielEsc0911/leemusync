//! The live progress line on stderr while `sync`, `restore` or `accounts recover` runs
//! (config-and-cli §Commands). Only on a terminal, never with `--json`.

use std::future::Future;
use std::io::{IsTerminal, Write};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use fluent_bundle::FluentValue;
use lemusync_api::{TransferPhase, TransferProgress};
use lemusync_engine::ProgressSink;

use crate::i18n::{I18n, s};

const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const BAR_CELLS: u64 = 12;
const REDRAW: Duration = Duration::from_millis(100);

pub struct Live {
    latest: Arc<Mutex<Option<TransferProgress>>>,
    enabled: bool,
    frame: usize,
    /// When the shown stream entered its current phase, for the rate.
    since: Option<(String, String, TransferPhase, Instant)>,
}

impl Live {
    /// On only for a terminal on stderr, a `TERM` other than `dumb`, and no `--json`.
    pub fn new(json: bool) -> Self {
        let dumb = std::env::var("TERM").is_ok_and(|t| t == "dumb");
        Self {
            latest: Arc::default(),
            enabled: !json && !dumb && std::io::stderr().is_terminal(),
            frame: 0,
            since: None,
        }
    }

    /// Whether the live line is drawn.
    pub fn on(&self) -> bool {
        self.enabled
    }

    pub fn sink(&self) -> ProgressSink {
        let latest = self.latest.clone();
        Arc::new(move |p| *latest.lock().unwrap_or_else(PoisonError::into_inner) = Some(p.clone()))
    }

    /// Runs `fut`, redrawing the line every 100 ms, and clears it before returning.
    pub async fn run<F: Future>(&mut self, t: &I18n, fut: F) -> F::Output {
        if !self.enabled {
            return fut.await;
        }
        tokio::pin!(fut);
        let mut tick = tokio::time::interval(REDRAW);
        let out = loop {
            tokio::select! {
                out = &mut fut => break out,
                _ = tick.tick() => self.draw(t),
            }
        };
        let mut err = std::io::stderr();
        let _ = write!(err, "\r\x1b[2K");
        let _ = err.flush();
        out
    }

    fn draw(&mut self, t: &I18n) {
        let Some(p) = self
            .latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        else {
            return;
        };
        let started = match &self.since {
            Some((pl, g, ph, at)) if *pl == p.player && *g == p.game && *ph == p.phase => *at,
            _ => {
                let now = Instant::now();
                self.since = Some((p.player.clone(), p.game.clone(), p.phase, now));
                now
            }
        };
        let width = std::env::var("COLUMNS")
            .ok()
            .and_then(|c| c.parse::<usize>().ok())
            .unwrap_or(80);
        let text: String = format!(
            "{} {}",
            FRAMES[self.frame % FRAMES.len()],
            line(t, &p, started.elapsed())
        )
        .chars()
        .take(width.saturating_sub(1))
        .collect();
        self.frame += 1;
        let mut err = std::io::stderr();
        let _ = write!(err, "\r\x1b[2K{text}");
        let _ = err.flush();
    }
}

/// Decimal megabytes, one decimal place, no unit: `1.5`.
pub fn mb(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)] // display only
    let v = bytes as f64 / 1e6;
    format!("{v:.1}")
}

fn bar(done: u64, total: u64) -> String {
    let full = done
        .min(total)
        .saturating_mul(BAR_CELLS)
        .checked_div(total)
        .unwrap_or(BAR_CELLS);
    let full = usize::try_from(full).unwrap_or(0);
    let cells = usize::try_from(BAR_CELLS).unwrap_or(0);
    format!("{}{}", "█".repeat(full), "░".repeat(cells - full))
}

/// The line's text without the spinner.
pub fn line(t: &I18n, p: &TransferProgress, elapsed: Duration) -> String {
    let key = match p.phase {
        TransferPhase::Checking => {
            return t.t(
                "progress-checking",
                &[("player", s(p.player.clone())), ("game", s(p.game.clone()))],
            );
        }
        TransferPhase::Uploading => "progress-uploading",
        TransferPhase::Downloading => "progress-downloading",
    };
    let secs = elapsed.as_secs_f64();
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let rate = if secs > 0.0 {
        (p.bytes_done as f64 / secs) as u64
    } else {
        0
    };
    t.t(
        key,
        &[
            ("player", s(p.player.clone())),
            ("game", s(p.game.clone())),
            ("bar", s(bar(p.bytes_done, p.bytes_total))),
            ("done", FluentValue::from(p.files_done)),
            ("total", FluentValue::from(p.files_total)),
            ("size_done", s(mb(p.bytes_done))),
            ("size_total", s(mb(p.bytes_total))),
            ("rate", s(mb(rate))),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(phase: TransferPhase) -> TransferProgress {
        TransferProgress {
            player: "Ana".into(),
            emulator: "cemu".into(),
            game: "101c9400".into(),
            phase,
            files_done: 3,
            files_total: 12,
            bytes_done: 4_200_000,
            bytes_total: 16_800_000,
        }
    }

    #[test]
    fn the_line_shows_bar_files_size_and_rate() {
        let en = I18n::new("en");
        let up = line(&en, &p(TransferPhase::Uploading), Duration::from_secs(2));
        assert_eq!(
            up,
            "Ana · 101c9400 · uploading ███░░░░░░░░░ 3/12 · 4.2/16.8 MB · 2.1 MB/s"
        );
        let es = I18n::new("es");
        let down = line(&es, &p(TransferPhase::Downloading), Duration::ZERO);
        assert!(
            down.starts_with("Ana · 101c9400 · descargando ███"),
            "{down}"
        );
        assert!(down.ends_with("0.0 MB/s"), "{down}");
        let check = line(&en, &p(TransferPhase::Checking), Duration::ZERO);
        assert_eq!(check, "Ana · 101c9400 · checking…");
    }

    #[test]
    fn the_bar_is_full_when_there_is_nothing_to_send() {
        assert_eq!(bar(0, 0), "█".repeat(12));
        assert_eq!(bar(5, 10), format!("{}{}", "█".repeat(6), "░".repeat(6)));
    }
}
