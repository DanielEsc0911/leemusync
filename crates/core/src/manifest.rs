//! Version manifest, format 1. Spec: docs/specs/sync-model.md (Versions).

use serde::{Deserialize, Serialize};

use crate::{BlobId, DeviceId, PlayerId, RelPath, StreamId, VersionId};

pub const FORMAT: u32 = 1;
const MAX_FILES: usize = 100_000;
const MAX_TEXT: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Version {
    pub format: u32,
    pub stream: StreamId,
    pub id: VersionId,
    pub parents: Vec<VersionId>,
    pub device: DeviceId,
    pub player: PlayerId,
    /// Unix milliseconds.
    pub created_at: u64,
    pub emulator: EmulatorRef,
    /// Sorted by path, no duplicates.
    pub files: Vec<FileEntry>,
    pub meta: Meta,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmulatorRef {
    pub id: String,
    pub version: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    pub path: RelPath,
    pub size: u64,
    /// Unix milliseconds.
    pub mtime: i64,
    pub blob: BlobId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    pub session_secs: Option<u64>,
    pub changed_files: u32,
    pub takeover: bool,
    pub note: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("unsupported manifest format {0}")]
    Format(u32),
    #[error("manifest belongs to another stream")]
    WrongStream,
    #[error("manifest id doesn't match its object name")]
    WrongId,
    #[error("manifest files are unsorted, duplicated or too many")]
    Files,
    #[error("manifest parents are duplicated or include itself")]
    Parents,
    #[error("manifest text field is too long")]
    Text,
    #[error("manifest JSON: {0}")]
    Json(String),
}

impl Version {
    /// # Errors
    /// `ManifestError::Json` if serialisation fails.
    pub fn to_json(&self) -> Result<Vec<u8>, ManifestError> {
        serde_json::to_vec(self).map_err(|e| ManifestError::Json(e.to_string()))
    }

    /// Parses and validates a manifest read from `streams/<stream>/versions/<id>`.
    ///
    /// # Errors
    /// Any `ManifestError`; callers then ignore the version (sync-model, Commit point).
    pub fn from_json(
        bytes: &[u8],
        stream: &StreamId,
        id: &VersionId,
    ) -> Result<Self, ManifestError> {
        let v: Self =
            serde_json::from_slice(bytes).map_err(|e| ManifestError::Json(e.to_string()))?;
        v.validate(stream, id)?;
        Ok(v)
    }

    /// # Errors
    /// The first rule the manifest breaks.
    pub fn validate(&self, stream: &StreamId, id: &VersionId) -> Result<(), ManifestError> {
        if self.format != FORMAT {
            return Err(ManifestError::Format(self.format));
        }
        if self.stream != *stream {
            return Err(ManifestError::WrongStream);
        }
        if self.id != *id {
            return Err(ManifestError::WrongId);
        }
        if self.files.len() > MAX_FILES || self.files.windows(2).any(|w| w[0].path >= w[1].path) {
            return Err(ManifestError::Files);
        }
        let mut parents = self.parents.clone();
        parents.sort_unstable();
        parents.dedup();
        if parents.len() != self.parents.len() || parents.contains(&self.id) {
            return Err(ManifestError::Parents);
        }
        let texts = [
            Some(&self.emulator.id),
            self.emulator.version.as_ref(),
            self.meta.note.as_ref(),
        ];
        if texts.iter().flatten().any(|t| t.len() > MAX_TEXT) {
            return Err(ManifestError::Text);
        }
        Ok(())
    }

    #[must_use]
    pub fn same_content(&self, files: &[FileEntry]) -> bool {
        self.files.len() == files.len()
            && self
                .files
                .iter()
                .zip(files)
                .all(|(a, b)| a.path == b.path && a.blob == b.blob)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlobId, DeviceId, Hash32, PlayerId, RelPath, StreamId, VersionId};

    fn sample() -> Version {
        Version {
            format: FORMAT,
            stream: StreamId(Hash32([1; 32])),
            id: VersionId::from_parts(1_760_000_000_000, [2; 10]),
            parents: vec![],
            device: DeviceId(VersionId::from_parts(1, [3; 10])),
            player: PlayerId(VersionId::from_parts(2, [4; 10])),
            created_at: 1_760_000_000_000,
            emulator: EmulatorRef {
                id: "cemu".into(),
                version: None,
            },
            files: vec![FileEntry {
                path: RelPath::parse("option.sav").unwrap(),
                size: 3,
                mtime: 1_760_000_000_000,
                blob: BlobId(Hash32([5; 32])),
            }],
            meta: Meta {
                session_secs: None,
                changed_files: 1,
                takeover: false,
                note: None,
            },
        }
    }

    #[test]
    fn round_trips_through_json() {
        let v = sample();
        let bytes = v.to_json().unwrap();
        assert_eq!(Version::from_json(&bytes, &v.stream, &v.id).unwrap(), v);
    }

    #[test]
    fn matches_the_golden_file() {
        let json = String::from_utf8(sample().to_json().unwrap()).unwrap();
        assert_eq!(json, include_str!("golden/manifest-v1.json").trim_end());
    }

    #[test]
    #[ignore = "prints the golden file; run once with --nocapture and review"]
    fn print_golden() {
        println!(
            "{}",
            String::from_utf8(sample().to_json().unwrap()).unwrap()
        );
    }

    #[test]
    fn rejects_unknown_fields_and_other_formats() {
        let v = sample();
        let mut json: serde_json::Value = serde_json::from_slice(&v.to_json().unwrap()).unwrap();
        json["extra"] = serde_json::json!(1);
        assert!(Version::from_json(json.to_string().as_bytes(), &v.stream, &v.id).is_err());
        let mut v2 = sample();
        v2.format = 2;
        assert_eq!(v2.validate(&v.stream, &v.id), Err(ManifestError::Format(2)));
    }

    #[test]
    fn rejects_a_manifest_stored_under_another_name() {
        let v = sample();
        let other = VersionId::from_parts(9, [9; 10]);
        assert_eq!(v.validate(&v.stream, &other), Err(ManifestError::WrongId));
        assert_eq!(
            v.validate(&StreamId(Hash32([7; 32])), &v.id),
            Err(ManifestError::WrongStream)
        );
    }

    #[test]
    fn rejects_duplicate_or_unsorted_paths_and_bad_parents() {
        let mut v = sample();
        v.files.push(v.files[0].clone());
        assert_eq!(v.validate(&v.stream, &v.id), Err(ManifestError::Files));
        let mut v = sample();
        v.parents = vec![v.id];
        assert_eq!(v.validate(&v.stream, &v.id), Err(ManifestError::Parents));
    }

    #[test]
    fn same_content_ignores_mtime() {
        let v = sample();
        let mut files = v.files.clone();
        files[0].mtime = 1;
        assert!(v.same_content(&files));
        files[0].blob = BlobId(Hash32([6; 32]));
        assert!(!v.same_content(&files));
    }
}
