//! Test-only request faults (feature `faults`, never in release builds): fail queued attempts,
//! the Nth request once, or every request from the Nth on (a network cut) (testing §Layers).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use crate::{ErrorClass, StoreError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// Fail before touching the backend.
    Transient,
    /// Do the real call, then report `Transient` (a lost response).
    TransientAfterWrite,
}

#[derive(Default)]
struct Plan {
    queue: VecDeque<Fault>,
    nth: Option<(u32, Fault)>,
    cut_from: Option<u32>,
}

/// A shared handle: every clone of a store built with it sees the same plan and counters.
#[derive(Clone, Default)]
pub struct Faults {
    plan: Arc<Mutex<Plan>>,
    requests: Arc<AtomicU32>,
    reads: Arc<AtomicU32>,
}

impl Faults {
    /// Fails the next `n` attempts with `f`.
    ///
    /// # Panics
    /// If the plan's lock is poisoned (test-only code).
    pub fn push(&self, f: Fault, n: usize) {
        self.lock().queue.extend(std::iter::repeat_n(f, n));
    }

    /// Fails attempt number `n` (1-based, counted from now) with `f`, once.
    pub fn fail_nth(&self, n: u32, f: Fault) {
        let base = self.requests();
        self.lock().nth = Some((base + n, f));
    }

    /// Fails every attempt from number `n` (1-based, counted from now) until `heal`.
    pub fn cut_from(&self, n: u32) {
        let base = self.requests();
        self.lock().cut_from = Some(base + n);
    }

    /// Clears every planned fault.
    pub fn heal(&self) {
        *self.lock() = Plan::default();
    }

    /// Attempts made so far (each retry counts).
    #[must_use]
    pub fn requests(&self) -> u32 {
        self.requests.load(Ordering::SeqCst)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Plan> {
        self.plan
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Before an attempt: `Err` to fail now, `true` to fail after the real call.
    pub(crate) fn before(&self) -> Result<bool, StoreError> {
        let n = self.requests.fetch_add(1, Ordering::SeqCst) + 1;
        let mut plan = self.lock();
        let fault = plan.queue.pop_front().or_else(|| match plan.nth {
            Some((at, f)) if at == n => {
                plan.nth = None;
                Some(f)
            }
            _ => None,
        });
        match fault {
            Some(Fault::Transient) => Err(StoreError::new(ErrorClass::Transient, "injected")),
            Some(Fault::TransientAfterWrite) => Ok(true),
            None if plan.cut_from.is_some_and(|at| n >= at) => Err(StoreError::new(
                ErrorClass::Transient,
                "injected: network down",
            )),
            None => Ok(false),
        }
    }

    pub(crate) fn count_read(&self) {
        self.reads.fetch_add(1, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn reads(&self) -> u32 {
        self.reads.load(Ordering::SeqCst)
    }
}
