//! The "connection trouble" notice on stderr while the store retries (config-and-cli
//! §Connection notice). Silent for one quick retry; at most two lines per outage; never with
//! `--json`.

use std::io::Write;
use std::sync::{Arc, Mutex, PoisonError};

use lemusync_api::ConnectionEvent;
use lemusync_engine::ConnectionSink;

use crate::i18n::I18n;

/// Retries in one outage before the notice shows: the first quick retry stays quiet.
const SHOW_FROM: u32 = 2;

/// One outage's state: retries seen since the connection was last fine, and whether the
/// trouble line was printed.
#[derive(Default)]
pub struct Notice {
    retries: u32,
    shown: bool,
}

impl Notice {
    /// The message key to print for `e`, if any.
    pub fn on(&mut self, e: &ConnectionEvent) -> Option<&'static str> {
        match e {
            ConnectionEvent::Retrying { .. } => {
                self.retries = self.retries.saturating_add(1);
                if self.retries >= SHOW_FROM && !self.shown {
                    self.shown = true;
                    return Some("connection-trouble");
                }
                None
            }
            ConnectionEvent::Recovered => {
                let was = self.shown;
                *self = Self::default();
                was.then_some("connection-back")
            }
        }
    }
}

/// A sink that prints the notice lines in the user's language. `clear` wipes the live
/// progress line first so the two don't mix.
pub fn sink(lang: &'static str, clear: bool) -> ConnectionSink {
    let notice = Arc::new(Mutex::new(Notice::default()));
    Arc::new(move |e| {
        let key = notice.lock().unwrap_or_else(PoisonError::into_inner).on(e);
        if let Some(key) = key {
            let text = I18n::new(lang).t(key, &[]);
            let mut err = std::io::stderr();
            let _ = if clear {
                writeln!(err, "\r\x1b[2K{text}")
            } else {
                writeln!(err, "{text}")
            };
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn retry(attempt: u32) -> ConnectionEvent {
        ConnectionEvent::Retrying {
            operation: "put".into(),
            attempt,
            max_attempts: 5,
            delay_ms: 1000,
        }
    }

    #[test]
    fn one_quick_retry_says_nothing() {
        let mut n = Notice::default();
        assert_eq!(n.on(&retry(2)), None);
        assert_eq!(n.on(&ConnectionEvent::Recovered), None);
    }

    #[test]
    fn an_outage_prints_two_lines_at_most() {
        let mut n = Notice::default();
        let lines: Vec<_> = [retry(2), retry(3), retry(4), retry(2), retry(3)]
            .iter()
            .chain([&ConnectionEvent::Recovered])
            .filter_map(|e| n.on(e))
            .collect();
        assert_eq!(lines, ["connection-trouble", "connection-back"]);
        assert_eq!(n.on(&retry(2)), None, "a new outage starts quiet again");
        assert_eq!(n.on(&retry(3)), Some("connection-trouble"));
    }

    #[test]
    fn the_lines_exist_in_english_and_spanish() {
        let en = I18n::new("en");
        assert_eq!(
            en.t("connection-trouble", &[]),
            "Connection trouble, retrying…"
        );
        assert_eq!(en.t("connection-back", &[]), "Connection is back.");
        let es = I18n::new("es");
        assert_eq!(
            es.t("connection-trouble", &[]),
            "Problemas de conexión, reintentando…"
        );
        assert_eq!(es.t("connection-back", &[]), "La conexión volvió.");
    }
}
