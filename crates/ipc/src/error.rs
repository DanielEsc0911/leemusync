use std::path::PathBuf;

use lemusync_api::ErrorBody;

/// Everything that can go wrong on the IPC transport.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// Socket or file-system error.
    #[error("ipc i/o: {0}")]
    Io(#[from] std::io::Error),
    /// A frame wasn't valid JSON for the expected type.
    #[error("ipc frame is not valid: {0}")]
    Json(#[from] serde_json::Error),
    /// A frame announced or needed more than `MAX_FRAME` bytes.
    #[error("ipc frame of {0} bytes is over the limit")]
    FrameTooLarge(u64),
    /// `XDG_RUNTIME_DIR` is not set; there is no safe place for the socket.
    #[error("XDG_RUNTIME_DIR is not set")]
    NoRuntimeDir,
    /// The socket dir or path is a symlink, not a dir, or owned by another user.
    #[error("unsafe ipc path: {0}")]
    UnsafePath(PathBuf),
    /// Another daemon answers on the socket.
    #[error("the daemon is already running")]
    AlreadyRunning,
    /// Nothing listens on the socket.
    #[error("the daemon is not running")]
    NotRunning,
    /// The peer closed the connection.
    #[error("the connection closed")]
    Closed,
    /// The daemon refused the request or the handshake.
    #[error("the daemon refused: {}", .0.message)]
    Rejected(ErrorBody),
    /// The peer sent a frame that doesn't fit the protocol here.
    #[error("unexpected ipc frame")]
    Protocol,
    /// This OS has no IPC transport yet.
    #[error("local ipc is not supported on this OS yet")]
    Unsupported,
}
