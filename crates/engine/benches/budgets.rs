//! Engine-side budgets (reliability §Budgets): cold start to ready, warm rescan of 10 000
//! files (an unchanged save folder, dry-run sync), idle RSS of an open engine. Run with `just bench` (release). Not enforced in CI yet.
#![allow(clippy::unwrap_used)] // bench setup: a failure here is a failed run

#[path = "../tests/common/mod.rs"]
mod common;

use std::sync::Arc;

use common::{FileSecrets, Pc, SAVE, env_with, map};
use criterion::{Criterion, criterion_group, criterion_main};
use lemusync_engine::{Engine, FakeProbe, MapChoice, SecretStore, SyncOptions};

const FILES: usize = 10_000;

/// Resident set size of this process in MiB, from `/proc/self/statm` (Linux only).
fn rss_mib() -> Option<f64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: f64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096.0 / 1_048_576.0)
}

fn budgets(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("a");
    let mut pc = Pc::new(&root, true, b"save");
    let many = pc.cemu.join(SAVE).join("80000001/0/many");
    std::fs::create_dir_all(&many).unwrap();
    for i in 0..FILES - 1 {
        std::fs::write(many.join(format!("f{i:05}.bin")), i.to_le_bytes()).unwrap();
    }
    rt.block_on(async {
        pc.engine
            .backend_add_fs(&dir.path().join("remote"))
            .unwrap();
        pc.engine.init("pass phrase").await.unwrap();
        pc.engine.players_add("Ana").await.unwrap();
        map(&mut pc, "Ana", "80000001", MapChoice::Ask).await;
        let r = pc.sync().await;
        assert!(
            r.streams[0].files >= u32::try_from(FILES).unwrap(),
            "all files uploaded"
        );
    });
    let Pc { engine, .. } = pc;
    drop(engine);

    let secrets: Arc<dyn SecretStore> = Arc::new(FileSecrets::under(&root));
    let env = || env_with(&root, Arc::new(FakeProbe::default()), secrets.clone());
    // Ready = state DB open, repo unlocked from the keystore, one remote read answered.
    c.bench_function("cold_start_to_ready", |b| {
        b.iter(|| {
            let mut e = Engine::open(env()).unwrap();
            assert_eq!(rt.block_on(e.players_list()).unwrap().len(), 1);
        });
    });

    let mut engine = Engine::open(env()).unwrap();
    if let Some(mib) = rss_mib() {
        eprintln!("idle RSS of the bench process with an open engine: {mib:.1} MiB");
    }
    c.bench_function("rescan_10k_files_warm", |b| {
        b.iter(|| {
            let r = rt
                .block_on(engine.sync(SyncOptions { dry_run: true }))
                .unwrap();
            assert_eq!(r.streams.len(), 1);
        });
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = budgets
}
criterion_main!(benches);
