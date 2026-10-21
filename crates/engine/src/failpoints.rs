//! Test-only fault injection: `hit` fails once at an armed point, `pause` waits once at an armed
//! barrier so two engines can interleave deterministically (testing §Fault injection).
//!
//! Names: `upload.*`, `restore.*`, and `lease.after_write` (a pause between writing our lease
//! and listing again, used to force the both-write-at-once race).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use tokio::sync::Barrier;

use crate::EngineError;

#[derive(Default)]
pub struct Failpoints {
    armed: Mutex<BTreeSet<&'static str>>,
    barriers: Mutex<BTreeMap<&'static str, Arc<Barrier>>>,
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

    /// Waits once at `name`'s barrier, if one was armed.
    pub async fn pause(&self, name: &'static str) {
        let barrier = self.barriers.lock().ok().and_then(|mut b| b.remove(name));
        if let Some(b) = barrier {
            b.wait().await;
        }
    }

    /// # Errors
    /// `Injected(name)` once, if `name` was armed.
    pub fn hit(&self, name: &'static str) -> Result<(), EngineError> {
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
}
