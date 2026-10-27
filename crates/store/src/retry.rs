//! Retries transient store errors with backoff (storage-backends §Interface).

use std::future::Future;
use std::sync::Arc;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use crate::error::{ErrorClass, StoreError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RetryPolicy {
    /// Total tries, the first one included.
    pub attempts: u32,
    pub base: Duration,
    pub cap: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 5,
            base: Duration::from_secs(1),
            cap: Duration::from_secs(30),
        }
    }
}

/// What the store tells its observer while it retries (reliability §Network).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetryNotice {
    /// A request failed with a transient error; try number `attempt` (2 or more, out of
    /// `max_attempts`) starts after `delay`.
    Retrying {
        operation: &'static str,
        attempt: u32,
        max_attempts: u32,
        delay: Duration,
    },
    /// A request succeeded after trouble, and no other request is still retrying.
    Recovered,
}

/// Receives every `RetryNotice`. Called from the store's task; keep it cheap.
pub type RetryObserver = Arc<dyn Fn(RetryNotice) + Send + Sync>;

/// The observer, and the connection's trouble state. Clones share the state, so one watch
/// given to every store an engine opens reports the recovery even when the request that gave
/// up ran on another store.
#[derive(Clone, Default)]
pub struct Watch {
    observer: Option<RetryObserver>,
    trouble: Arc<Mutex<Trouble>>,
}

/// Requests retrying right now, and whether anything failed since the last recovery.
#[derive(Default)]
struct Trouble {
    retrying: u32,
    troubled: bool,
}

impl Watch {
    #[must_use]
    pub fn new(observer: RetryObserver) -> Self {
        Self {
            observer: Some(observer),
            trouble: Arc::default(),
        }
    }

    /// Runs `f` on the shared state; notices are sent under the lock, so they stay in order.
    fn with<R>(&self, f: impl FnOnce(&mut Trouble) -> R) -> R {
        f(&mut self.trouble.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn tell(&self, n: RetryNotice) {
        if let Some(o) = &self.observer {
            o(n);
        }
    }
}

/// Runs `op(attempt)` (0-based) until it succeeds, fails with a non-transient error, or runs
/// out of attempts. The last error is returned unchanged. `watch` hears each retry, and
/// `Recovered` on a success once no other request is still retrying.
pub(crate) async fn retry<T, F, Fut>(
    policy: &RetryPolicy,
    watch: &Watch,
    operation: &'static str,
    mut op: F,
) -> Result<T, StoreError>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<T, StoreError>>,
{
    let mut attempt = 0;
    let mut counted = false;
    loop {
        match op(attempt).await {
            Err(e) if e.class() == ErrorClass::Transient && attempt + 1 < policy.attempts => {
                attempt += 1;
                let delay = wait(policy, attempt, &e);
                let first = !counted;
                counted = true;
                watch.with(|t| {
                    if first {
                        t.retrying += 1;
                    }
                    t.troubled = true;
                    watch.tell(RetryNotice::Retrying {
                        operation,
                        attempt: attempt + 1,
                        max_attempts: policy.attempts,
                        delay,
                    });
                });
                tokio::time::sleep(delay).await;
            }
            Err(e) => {
                watch.with(|t| {
                    if counted {
                        t.retrying -= 1;
                    }
                    if e.class() == ErrorClass::Transient {
                        t.troubled = true;
                    }
                });
                return Err(e);
            }
            Ok(v) => {
                watch.with(|t| {
                    if counted {
                        t.retrying -= 1;
                    }
                    if t.retrying == 0 && t.troubled {
                        t.troubled = false;
                        watch.tell(RetryNotice::Recovered);
                    }
                });
                return Ok(v);
            }
        }
    }
}

/// The wait before retry `n` (1-based): the server's `Retry-After`, else full jitter.
fn wait(policy: &RetryPolicy, n: u32, e: &StoreError) -> Duration {
    if let Some(after) = e.retry_after() {
        return after.min(policy.cap);
    }
    let ceiling = policy
        .base
        .saturating_mul(2u32.saturating_pow(n - 1))
        .min(policy.cap);
    let max = u64::try_from(ceiling.as_nanos()).unwrap_or(u64::MAX);
    let mut bytes = [0u8; 8];
    // No randomness means no jitter, not a failure.
    if getrandom::fill(&mut bytes).is_err() {
        return ceiling;
    }
    Duration::from_nanos(u64::from_le_bytes(bytes) % max.saturating_add(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn fast() -> RetryPolicy {
        RetryPolicy {
            attempts: 5,
            base: Duration::from_millis(1),
            cap: Duration::from_millis(4),
        }
    }

    #[tokio::test]
    async fn the_recovery_waits_for_every_retrying_request() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let s = seen.clone();
        let watch = Watch::new(Arc::new(move |n| s.lock().unwrap().push(n)));
        let a_done = tokio::sync::Notify::new();
        let recovered = || {
            seen.lock()
                .unwrap()
                .iter()
                .filter(|n| **n == RetryNotice::Recovered)
                .count()
        };
        let a = async {
            let r = retry(&fast(), &watch, "put", |n| async move {
                if n == 0 {
                    Err(err(ErrorClass::Transient))
                } else {
                    Ok(())
                }
            })
            .await;
            assert_eq!(recovered(), 0, "B is still retrying");
            a_done.notify_one();
            r
        };
        let policy = fast();
        let b = retry(&policy, &watch, "put", |n| {
            let a_done = &a_done;
            async move {
                match n {
                    0 => Err(err(ErrorClass::Transient)),
                    1 => {
                        a_done.notified().await;
                        Err(err(ErrorClass::Transient))
                    }
                    _ => Ok(()),
                }
            }
        });
        let (ra, rb) = tokio::join!(a, b);
        ra.unwrap();
        rb.unwrap();
        assert_eq!(recovered(), 1);
        assert_eq!(seen.lock().unwrap().last(), Some(&RetryNotice::Recovered));
    }

    fn err(class: ErrorClass) -> StoreError {
        StoreError::new(class, "boom")
    }

    #[tokio::test]
    async fn succeeds_after_two_transient_failures() {
        let calls = AtomicU32::new(0);
        let r = retry(&fast(), &Watch::default(), "test", |n| {
            calls.fetch_add(1, Ordering::SeqCst);
            async move {
                if n < 2 {
                    Err(err(ErrorClass::Transient))
                } else {
                    Ok(n)
                }
            }
        })
        .await;
        assert_eq!(r.unwrap(), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn gives_up_after_all_attempts_with_the_last_error() {
        let calls = AtomicU32::new(0);
        let r: Result<(), _> = retry(&fast(), &Watch::default(), "test", |n| {
            calls.fetch_add(1, Ordering::SeqCst);
            async move { Err(StoreError::new(ErrorClass::Transient, &format!("try {n}"))) }
        })
        .await;
        assert_eq!(r.unwrap_err().message(), "try 4");
        assert_eq!(calls.load(Ordering::SeqCst), 5);
    }

    #[tokio::test]
    async fn other_classes_are_never_retried() {
        for class in [
            ErrorClass::Auth,
            ErrorClass::NotFound,
            ErrorClass::AlreadyExists,
            ErrorClass::Permanent,
            ErrorClass::Quota,
        ] {
            let calls = AtomicU32::new(0);
            let r: Result<(), _> = retry(&fast(), &Watch::default(), "test", |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                async move { Err(err(class)) }
            })
            .await;
            assert_eq!(r.unwrap_err().class(), class);
            assert_eq!(calls.load(Ordering::SeqCst), 1, "{class:?}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_long_retry_after_waits_the_cap() {
        let policy = RetryPolicy {
            attempts: 2,
            base: Duration::from_millis(1),
            cap: Duration::from_secs(30),
        };
        let start = tokio::time::Instant::now();
        let r = retry(&policy, &Watch::default(), "test", |n| async move {
            if n == 0 {
                Err(err(ErrorClass::Transient).with_retry_after(Duration::from_secs(120)))
            } else {
                Ok(())
            }
        })
        .await;
        assert!(r.is_ok());
        let elapsed = start.elapsed();
        assert!(elapsed >= policy.cap && elapsed < policy.cap + Duration::from_secs(1));
    }

    #[test]
    fn jitter_stays_under_the_exponential_ceiling() {
        let policy = RetryPolicy::default();
        let e = err(ErrorClass::Transient);
        for n in 1..=8 {
            let ceiling = (policy.base * 2u32.pow(n - 1)).min(policy.cap);
            assert!(wait(&policy, n, &e) <= ceiling, "{n}");
        }
    }
}
