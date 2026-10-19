//! Domain model and pure sync logic. No I/O, no async.
//! Spec: docs/specs/architecture.md (Layers), docs/specs/sync-model.md.

pub mod id;
pub mod path;

pub use id::{BlobId, DeviceId, Hash32, IdError, PlayerId, StreamId, VersionId};
pub use path::{PathError, RelPath};
