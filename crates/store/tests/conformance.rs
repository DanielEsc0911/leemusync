//! One suite for every adapter (storage-backends §Conformance suite).
#![allow(clippy::unwrap_used)] // test helper; clippy.toml only exempts `#[test]` fns

use leemusync_store::{Entry, ErrorClass, PutMode, Store};

async fn suite(store: &Store, root: &str) {
    let a = format!("{root}a");
    let b = format!("{root}b");
    store
        .put(&a, b"one".to_vec(), PutMode::CreateOnly)
        .await
        .unwrap();
    let again = store
        .put(&a, b"two".to_vec(), PutMode::CreateOnly)
        .await
        .unwrap_err();
    assert_eq!(
        again.class(),
        ErrorClass::AlreadyExists,
        "create-only never overwrites"
    );
    assert_eq!(store.get(&a).await.unwrap(), b"one");
    assert!(store.stat(&a).await.unwrap().is_some());
    assert!(
        store
            .stat(&format!("{root}missing"))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .get(&format!("{root}missing"))
            .await
            .unwrap_err()
            .class(),
        ErrorClass::NotFound
    );
    store
        .put(&b, b"v1".to_vec(), PutMode::Overwrite)
        .await
        .unwrap();
    store
        .put(&b, b"v2".to_vec(), PutMode::Overwrite)
        .await
        .unwrap();
    assert_eq!(store.get(&b).await.unwrap(), b"v2");
    let mut names: Vec<String> = store
        .list(root)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.path)
        .collect();
    names.sort();
    assert_eq!(names, vec![a.clone(), b.clone()]);
    assert_eq!(
        store.list(&format!("{root}nothing/")).await.unwrap(),
        Vec::<Entry>::new()
    );
    let empty = format!("{root}empty");
    store
        .put(&empty, Vec::new(), PutMode::CreateOnly)
        .await
        .unwrap();
    assert_eq!(store.get(&empty).await.unwrap(), Vec::<u8>::new());
    assert_eq!(store.stat(&empty).await.unwrap().map(|e| e.size), Some(0));
    store.delete(&empty).await.unwrap();
    store.delete(&a).await.unwrap();
    store.delete(&a).await.unwrap();
    assert!(store.stat(&a).await.unwrap().is_none());
    assert!(
        store.get("../escape").await.is_err(),
        "paths are validated before I/O"
    );
    store.delete(&b).await.unwrap();
}

#[tokio::test]
async fn fs_conformance() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::fs(dir.path()).unwrap();
    assert!(store.capabilities().native_create_only);
    suite(&store, "conformance/").await;
    modified_advances(&store, "conformance/").await;
}

/// Lease liveness reads the backend's modified time (sync-model §Leases).
async fn modified_advances(store: &Store, root: &str) {
    let lock = format!("{root}locks/a/b");
    store
        .put(&lock, b"1".to_vec(), PutMode::Overwrite)
        .await
        .unwrap();
    let listed = |store: &Store| {
        let prefix = format!("{root}locks/a/");
        let lock = lock.clone();
        let store = store.clone();
        async move {
            store
                .list(&prefix)
                .await
                .unwrap()
                .into_iter()
                .find(|e| e.path == lock)
                .unwrap()
                .modified
                .unwrap()
        }
    };
    let first = store.stat(&lock).await.unwrap().unwrap().modified.unwrap();
    let first_listed = listed(store).await;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    store
        .put(&lock, b"2".to_vec(), PutMode::Overwrite)
        .await
        .unwrap();
    let second = store.stat(&lock).await.unwrap().unwrap().modified.unwrap();
    assert!(second > first, "stat modified advances on overwrite");
    assert!(listed(store).await > first_listed, "list modified advances");
    store.delete(&lock).await.unwrap();
}

#[tokio::test]
#[ignore = "live Google Drive: run `cargo run -p leemusync-store --example gdrive_login` first"]
async fn gdrive_conformance() {
    use leemusync_store::{GdriveConfig, oauth::OAuthClient};
    use zeroize::Zeroizing;
    let client_id = std::env::var("LEEMUSYNC_TEST_GDRIVE_CLIENT_ID").unwrap();
    let secret = Zeroizing::new(
        keyring::Entry::new("leemusync-test", "gdrive-client-secret")
            .unwrap()
            .get_password()
            .unwrap(),
    );
    let refresh = Zeroizing::new(
        keyring::Entry::new("leemusync-test", "gdrive-refresh")
            .unwrap()
            .get_password()
            .unwrap(),
    );
    let store = Store::gdrive(GdriveConfig {
        root: "/".into(),
        client: OAuthClient {
            client_id,
            client_secret: Some(secret),
        },
        refresh_token: refresh,
    })
    .unwrap();
    assert!(!store.capabilities().native_create_only);
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    suite(&store, &format!("leemusync-conformance/{run}/")).await;
}

#[tokio::test]
#[ignore = "live S3/R2: set LEEMUSYNC_TEST_S3_ENDPOINT, _BUCKET, _KEY_ID and keyring leemusync-test/s3-secret"]
async fn s3_conformance() {
    use leemusync_store::S3Config;
    use zeroize::Zeroizing;
    let var = |name: &str| std::env::var(format!("LEEMUSYNC_TEST_S3_{name}")).unwrap();
    let secret = Zeroizing::new(
        keyring::Entry::new("leemusync-test", "s3-secret")
            .unwrap()
            .get_password()
            .unwrap(),
    );
    let store = Store::s3(&S3Config {
        endpoint: var("ENDPOINT"),
        bucket: var("BUCKET"),
        region: "auto".into(),
        root: "/".into(),
        access_key_id: var("KEY_ID"),
        secret_access_key: secret,
    })
    .unwrap();
    assert!(store.capabilities().native_create_only);
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let root = format!("leemusync-conformance/{run}/");
    suite(&store, &root).await;

    // V-STORE-1: concurrent create-only writes to one name have exactly one winner.
    let race = format!("{root}race");
    let mut tasks = tokio::task::JoinSet::new();
    for i in 0..20u8 {
        let (store, race) = (store.clone(), race.clone());
        tasks.spawn(async move { store.put(&race, vec![i], PutMode::CreateOnly).await });
    }
    let (mut won, mut lost) = (0, 0);
    while let Some(r) = tasks.join_next().await {
        match r.unwrap() {
            Ok(()) => won += 1,
            Err(e) if e.class() == ErrorClass::AlreadyExists => lost += 1,
            Err(e) => panic!("unexpected error: {e}"),
        }
    }
    assert_eq!((won, lost), (1, 19), "exactly one create-only write wins");
    store.delete(&race).await.unwrap();
}
