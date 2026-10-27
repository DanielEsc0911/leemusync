//! Spike SP9 (live S3/R2): lease liveness reads the backend's modified time (sync-model §Leases,
//! V-SYNC-1).
#![allow(clippy::unwrap_used)] // test helper; clippy.toml only exempts `#[test]` fns

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lemusync_store::{PutMode, S3Config, Store};
use zeroize::Zeroizing;

fn store() -> Store {
    let var = |name: &str| std::env::var(format!("LEMUSYNC_TEST_S3_{name}")).unwrap();
    let secret = Zeroizing::new(
        keyring::Entry::new("lemusync-test", "s3-secret")
            .unwrap()
            .get_password()
            .unwrap(),
    );
    Store::s3(&S3Config {
        endpoint: var("ENDPOINT"),
        bucket: var("BUCKET"),
        region: "auto".into(),
        root: "/".into(),
        access_key_id: var("KEY_ID"),
        secret_access_key: secret,
    })
    .unwrap()
}

async fn listed(store: &Store, prefix: &str, path: &str) -> SystemTime {
    store
        .list(prefix)
        .await
        .unwrap()
        .into_iter()
        .find(|e| e.path == path)
        .unwrap()
        .modified
        .unwrap()
}

/// Signed seconds from the local clock to the server's time (positive: server ahead).
fn offset(server: SystemTime, local: SystemTime) -> f64 {
    match server.duration_since(local) {
        Ok(d) => d.as_secs_f64(),
        Err(e) => -e.duration().as_secs_f64(),
    }
}

#[tokio::test]
#[ignore = "live S3/R2: set LEMUSYNC_TEST_S3_ENDPOINT, _BUCKET, _KEY_ID and keyring lemusync-test/s3-secret"]
async fn lease_mtime() {
    let store = store();
    let run = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let prefix = format!("lemusync-conformance/{run}/locks/a/");
    let lock = format!("{prefix}b");

    let before = SystemTime::now();
    store
        .put(&lock, b"1".to_vec(), PutMode::Overwrite)
        .await
        .unwrap();
    let after = SystemTime::now();
    let first = store.stat(&lock).await.unwrap().unwrap().modified.unwrap();
    let first_listed = listed(&store, &prefix, &lock).await;
    println!(
        "SP9 offset server-local: {:+.3} s (put took {:.3} s)",
        offset(first, before),
        after.duration_since(before).unwrap().as_secs_f64()
    );

    tokio::time::sleep(Duration::from_millis(1100)).await;
    store
        .put(&lock, b"2".to_vec(), PutMode::Overwrite)
        .await
        .unwrap();
    let second = store.stat(&lock).await.unwrap().unwrap().modified.unwrap();
    let second_listed = listed(&store, &prefix, &lock).await;
    store.delete(&lock).await.unwrap();
    assert!(second > first, "stat LastModified advances on overwrite");
    assert!(second_listed > first_listed, "list LastModified advances");
}
