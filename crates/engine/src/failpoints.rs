//! Test-only fault injection: `hit` fails once at an armed point (testing §Fault injection).

use std::collections::BTreeSet;
use std::sync::Mutex;

use crate::EngineError;

#[derive(Default)]
pub struct Failpoints {
    armed: Mutex<BTreeSet<&'static str>>,
}

impl Failpoints {
    #[cfg(any(test, feature = "failpoints"))]
    pub fn arm(&self, name: &'static str) {
        if let Ok(mut armed) = self.armed.lock() {
            armed.insert(name);
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
