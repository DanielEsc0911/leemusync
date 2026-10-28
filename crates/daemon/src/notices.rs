//! The notifications the daemon sends, localised. Pure. Policy:
//! docs/specs/ui/status-and-notifications.md §Notification policy (coalescing is the
//! caller's job). Action ids are stable English; labels are localised.

use crate::i18n::{I18n, s};
use crate::platform::{Notice, NoticeKind};

/// Action ids carried back with a click.
pub const ACTION_REVIEW: &str = "review";
pub const ACTION_WAIT: &str = "wait";
pub const ACTION_OPEN: &str = "open";

/// What happened that needs the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoticeEvent {
    Conflict {
        game: String,
    },
    ForeignLease {
        game: String,
        device: String,
    },
    AuthError,
    QuotaError,
    /// Watch mode: another device has a newer save; it updates once the emulator closes.
    RemoteNewer {
        game: String,
        device: String,
        emulator: String,
    },
}

/// The window in which one stream's notification of one kind is sent once
/// (status-and-notifications §Notification policy).
pub const COALESCE_MS: u64 = 5 * 60_000;

/// Coalesces bursts: one notification per stream per event type per [`COALESCE_MS`].
#[derive(Debug, Default)]
pub struct Coalescer {
    last: std::collections::HashMap<(String, &'static str), u64>,
}

impl Coalescer {
    /// True when a notification for `(stream, kind)` may go out at `now_ms`; records it.
    pub fn allow(&mut self, stream: &str, kind: &'static str, now_ms: u64) -> bool {
        let key = (stream.to_owned(), kind);
        if self
            .last
            .get(&key)
            .is_some_and(|t| now_ms.saturating_sub(*t) < COALESCE_MS)
        {
            return false;
        }
        self.last.insert(key, now_ms);
        true
    }
}

#[must_use]
pub fn notice(ev: &NoticeEvent, t: &I18n) -> Notice {
    let action = |id: &str, key: &str| (id.to_owned(), t.t(key, &[]));
    match ev {
        NoticeEvent::Conflict { game } => Notice {
            kind: NoticeKind::Attention,
            title: t.t("notice-conflict-title", &[("game", s(game.clone()))]),
            body: t.t("notice-conflict-body", &[]),
            actions: vec![action(ACTION_REVIEW, "notice-action-review")],
        },
        NoticeEvent::ForeignLease { game, device } => Notice {
            kind: NoticeKind::Attention,
            title: t.t(
                "notice-foreign-lease-title",
                &[("game", s(game.clone())), ("device", s(device.clone()))],
            ),
            body: t.t("notice-foreign-lease-body", &[]),
            actions: vec![
                action(ACTION_WAIT, "notice-action-wait"),
                action(ACTION_OPEN, "notice-action-open"),
            ],
        },
        NoticeEvent::AuthError => Notice {
            kind: NoticeKind::Error,
            title: t.t("status-error-auth", &[]),
            body: t.t("notice-auth-body", &[]),
            actions: vec![action(ACTION_OPEN, "notice-action-open")],
        },
        NoticeEvent::RemoteNewer {
            game,
            device,
            emulator,
        } => Notice {
            kind: NoticeKind::Attention,
            title: t.t(
                "notice-remote-newer-title",
                &[("game", s(game.clone())), ("device", s(device.clone()))],
            ),
            body: t.t(
                "notice-remote-newer-body",
                &[("emulator", s(emulator.clone()))],
            ),
            actions: vec![action(ACTION_OPEN, "notice-action-open")],
        },
        NoticeEvent::QuotaError => Notice {
            kind: NoticeKind::Error,
            title: t.t("status-error-quota", &[]),
            body: t.t("notice-quota-body", &[]),
            actions: vec![action(ACTION_OPEN, "notice-action-open")],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_notice_in_en_and_es() {
        let events = [
            NoticeEvent::Conflict {
                game: "BotW".into(),
            },
            NoticeEvent::ForeignLease {
                game: "BotW".into(),
                device: "Deck".into(),
            },
            NoticeEvent::AuthError,
            NoticeEvent::QuotaError,
            NoticeEvent::RemoteNewer {
                game: "BotW".into(),
                device: "Deck".into(),
                emulator: "Cemu".into(),
            },
        ];
        for lang in ["en", "es"] {
            let t = I18n::new(lang);
            for ev in &events {
                let n = notice(ev, &t);
                for text in [&n.title, &n.body]
                    .into_iter()
                    .chain(n.actions.iter().map(|(_, l)| l))
                {
                    assert!(!text.starts_with("notice-"), "{lang} {ev:?}: {text}");
                    assert!(text.chars().any(char::is_alphabetic));
                }
            }
        }
        let es = notice(&events[1], &I18n::new("es"));
        assert_eq!(es.title, "BotW está en uso en Deck");
        assert_eq!(es.actions[0], ("wait".into(), "Esperar".into()));
        let en = notice(&events[4], &I18n::new("en"));
        assert_eq!(en.body, "Close Cemu and LemuSync updates it.");
    }

    #[test]
    fn one_notice_per_stream_and_kind_per_five_minutes() {
        let mut c = Coalescer::default();
        assert!(c.allow("s1", "conflict", 0));
        assert!(!c.allow("s1", "conflict", COALESCE_MS - 1));
        assert!(c.allow("s1", "lease", 1));
        assert!(c.allow("s2", "conflict", 1));
        assert!(c.allow("s1", "conflict", COALESCE_MS));
    }
}
