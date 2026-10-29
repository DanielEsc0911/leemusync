//! `play` run by the daemon for a client (D35): steps go out as frames, and the lease and
//! conflict questions go to the client, whose answer comes back. No client (the tray), a
//! dropped connection, or no answer in time → the non-interactive answer: a held lease fails
//! with `LeaseHeld` (exit 5) and a conflict with `NeedsDecision` (exit 4), never a silent
//! "play anyway" (sync-model §Launcher mode).

use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use lemusync_api::{ConflictInfo, LeaseAnswer, PlayAnswer, PlayEvent, PlayQuestion};
use tokio::sync::mpsc;

use crate::{EngineError, LeaseChoice, PlayPrompt, VersionId};

/// How long a question waits for the player's answer.
pub const PROMPT_TIMEOUT: Duration = Duration::from_mins(10);

/// A frame for the client while `play` runs.
#[derive(Debug)]
pub enum PlayFrame {
    /// A step.
    Event(PlayEvent),
    /// A question; send the answer back on the channel.
    Ask(PlayQuestion, std_mpsc::SyncSender<PlayAnswer>),
}

/// One `play` for the service. `io: None` is the non-interactive policy (the tray).
#[derive(Debug)]
pub struct PlayCall {
    /// Player name.
    pub player: String,
    /// Emulator profile id.
    pub emulator: String,
    /// Game id.
    pub game: String,
    /// Extra emulator arguments.
    pub args: Vec<String>,
    /// Frames for the client.
    pub io: Option<mpsc::UnboundedSender<PlayFrame>>,
}

pub(crate) struct ChannelPrompt {
    pub io: Option<mpsc::UnboundedSender<PlayFrame>>,
    pub timeout: Duration,
}

impl ChannelPrompt {
    /// The client's answer, or `Refuse`. Blocks the engine thread while the player decides
    /// (at most `timeout`); `status`, `pause` and `resume` wait meanwhile.
    fn ask(&self, q: PlayQuestion) -> PlayAnswer {
        let Some(io) = &self.io else {
            return PlayAnswer::Refuse;
        };
        let (tx, rx) = std_mpsc::sync_channel(1);
        if io.send(PlayFrame::Ask(q, tx)).is_err() {
            return PlayAnswer::Refuse;
        }
        rx.recv_timeout(self.timeout).unwrap_or(PlayAnswer::Refuse)
    }
}

impl PlayPrompt for ChannelPrompt {
    fn lease_held(&self, by: &str, ago: u64, stale: bool) -> Result<LeaseChoice, EngineError> {
        let q = PlayQuestion::Lease {
            by: by.to_owned(),
            ago_s: ago,
            stale,
        };
        match self.ask(q) {
            PlayAnswer::Lease { choice } => Ok(match choice {
                LeaseAnswer::Wait => LeaseChoice::Wait,
                LeaseAnswer::PlayAnyway => LeaseChoice::PlayAnyway,
                LeaseAnswer::TakeOver => LeaseChoice::TakeOver,
            }),
            PlayAnswer::Keep { .. } | PlayAnswer::Refuse => {
                Err(EngineError::LeaseHeld(by.to_owned()))
            }
        }
    }

    fn conflict(&self, c: &ConflictInfo) -> Result<Option<VersionId>, EngineError> {
        let refuse = || EngineError::NeedsDecision(c.game.clone());
        match self.ask(PlayQuestion::Conflict {
            conflict: c.clone(),
        }) {
            PlayAnswer::Keep { version: None } => Ok(None),
            PlayAnswer::Keep { version: Some(v) } => v.parse().map(Some).map_err(|_| refuse()),
            PlayAnswer::Lease { .. } | PlayAnswer::Refuse => Err(refuse()),
        }
    }

    fn event(&self, e: PlayEvent) {
        if let Some(io) = &self.io {
            let _ = io.send(PlayFrame::Event(e));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conflict() -> ConflictInfo {
        ConflictInfo {
            player: "P".into(),
            emulator: "cemu".into(),
            game: "g".into(),
            name: None,
            heads: vec![],
        }
    }

    #[test]
    fn no_client_or_a_gone_client_is_the_non_interactive_answer() {
        let none = ChannelPrompt {
            io: None,
            timeout: Duration::from_secs(1),
        };
        assert_eq!(
            none.lease_held("Deck", 1, false).unwrap_err().exit_code(),
            5
        );
        assert_eq!(none.conflict(&conflict()).unwrap_err().exit_code(), 4);
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);
        let gone = ChannelPrompt {
            io: Some(tx),
            timeout: Duration::from_secs(1),
        };
        assert_eq!(gone.lease_held("Deck", 1, true).unwrap_err().exit_code(), 5);
    }

    #[test]
    fn the_clients_answer_is_used_and_silence_times_out() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let p = ChannelPrompt {
            io: Some(tx),
            timeout: Duration::from_secs(5),
        };
        let answerer = std::thread::spawn(move || {
            let Some(PlayFrame::Ask(PlayQuestion::Lease { by, .. }, reply)) = rx.blocking_recv()
            else {
                panic!("expected a lease question");
            };
            assert_eq!(by, "Deck");
            reply
                .send(PlayAnswer::Lease {
                    choice: LeaseAnswer::Wait,
                })
                .unwrap();
            // The next question gets no answer: the sender is dropped.
            let Some(PlayFrame::Ask(..)) = rx.blocking_recv() else {
                panic!("expected a conflict question");
            };
        });
        assert_eq!(p.lease_held("Deck", 3, false).unwrap(), LeaseChoice::Wait);
        assert_eq!(p.conflict(&conflict()).unwrap_err().exit_code(), 4);
        answerer.join().unwrap();
    }
}
