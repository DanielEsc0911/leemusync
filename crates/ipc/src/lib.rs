//! Same-user local IPC: framing, handshake, engine client and server.
//! Spec: docs/specs/architecture.md (IPC), docs/specs/security.md (Local IPC).
//!
//! Unix (Linux, macOS): a socket in the user runtime dir, `0600` in a `0700` dir, plus a peer
//! uid check. Windows returns [`IpcError::Unsupported`] until the named pipe lands.

pub mod client;
mod error;
pub mod frame;
pub mod server;
pub mod socket;

pub use client::Client;
pub use error::IpcError;
pub use server::{Connection, Server};
