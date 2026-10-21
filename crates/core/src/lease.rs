//! Advisory per-stream leases: format and pure rules. No clock: callers pass `now`.
//! Spec: docs/specs/sync-model.md (Leases).

use serde::{Deserialize, Serialize};

use crate::{DeviceId, PlayerId, SessionId};

pub const FORMAT: u32 = 1;
/// How often a holder renews its lease.
pub const RENEW_MS: i64 = 60_000;
/// Default lease lifetime (`config.toml` `[leases] ttl_secs`).
pub const DEFAULT_TTL_MS: i64 = 5 * 60_000;
/// Clock skew tolerated between devices, and between a device and the server.
pub const SKEW_MS: i64 = 2 * 60_000;

/// The plaintext of `locks/<streamId>/<deviceId>`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lease {
    pub format: u32,
    pub device: DeviceId,
    pub player: PlayerId,
    pub session: SessionId,
    /// Unix milliseconds on the holder's clock.
    pub acquired_at_ms: i64,
    pub renewed_at_ms: i64,
    pub ttl_ms: i64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LeaseError {
    #[error("unsupported lease format {0}")]
    Format(u32),
    #[error("lease JSON: {0}")]
    Json(String),
}

impl Lease {
    /// # Errors
    /// `LeaseError::Json` if serialisation fails.
    pub fn to_json(&self) -> Result<Vec<u8>, LeaseError> {
        serde_json::to_vec(self).map_err(|e| LeaseError::Json(e.to_string()))
    }

    /// # Errors
    /// `Json` for malformed input; `Format` for an unknown format.
    pub fn from_json(bytes: &[u8]) -> Result<Self, LeaseError> {
        let l: Self = serde_json::from_slice(bytes).map_err(|e| LeaseError::Json(e.to_string()))?;
        if l.format != FORMAT {
            return Err(LeaseError::Format(l.format));
        }
        Ok(l)
    }
}

/// A lease as listed: its content plus the backend's modified time, when reported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observed {
    pub lease: Lease,
    pub server_modified_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liveness {
    Live,
    Stale,
}

/// Live until `last renewal + ttl + skew`. The last renewal is the server's modified time when
/// the backend reports one (immune to a wrong holder clock), else the holder's `renewed_at`.
#[must_use]
pub fn liveness(o: &Observed, now_ms: i64, skew_ms: i64) -> Liveness {
    let renewed = o.server_modified_ms.unwrap_or(o.lease.renewed_at_ms);
    let ttl = o.lease.ttl_ms.max(0);
    if now_ms > renewed.saturating_add(ttl).saturating_add(skew_ms) {
        Liveness::Stale
    } else {
        Liveness::Live
    }
}

/// The lease that wins a race: earliest `(acquired_at, device id)`.
#[must_use]
pub fn winner(live: &[Observed]) -> Option<DeviceId> {
    live.iter()
        .map(|o| (o.lease.acquired_at_ms, o.lease.device))
        .min()
        .map(|(_, d)| d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VersionId;

    fn lease(device: u64, acquired: i64, renewed: i64) -> Lease {
        Lease {
            format: FORMAT,
            device: DeviceId(VersionId::from_parts(device, [1; 10])),
            player: PlayerId(VersionId::from_parts(9, [2; 10])),
            session: SessionId(VersionId::from_parts(7, [3; 10])),
            acquired_at_ms: acquired,
            renewed_at_ms: renewed,
            ttl_ms: DEFAULT_TTL_MS,
        }
    }

    fn seen(l: Lease, server: Option<i64>) -> Observed {
        Observed {
            lease: l,
            server_modified_ms: server,
        }
    }

    #[test]
    fn live_within_ttl_and_skew_stale_after() {
        let o = seen(lease(1, 0, 1_000), None);
        let edge = 1_000 + DEFAULT_TTL_MS + SKEW_MS;
        assert_eq!(liveness(&o, 1_000, SKEW_MS), Liveness::Live);
        assert_eq!(liveness(&o, edge, SKEW_MS), Liveness::Live);
        assert_eq!(liveness(&o, edge + 1, SKEW_MS), Liveness::Stale);
    }

    #[test]
    fn server_time_wins_over_renewed_at() {
        // The holder's clock is an hour behind; the server saw it renew just now.
        let now = 10_000_000;
        let o = seen(lease(1, 0, now - 3_600_000), Some(now - 1_000));
        assert_eq!(liveness(&o, now, SKEW_MS), Liveness::Live);
        // The holder's clock runs ahead, but the server last saw it long ago.
        let o = seen(lease(1, 0, now + 3_600_000), Some(now - 3_600_000));
        assert_eq!(liveness(&o, now, SKEW_MS), Liveness::Stale);
    }

    #[test]
    fn a_negative_ttl_is_not_longer_life() {
        let mut l = lease(1, 0, 0);
        l.ttl_ms = -1;
        assert_eq!(
            liveness(&seen(l, None), SKEW_MS + 1, SKEW_MS),
            Liveness::Stale
        );
    }

    #[test]
    fn winner_is_the_earliest_acquire_then_the_lowest_device() {
        assert_eq!(winner(&[]), None);
        let a = seen(lease(1, 500, 500), None);
        let b = seen(lease(2, 400, 400), None);
        assert_eq!(winner(&[a.clone(), b.clone()]), Some(b.lease.device));
        let tie = seen(lease(2, 500, 500), None);
        assert_eq!(winner(&[tie.clone(), a.clone()]), Some(a.lease.device));
        assert_eq!(winner(&[a.clone(), tie]), Some(a.lease.device));
    }

    #[test]
    fn json_round_trips_and_rejects_unknown_formats_and_fields() {
        let l = lease(1, 2, 3);
        assert_eq!(Lease::from_json(&l.to_json().unwrap()).unwrap(), l);
        let mut v: serde_json::Value = serde_json::from_slice(&l.to_json().unwrap()).unwrap();
        v["format"] = 2.into();
        assert_eq!(
            Lease::from_json(v.to_string().as_bytes()),
            Err(LeaseError::Format(2))
        );
        v["format"] = 1.into();
        v["extra"] = true.into();
        assert!(matches!(
            Lease::from_json(v.to_string().as_bytes()),
            Err(LeaseError::Json(_))
        ));
    }
}
