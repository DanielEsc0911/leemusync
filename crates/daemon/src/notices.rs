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
    Conflict { game: String },
    ForeignLease { game: String, device: String },
    AuthError,
    QuotaError,
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
    }
}
