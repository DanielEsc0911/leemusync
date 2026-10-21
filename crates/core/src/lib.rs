//! Domain model and pure sync logic. No I/O, no async.
//! Spec: docs/specs/architecture.md (Layers), docs/specs/sync-model.md.

pub mod account;
pub mod config;
pub mod heads;
pub mod id;
pub mod lease;
pub mod manifest;
pub mod path;
pub mod profile;
pub mod reconcile;

pub use id::{BlobId, DeviceId, Hash32, IdError, PlayerId, SessionId, StreamId, VersionId};
pub use path::{PathError, RelPath};
