//! `lemusync play` over the terminal: the player's answers and one line per step
//! (sync-model §Launcher mode). Without a terminal, or with `--json`, nothing is asked:
//! a held lease fails with exit 5 and a conflict with exit 4 (never a silent `PlayAnyway`).

use lemusync_api::{ConflictInfo, PlayEvent};
use lemusync_engine::{EngineError, LeaseChoice, PlayPrompt, VersionId};

use crate::i18n::{I18n, s};
use crate::prompt;

/// `I18n` is neither `Send` nor `Sync`, so the prompt keeps the language and builds one per use.
pub struct TermPrompt {
    pub lang: &'static str,
    /// Questions may be asked (a terminal and no `--json`).
    pub ask: bool,
    /// Step lines on stderr (off with `--json`).
    pub events: bool,
    pub emulator: String,
}

impl TermPrompt {
    fn lock(&self) -> I18n {
        I18n::new(self.lang)
    }
}

impl PlayPrompt for TermPrompt {
    fn lease_held(&self, by: &str, ago: u64, stale: bool) -> Result<LeaseChoice, EngineError> {
        if !self.ask {
            return Err(EngineError::LeaseHeld(by.to_owned()));
        }
        let t = self.lock();
        let key = if stale {
            "prompt-lease-stale"
        } else {
            "prompt-lease-held"
        };
        let question = t.t(
            key,
            &[
                ("device", s(by)),
                ("ago", fluent_bundle::FluentValue::from(ago)),
            ],
        );
        let keys = if stale {
            "prompt-lease-keys-stale"
        } else {
            "prompt-lease-keys"
        };
        let answer = prompt::ask(&t, &format!("{question}\n{}", t.t(keys, &[])))?;
        Ok(if answer == t.t("answer-wait", &[]) {
            LeaseChoice::Wait
        } else if answer == t.t("answer-play-anyway", &[]) {
            LeaseChoice::PlayAnyway
        } else if stale && answer == t.t("answer-take-over", &[]) {
            LeaseChoice::TakeOver
        } else {
            return Err(EngineError::LeaseHeld(by.to_owned()));
        })
    }

    fn conflict(&self, c: &ConflictInfo) -> Result<Option<VersionId>, EngineError> {
        if !self.ask {
            return Err(EngineError::NeedsDecision(c.game.clone()));
        }
        let t = self.lock();
        eprintln!("{}", t.t("prompt-conflict", &[("game", s(c.game.clone()))]));
        for (i, h) in c.heads.iter().enumerate() {
            eprintln!(
                "  {}",
                t.t(
                    "conflict-head",
                    &[
                        ("n", fluent_bundle::FluentValue::from(i + 1)),
                        ("version", s(h.version.clone())),
                        ("device", s(h.device_name.clone())),
                        ("files", fluent_bundle::FluentValue::from(h.files)),
                    ],
                )
            );
        }
        let answer = prompt::ask(&t, &t.t("prompt-conflict-pick", &[]))?;
        match answer.parse::<usize>() {
            Ok(n) if (1..=c.heads.len()).contains(&n) => c.heads[n - 1]
                .version
                .parse()
                .map(Some)
                .map_err(|_| EngineError::NeedsDecision(c.game.clone())),
            _ => Err(EngineError::NeedsDecision(c.game.clone())),
        }
    }

    fn event(&self, e: PlayEvent) {
        if !self.events {
            return;
        }
        let key = match e {
            PlayEvent::Pulling => "play-pulling",
            PlayEvent::WaitingForLease => "play-waiting",
            PlayEvent::Playing => "play-running",
            PlayEvent::Uploading => "play-uploading",
        };
        eprintln!(
            "{}",
            self.lock()
                .t(key, &[("emulator", s(self.emulator.clone()))])
        );
    }
}

/// While the emulator runs, Ctrl-C only prints a hint: the emulator is in its own process
/// group, and this process stays to upload and release the lease when it exits.
pub fn hold_ctrl_c(t: &I18n, events: bool) -> tokio::task::JoinHandle<()> {
    let hint = t.t("play-ctrl-c", &[]);
    tokio::spawn(async move {
        while tokio::signal::ctrl_c().await.is_ok() {
            if events {
                eprintln!("{hint}");
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lemusync_api::HeadInfo;

    fn info() -> ConflictInfo {
        ConflictInfo {
            player: "P".into(),
            emulator: "cemu".into(),
            game: "101c9400".into(),
            heads: vec![HeadInfo {
                version: "v".into(),
                device_name: "A".into(),
                player: "P".into(),
                created_at_ms: 0,
                files: 1,
                bytes: 1,
                changed_files: 1,
            }],
        }
    }

    #[test]
    fn without_a_terminal_a_held_lease_is_exit_5_and_a_conflict_exit_4() {
        let p = TermPrompt {
            lang: "en",
            ask: false,
            events: false,
            emulator: "cemu".into(),
        };
        let e = p.lease_held("Laptop", 3, false).unwrap_err();
        assert_eq!(e.exit_code(), 5);
        let e = p.lease_held("Laptop", 900, true).unwrap_err();
        assert_eq!(e.exit_code(), 5, "a stale lease isn't taken over silently");
        assert_eq!(p.conflict(&info()).unwrap_err().exit_code(), 4);
    }

    #[test]
    fn play_messages_exist_in_english_and_spanish() {
        for key in [
            "play-pulling",
            "play-waiting",
            "play-running",
            "play-uploading",
            "play-ctrl-c",
            "play-done",
            "prompt-lease-held",
            "prompt-lease-stale",
            "prompt-conflict",
            "conflicts-none",
            "conflict-head",
            "resolve-done",
            "lease-none",
            "lease-line",
            "lease-released",
            "error-lease_held",
            "error-needs_decision",
        ] {
            let args = [("emulator", s("Cemu"))];
            let en = I18n::new("en").get(key, &args);
            let es = I18n::new("es").get(key, &args);
            assert!(en.is_some() && es.is_some(), "{key}");
            assert_ne!(en, es, "{key}");
        }
    }
}
