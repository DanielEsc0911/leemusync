//! Retries transient store errors with backoff (storage-backends §Interface).

use std::future::Future;
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

/// Runs `op(attempt)` (0-based) until it succeeds, fails with a non-transient error, or runs
/// out of attempts. The last error is returned unchanged.
pub(crate) async fn retry<T, F, Fut>(policy: &RetryPolicy, mut op: F) -> Result<T, StoreError>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<T, StoreError>>,
{
    let mut attempt = 0;
    loop {
        match op(attempt).await {
            Err(e) if e.class() == ErrorClass::Transient && attempt + 1 < policy.attempts => {
                attempt += 1;
                tokio::time::sleep(wait(policy, attempt, &e)).await;
            }
            other => return other,
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

    fn err(class: ErrorClass) -> StoreError {
        StoreError::new(class, "boom")
    }

    #[tokio::test]
    async fn succeeds_after_two_transient_failures() {
        let calls = AtomicU32::new(0);
        let r = retry(&fast(), |n| {
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
        let r: Result<(), _> = retry(&fast(), |n| {
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
            let r: Result<(), _> = retry(&fast(), |_| {
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
        let r = retry(&policy, |n| async move {
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
