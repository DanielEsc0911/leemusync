//! The Unix socket transport end to end, in a temp dir.
#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used)] // tests: a failure here is a failed test

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use lemusync_api::{
    API_VERSION, DoctorReport, ErrorBody, Event, Hello, Request, Response, Status, StatusFacts,
};
use lemusync_ipc::frame::{read_frame, write_frame};
use lemusync_ipc::{Client, IpcError, Server};

fn sock(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("run").join("daemon.sock")
}

fn mode(p: &Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

/// Serves every connection: answers `Status` with a doctor report, `Subscribe` with two events.
fn serve(server: Server) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Ok(mut conn) = server.accept().await {
            tokio::spawn(async move {
                while let Some(req) = conn.recv().await {
                    if req == Request::Subscribe {
                        conn.send(&Response::Subscribed).await.unwrap();
                        let s = Status::from_facts(&StatusFacts::default());
                        conn.event(&Event::StatusChanged(s)).await.unwrap();
                        conn.event(&Event::SessionChanged(None)).await.unwrap();
                        return;
                    }
                    let r = DoctorReport {
                        api_version: API_VERSION,
                        checks: vec![],
                    };
                    conn.send(&Response::Doctor(r)).await.unwrap();
                }
            });
        }
    })
}

#[tokio::test]
async fn bind_sets_modes_and_refuses_a_second_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    let server = Server::bind(&path).unwrap();
    assert_eq!(mode(path.parent().unwrap()), 0o700);
    assert_eq!(mode(&path), 0o600);
    assert!(matches!(Server::bind(&path), Err(IpcError::AlreadyRunning)));
    drop(server);
    assert!(!path.exists());
}

#[tokio::test]
async fn stale_socket_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
    assert!(path.exists());
    let _server = Server::bind(&path).unwrap();
    assert_eq!(mode(&path), 0o600);
}

#[tokio::test]
async fn symlinked_dir_and_non_socket_file_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    std::fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, dir.path().join("run")).unwrap();
    assert!(matches!(
        Server::bind(&sock(&dir)),
        Err(IpcError::UnsafePath(_))
    ));

    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"x").unwrap();
    assert!(matches!(Server::bind(&path), Err(IpcError::UnsafePath(_))));
}

#[tokio::test]
async fn newer_client_gets_an_upgrade_hint() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    let _task = serve(Server::bind(&path).unwrap());
    let mut s = tokio::net::UnixStream::connect(&path).await.unwrap();
    let hello = Hello {
        api_version: API_VERSION + 1,
        client: "future".into(),
    };
    write_frame(&mut s, &hello).await.unwrap();
    let reply: Response = read_frame(&mut s).await.unwrap().unwrap();
    let Response::Error(ErrorBody { code, message, .. }) = reply else {
        panic!("{reply:?}")
    };
    assert_eq!(code, "api_version_mismatch");
    assert!(message.contains("update LemuSync"), "{message}");
    assert!(read_frame::<Response, _>(&mut s).await.unwrap().is_none());
}

#[tokio::test]
async fn call_and_subscribe() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    let _task = serve(Server::bind(&path).unwrap());
    let mut client = Client::connect(&path, "test").await.unwrap();
    assert_eq!(client.daemon().api_version, API_VERSION);
    let r = client.call(Request::Status).await.unwrap();
    assert!(matches!(r, Response::Doctor(_)), "{r:?}");

    let events: Vec<_> = Client::connect(&path, "test")
        .await
        .unwrap()
        .subscribe()
        .await
        .unwrap()
        .collect()
        .await;
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], Ok(Event::StatusChanged(_))));
    assert!(matches!(events[1], Ok(Event::SessionChanged(None))));
}

#[tokio::test]
async fn bad_request_gets_an_error_and_a_close() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    let _task = serve(Server::bind(&path).unwrap());
    let mut s = tokio::net::UnixStream::connect(&path).await.unwrap();
    write_frame(
        &mut s,
        &Hello {
            api_version: API_VERSION,
            client: "t".into(),
        },
    )
    .await
    .unwrap();
    let _: lemusync_api::HelloReply = read_frame(&mut s).await.unwrap().unwrap();
    write_frame(&mut s, &serde_json::json!({"cmd": "rm_rf"}))
        .await
        .unwrap();
    let reply: Response = read_frame(&mut s).await.unwrap().unwrap();
    assert!(
        matches!(reply, Response::Error(ref b) if b.code == "bad_request"),
        "{reply:?}"
    );
    assert!(read_frame::<Response, _>(&mut s).await.unwrap().is_none());
}

#[tokio::test]
async fn connect_with_nothing_listening_is_not_running() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    assert!(matches!(
        Client::connect(&path, "t").await,
        Err(IpcError::NotRunning)
    ));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
    assert!(matches!(
        Client::connect(&path, "t").await,
        Err(IpcError::NotRunning)
    ));
}
