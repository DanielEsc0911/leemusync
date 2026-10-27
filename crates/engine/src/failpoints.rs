//! Test-only fault injection: `hit` fails once at an armed point (or, in `Kill` mode, aborts the
//! process there, feature `failpoints` only), `pause` waits once at an armed barrier so two
//! engines can interleave deterministically (testing §Layers).
//!
//! Names: `init.*`, `upload.*`, `restore.*`, `resolve.after_commit`, `lease.after_list` (a
//! pause after the first listing, so both engines list before either writes) and
//! `lease.after_write` (a hit, and a pause between writing our lease and listing again); the
//! two pauses together force the both-write-at-once race. `tests/kill.rs` fails when a hit name
//! has no crash case. A pause whose barrier never fills panics after `PAUSE_TIMEOUT` instead of
//! hanging.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::Barrier;

use crate::EngineError;

/// How long `pause` waits for the other side before failing the test.
const PAUSE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct Failpoints {
    armed: Mutex<BTreeSet<&'static str>>,
    barriers: Mutex<BTreeMap<&'static str, Arc<Barrier>>>,
    #[cfg(any(test, feature = "failpoints"))]
    kills: Mutex<BTreeSet<&'static str>>,
}

impl Failpoints {
    #[cfg(any(test, feature = "failpoints"))]
    pub fn arm(&self, name: &'static str) {
        if let Ok(mut armed) = self.armed.lock() {
            armed.insert(name);
        }
    }

    #[cfg(any(test, feature = "failpoints"))]
    pub fn arm_pause(&self, name: &'static str, barrier: Arc<Barrier>) {
        if let Ok(mut b) = self.barriers.lock() {
            b.insert(name, barrier);
        }
    }

    /// `Kill` mode: `hit(name)` aborts the process, like `kill -9` at that step.
    #[cfg(any(test, feature = "failpoints"))]
    pub fn arm_kill(&self, name: &'static str) {
        if let Ok(mut k) = self.kills.lock() {
            k.insert(name);
        }
    }

    /// Waits once at `name`'s barrier, if one was armed.
    ///
    /// # Panics
    /// If the barrier doesn't fill within `PAUSE_TIMEOUT`: a test bug, never a hang.
    #[allow(clippy::panic)] // test-only: barriers are only armed by tests
    pub async fn pause(&self, name: &'static str) {
        let barrier = self.barriers.lock().ok().and_then(|mut b| b.remove(name));
        if let Some(b) = barrier
            && tokio::time::timeout(PAUSE_TIMEOUT, b.wait()).await.is_err()
        {
            panic!("failpoint pause `{name}`: barrier not filled within {PAUSE_TIMEOUT:?}");
        }
    }

    /// # Errors
    /// `Injected(name)` once, if `name` was armed.
    pub fn hit(&self, name: &'static str) -> Result<(), EngineError> {
        #[cfg(any(test, feature = "failpoints"))]
        if self.kills.lock().is_ok_and(|k| k.contains(name)) {
            std::process::abort();
        }
        let fired = self.armed.lock().is_ok_and(|mut armed| armed.remove(name));
        if fired {
            Err(EngineError::Injected(name))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_armed_point_fails_once() {
        let f = Failpoints::default();
        assert!(f.hit("a").is_ok());
        f.arm("a");
        assert!(matches!(f.hit("b"), Ok(())));
        assert!(matches!(f.hit("a"), Err(EngineError::Injected("a"))));
        assert!(f.hit("a").is_ok());
    }

    #[tokio::test(start_paused = true)]
    #[should_panic(expected = "barrier not filled")]
    async fn a_pause_that_never_fills_fails_instead_of_hanging() {
        let f = Failpoints::default();
        f.arm_pause("p", Arc::new(Barrier::new(2)));
        f.pause("p").await;
    }
}
