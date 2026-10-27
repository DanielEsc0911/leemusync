//! Synthetic network suite (SC5, reliability §SC5 coverage): outages shorter and longer than
//! the retry budget, and a slow link, on an upload sync and a download (fast-forward) sync.
//! Cuts and lost responses at every request live in `faults.rs` and `play.rs`.
#![allow(clippy::unwrap_used)] // tests: a failure here is a failed test
#![allow(clippy::many_single_char_names)] // w, f, n, r, e: the matrix loops

mod common;

use std::time::Duration;

use common::{Pc, SAVE, join, map};
use lemusync_api::{StreamAction, SyncReport};
use lemusync_engine::{EngineError, MapChoice, SyncOptions};
use lemusync_store::Faults;

/// At most this many request indices per scenario (plan: keep the suite fast).
const MAX_CASES: usize = 12;

/// Tries per request under `Store::faulty`: an outage of fewer failures always heals in time.
const ATTEMPTS: u32 = 3;

async fn two_pcs(root: &std::path::Path) -> (Pc, Pc) {
    let remote = root.join("remote");
    let mut a = Pc::new(&root.join("a"), true, b"version one");
    a.engine.backend_add_fs(&remote).unwrap();
    a.engine.init("pass phrase").await.unwrap();
    a.engine.players_add("Ana").await.unwrap();
    map(&mut a, "Ana", "80000001", MapChoice::Ask).await;
    a.sync().await;
    let mut b = Pc::new(&root.join("b"), false, b"b's own");
    join(&mut b, &remote).await;
    b.create_account("80000002");
    map(&mut b, "Ana", "80000002", MapChoice::Ask).await;
    b.sync().await;
    a.sync().await;
    (a, b)
}

fn write_save(pc: &Pc, content: &[u8]) {
    std::fs::write(pc.cemu.join(SAVE).join("80000001/0/game_data.sav"), content).unwrap();
}

/// Request indices 1..=total, or a fixed-seed sample of `MAX_CASES` that keeps the first and
/// the last.
fn sample(total: u32) -> Vec<u32> {
    let mut picked: Vec<u32> = if total as usize <= MAX_CASES {
        (1..=total).collect()
    } else {
        let mut v = vec![1, total];
        let mut x: u64 = 0x5eed_1e55;
        while v.len() < MAX_CASES {
            x = x
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let n = u32::try_from((x >> 33) % u64::from(total)).unwrap() + 1;
            if !v.contains(&n) {
                v.push(n);
            }
        }
        v
    };
    picked.sort_unstable();
    picked
}

/// Two PCs and a counter for fresh save contents.
struct World {
    _root: tempfile::TempDir,
    a: Pc,
    b: Pc,
    k: u32,
}

impl World {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let (a, b) = two_pcs(root.path()).await;
        Self {
            _root: root,
            a,
            b,
            k: 0,
        }
    }

    /// A new save on A; for a download, A uploads it cleanly first. Then the faulty PC
    /// (A to upload, B to download) syncs with `f`. Returns the new content and the result.
    async fn round(
        &mut self,
        upload: bool,
        f: &Faults,
    ) -> (Vec<u8>, Result<SyncReport, EngineError>) {
        self.k += 1;
        let content = format!("network {}", self.k).into_bytes();
        write_save(&self.a, &content);
        if !upload {
            self.a.sync().await;
        }
        let pc = if upload { &mut self.a } else { &mut self.b };
        pc.engine.set_store_faults(Some(f.clone()));
        let r = pc.engine.sync(SyncOptions { dry_run: false }).await;
        pc.engine.set_store_faults(None);
        (content, r)
    }

    /// After a round: both PCs converge on `content`, one head, no conflict.
    async fn converge(&mut self, content: &[u8], case: &str) {
        for _ in 0..2 {
            let r = self.a.sync().await;
            assert!(
                !matches!(
                    r.streams[0].action,
                    StreamAction::UploadThenConflict | StreamAction::Conflict
                ),
                "{case}: false conflict"
            );
            self.b.sync().await;
        }
        assert_eq!(self.a.save("80000001"), content, "{case}");
        assert_eq!(self.b.save("80000002"), content, "{case}");
        assert_eq!(self.a.engine.conflicts().await.unwrap().0, vec![], "{case}");
        let h = self
            .a
            .engine
            .history("Ana", "cemu", "101c9400")
            .await
            .unwrap();
        assert_eq!(h.versions.iter().filter(|v| v.head).count(), 1, "{case}");
    }

    /// Requests a clean round makes in this direction.
    async fn requests(&mut self, upload: bool) -> u32 {
        let f = Faults::default();
        let (content, r) = self.round(upload, &f).await;
        r.unwrap();
        self.converge(&content, "count").await;
        eprintln!("upload={upload}: {} requests", f.requests());
        assert!(
            f.requests() > 3,
            "upload={upload}: the round reached the store"
        );
        f.requests()
    }
}

/// An outage shorter than the retry budget, starting at any request: the sync completes with
/// no error (the M1b test day: a short cable pull).
#[tokio::test]
async fn a_short_outage_at_any_request_completes_without_error() {
    let mut w = World::new().await;
    for upload in [true, false] {
        let total = w.requests(upload).await;
        for n in sample(total) {
            let case = format!("upload={upload} outage of {} at {n}", ATTEMPTS - 1);
            let f = Faults::default();
            f.outage(n, ATTEMPTS - 1);
            let (content, r) = w.round(upload, &f).await;
            let r = r.unwrap_or_else(|e| panic!("{case}: {e}"));
            let want = if upload {
                StreamAction::Upload
            } else {
                StreamAction::FastForward
            };
            assert_eq!(r.streams[0].action, want, "{case}");
            w.converge(&content, &case).await;
        }
    }
}

/// An outage longer than the retry budget: the sync fails with a transient storage error
/// (exit code 6), leaves the old or the new save, and the next run converges.
#[tokio::test]
async fn a_long_outage_at_any_request_fails_transient_then_converges() {
    let mut w = World::new().await;
    for upload in [true, false] {
        let total = w.requests(upload).await;
        let mut prev = if upload {
            w.b.save("80000002")
        } else {
            w.a.save("80000001")
        };
        for n in sample(total) {
            let case = format!("upload={upload} long outage at {n}");
            let f = Faults::default();
            f.outage(n, 10_000);
            let (content, r) = w.round(upload, &f).await;
            let e = r.err().unwrap_or_else(|| panic!("{case}: must fail"));
            assert_eq!(e.exit_code(), 6, "{case}: {e}");
            assert!(
                matches!(&e, EngineError::Store(s) if s.class() == lemusync_store::ErrorClass::Transient),
                "{case}: {e}"
            );
            let now = if upload {
                w.a.save("80000001")
            } else {
                w.b.save("80000002")
            };
            assert!(now == content || now == prev, "{case}: mixed {now:?}");
            w.converge(&content, &case).await;
            prev = content;
        }
    }
}

/// A slow link (a few ms per request): the same result as a fast run.
#[tokio::test]
async fn a_slow_link_gives_the_same_result_as_a_fast_one() {
    let mut w = World::new().await;
    for upload in [true, false] {
        let f = Faults::default();
        let (content, fast) = w.round(upload, &f).await;
        let fast = fast.unwrap();
        w.converge(&content, "fast").await;
        let f = Faults::default();
        f.delay(Duration::from_millis(3));
        let (content, slow) = w.round(upload, &f).await;
        let slow = slow.unwrap();
        assert_eq!(slow.streams[0].action, fast.streams[0].action, "{upload}");
        w.converge(&content, "slow").await;
    }
}
