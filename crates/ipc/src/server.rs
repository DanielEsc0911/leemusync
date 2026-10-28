//! The daemon side: bind the socket safely, accept same-user peers, do the handshake.

use std::path::{Path, PathBuf};

use lemusync_api::{Event, Request, Response};

use crate::IpcError;

/// How long a new peer has to send its `Hello`.
#[cfg(unix)]
const HELLO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// A bound IPC socket. The socket file is removed on drop.
pub struct Server {
    #[cfg(unix)]
    listener: tokio::net::UnixListener,
    path: PathBuf,
}

/// One handshaken, same-user client connection.
pub struct Connection {
    #[cfg(unix)]
    stream: tokio::net::UnixStream,
    client: String,
}

impl Server {
    /// Binds `path`: makes its dir `0700` (refusing a symlink or a dir owned by another user),
    /// replaces a stale socket that nothing answers on, and leaves the socket `0600`.
    ///
    /// # Errors
    /// [`IpcError::AlreadyRunning`] when a daemon answers on `path`, [`IpcError::UnsafePath`],
    /// I/O errors; [`IpcError::Unsupported`] on Windows.
    #[cfg(unix)]
    pub fn bind(path: &Path) -> Result<Server, IpcError> {
        use std::fs;
        use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};

        let dir = path
            .parent()
            .ok_or_else(|| IpcError::UnsafePath(path.to_path_buf()))?;
        if fs::symlink_metadata(dir).is_err() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
        }
        let meta = fs::symlink_metadata(dir)?;
        if !meta.is_dir() || !crate::socket::same_user(meta.uid(), crate::socket::own_uid()) {
            return Err(IpcError::UnsafePath(dir.to_path_buf()));
        }
        if meta.mode() & 0o777 != 0o700 {
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        }
        match fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_socket() => {
                if std::os::unix::net::UnixStream::connect(path).is_ok() {
                    return Err(IpcError::AlreadyRunning);
                }
                fs::remove_file(path)?;
            }
            Ok(_) => return Err(IpcError::UnsafePath(path.to_path_buf())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let listener = tokio::net::UnixListener::bind(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Ok(Server {
            listener,
            path: path.to_path_buf(),
        })
    }

    /// See the Unix version.
    ///
    /// # Errors
    /// Always [`IpcError::Unsupported`].
    #[cfg(not(unix))]
    pub fn bind(path: &Path) -> Result<Server, IpcError> {
        let _ = path;
        Err(IpcError::Unsupported)
    }

    /// The socket path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Waits for the next same-user peer with a matching `Hello`. Peers with another uid are
    /// closed without a reply; a different `api_version` gets an error with an upgrade hint.
    /// Both are logged and skipped.
    ///
    /// # Errors
    /// I/O errors from the listener.
    #[cfg(unix)]
    pub async fn accept(&self) -> Result<Connection, IpcError> {
        loop {
            let (stream, _) = self.listener.accept().await?;
            let uid = match stream.peer_cred() {
                Ok(c) => c.uid(),
                Err(e) => {
                    eprintln!("lemusync ipc: peer credentials unavailable: {e}");
                    continue;
                }
            };
            if !crate::socket::same_user(uid, crate::socket::own_uid()) {
                eprintln!("lemusync ipc: closed a connection from uid {uid}");
                continue;
            }
            match tokio::time::timeout(HELLO_TIMEOUT, handshake(stream)).await {
                Ok(Ok(Some(conn))) => return Ok(conn),
                Ok(Ok(None)) => {}
                Ok(Err(e)) => eprintln!("lemusync ipc: handshake failed: {e}"),
                Err(_) => eprintln!("lemusync ipc: handshake timed out"),
            }
        }
    }

    /// See the Unix version.
    ///
    /// # Errors
    /// Always [`IpcError::Unsupported`].
    #[cfg(not(unix))]
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)] // same signature as on Unix
    pub async fn accept(&self) -> Result<Connection, IpcError> {
        Err(IpcError::Unsupported)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// `Some` on a good handshake, `None` when the peer was turned away.
#[cfg(unix)]
async fn handshake(mut stream: tokio::net::UnixStream) -> Result<Option<Connection>, IpcError> {
    use lemusync_api::{API_VERSION, ErrorBody, Hello, HelloReply};

    use crate::frame::{read_frame, write_frame};

    let Some(hello) = read_frame::<Hello, _>(&mut stream).await? else {
        return Ok(None);
    };
    if hello.api_version != API_VERSION {
        let body = ErrorBody {
            code: "api_version_mismatch".into(),
            message: format!(
                "the client speaks API v{}, the daemon speaks v{API_VERSION}; update LemuSync so both match",
                hello.api_version
            ),
            detail: None,
        };
        write_frame(&mut stream, &Response::Error(body)).await?;
        return Ok(None);
    }
    let reply = HelloReply {
        api_version: API_VERSION,
        daemon_version: env!("CARGO_PKG_VERSION").into(),
    };
    write_frame(&mut stream, &reply).await?;
    Ok(Some(Connection {
        stream,
        client: hello.client,
    }))
}

impl Connection {
    /// The client name from its `Hello`.
    #[must_use]
    pub fn client(&self) -> &str {
        &self.client
    }

    /// The next request; `None` once the peer closes. A frame that isn't a valid request gets
    /// a `bad_request` error and ends the connection (`None`).
    #[cfg(unix)]
    pub async fn recv(&mut self) -> Option<Request> {
        match crate::frame::read_frame::<Request, _>(&mut self.stream).await {
            Ok(r) => r,
            Err(e) => {
                let body = lemusync_api::ErrorBody {
                    code: "bad_request".into(),
                    message: e.to_string(),
                    detail: None,
                };
                let _ = self.send(&Response::Error(body)).await;
                None
            }
        }
    }

    /// See the Unix version.
    #[cfg(not(unix))]
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)] // same signature as on Unix
    pub async fn recv(&mut self) -> Option<Request> {
        None
    }

    /// Sends the answer to the last request.
    ///
    /// # Errors
    /// I/O errors; [`IpcError::FrameTooLarge`].
    pub async fn send(&mut self, response: &Response) -> Result<(), IpcError> {
        self.write(response).await
    }

    /// Pushes an event to a subscribed client.
    ///
    /// # Errors
    /// I/O errors; [`IpcError::FrameTooLarge`].
    pub async fn event(&mut self, event: &Event) -> Result<(), IpcError> {
        self.write(event).await
    }

    #[cfg(unix)]
    async fn write<T: serde::Serialize>(&mut self, value: &T) -> Result<(), IpcError> {
        crate::frame::write_frame(&mut self.stream, value).await
    }

    #[cfg(not(unix))]
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)] // same signature as on Unix
    async fn write<T: serde::Serialize>(&mut self, value: &T) -> Result<(), IpcError> {
        let _ = value;
        Err(IpcError::Unsupported)
    }
}
