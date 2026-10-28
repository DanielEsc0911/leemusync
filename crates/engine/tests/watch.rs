//! Save-folder watcher on real `notify` over tempdirs (fake data).
//!
//! Timing: positive checks wait with a generous deadline (CI on Windows and macOS is slow);
//! negative checks first prove the watcher is live with a control write, then drain for a while.
#![allow(clippy::unwrap_used)] // tests: a failure here is a failed test

mod common;

use std::path::Path;
use std::time::Duration;

use common::{Pc, SAVE, map};
use lemusync_core::StreamId;
use lemusync_core::id::Hash32;
use lemusync_engine::{MapChoice, SaveWatcher};
use tokio::sync::mpsc::{Receiver, channel};
use tokio::time::{Instant, timeout};

const DEADLINE: Duration = Duration::from_secs(10);
const SETTLE: Duration = Duration::from_millis(1500);

fn s(n: u8) -> StreamId {
    StreamId(Hash32([n; 32]))
}

fn write(p: &Path, body: &[u8]) {
    std::fs::write(p, body).unwrap();
}

/// Keeps writing `file` (watch registration can lag on some OSes) until `want` arrives.
async fn expect(rx: &mut Receiver<StreamId>, want: StreamId, file: &Path) {
    let end = Instant::now() + DEADLINE;
    let mut n = 0u32;
    while Instant::now() < end {
        n += 1;
        write(file, &n.to_le_bytes());
        if let Ok(Some(got)) = timeout(Duration::from_millis(500), rx.recv()).await
            && got == want
        {
            return;
        }
    }
    panic!("no event for {want} within {DEADLINE:?}");
}

/// Everything received until the channel stays silent for `SETTLE`.
async fn drain(rx: &mut Receiver<StreamId>) -> Vec<StreamId> {
    let mut out = Vec::new();
    while let Ok(Some(id)) = timeout(SETTLE, rx.recv()).await {
        out.push(id);
    }
    out
}

#[tokio::test]
async fn a_write_sends_its_stream() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    std::fs::create_dir_all(a.join("sub")).unwrap();
    let (tx, mut rx) = channel(16);
    let _w = SaveWatcher::start(vec![(s(1), a.clone())], tx).unwrap();
    expect(&mut rx, s(1), &a.join("sub/save.bin")).await;
}

#[tokio::test]
async fn a_burst_sends_only_a_few_ids() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    std::fs::create_dir_all(&a).unwrap();
    let (tx, mut rx) = channel(256);
    let _w = SaveWatcher::start(vec![(s(1), a.clone())], tx).unwrap();
    expect(&mut rx, s(1), &a.join("warmup")).await;
    drain(&mut rx).await;
    for i in 0..100u32 {
        write(&a.join("save.bin"), &i.to_le_bytes());
    }
    let got = drain(&mut rx).await;
    assert!(!got.is_empty(), "the burst was seen");
    assert!(got.len() <= 10, "{} ids for 100 writes", got.len());
}

#[tokio::test]
async fn a_folder_created_later_is_picked_up() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("x/y/a");
    let (tx, mut rx) = channel(16);
    let _w = SaveWatcher::start(vec![(s(1), a.clone())], tx).unwrap();
    std::fs::create_dir_all(&a).unwrap();
    expect(&mut rx, s(1), &a.join("save.bin")).await;
}

#[tokio::test]
async fn a_symlink_out_of_the_folder_is_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, a.join("link")).unwrap();
    #[cfg(windows)]
    if std::os::windows::fs::symlink_dir(&outside, a.join("link")).is_err() {
        return; // creating symlinks needs developer mode on Windows
    }
    let (tx, mut rx) = channel(16);
    let _w = SaveWatcher::start(vec![(s(1), a.clone())], tx).unwrap();
    expect(&mut rx, s(1), &a.join("control")).await;
    drain(&mut rx).await;
    for i in 0..5u32 {
        write(&outside.join("x"), &i.to_le_bytes());
    }
    assert_eq!(drain(&mut rx).await, Vec::<StreamId>::new());
}

#[tokio::test]
async fn set_folders_stops_events_from_a_removed_folder() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let (tx, mut rx) = channel(16);
    let mut w = SaveWatcher::start(vec![(s(1), a.clone()), (s(2), b.clone())], tx).unwrap();
    expect(&mut rx, s(1), &a.join("f")).await;
    w.set_folders(vec![(s(2), b.clone())]);
    drain(&mut rx).await;
    for i in 0..5u32 {
        write(&a.join("f"), &i.to_le_bytes());
    }
    // `b` still works, so silence from `a` isn't a dead watcher.
    expect(&mut rx, s(2), &b.join("f")).await;
    assert!(!drain(&mut rx).await.contains(&s(1)));
}

#[tokio::test]
async fn watch_targets_lists_the_mapped_cemu_save_folder() {
    let root = tempfile::tempdir().unwrap();
    let mut a = Pc::new(&root.path().join("a"), true, b"A's save");
    a.engine
        .backend_add_fs(&root.path().join("remote"))
        .unwrap();
    a.engine.init("pass phrase").await.unwrap();
    a.engine.players_add("Ana").await.unwrap();
    map(&mut a, "Ana", "80000001", MapChoice::Ask).await;
    let targets = a.engine.watch_targets().await.unwrap();
    assert_eq!(targets.len(), 1, "{targets:?}");
    assert_eq!(targets[0].1, a.cemu.join(SAVE).join("80000001"));
}
