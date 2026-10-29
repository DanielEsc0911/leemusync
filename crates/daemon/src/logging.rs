//! `lemusyncd` logs: `tracing` into `<data>/logs/lemusyncd.log`, size-capped (rotate at
//! 5 MiB, keep 3 old files: `.1` newest … `.3` oldest) and redacted. Every formatted event
//! passes through `lemusync_engine::redact` before it reaches the file (V-SEC-4).
//! Spec: docs/specs/config-and-cli.md (`logs`), docs/specs/reliability.md (Always on).

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use tracing_subscriber::fmt::MakeWriter;

/// Rotate once the file reaches this size.
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;
/// Old files kept after a rotation.
pub const KEEP: usize = 3;

/// The log file name under `<data>/logs/`.
pub const FILE_NAME: &str = "lemusyncd.log";

/// A file that rotates by size: `name` → `name.1` → … → `name.<keep>` (dropped).
pub struct RotatingFile {
    path: PathBuf,
    max: u64,
    keep: usize,
    file: Option<File>,
    size: u64,
}

impl RotatingFile {
    /// Opens (appends to) `path`, creating its folder.
    ///
    /// # Errors
    /// I/O errors creating the folder or the file.
    pub fn open(path: &Path, max: u64, keep: usize) -> io::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = open_append(path)?;
        let size = file.metadata()?.len();
        Ok(Self {
            path: path.to_path_buf(),
            max,
            keep,
            file: Some(file),
            size,
        })
    }

    fn rotated(&self, n: usize) -> PathBuf {
        let mut name = self.path.as_os_str().to_owned();
        name.push(format!(".{n}"));
        PathBuf::from(name)
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file = None;
        let _ = std::fs::remove_file(self.rotated(self.keep));
        for n in (1..self.keep).rev() {
            let from = self.rotated(n);
            if from.exists() {
                std::fs::rename(&from, self.rotated(n + 1))?;
            }
        }
        if self.keep > 0 {
            std::fs::rename(&self.path, self.rotated(1))?;
        } else {
            std::fs::remove_file(&self.path)?;
        }
        self.file = Some(open_append(&self.path)?);
        self.size = 0;
        Ok(())
    }

    /// Writes one whole record; rotates first when it would cross the cap.
    ///
    /// # Errors
    /// I/O errors.
    pub fn write_record(&mut self, record: &[u8]) -> io::Result<()> {
        if self.size > 0 && self.size + record.len() as u64 > self.max {
            self.rotate()?;
        }
        let file = match &mut self.file {
            Some(f) => f,
            None => self.file.insert(open_append(&self.path)?),
        };
        file.write_all(record)?;
        self.size += record.len() as u64;
        Ok(())
    }
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

/// The `MakeWriter` for the subscriber: each event is collected whole, redacted, then written.
#[derive(Clone)]
pub struct RedactedLog {
    file: Arc<Mutex<RotatingFile>>,
}

impl RedactedLog {
    #[must_use]
    pub fn new(file: RotatingFile) -> Self {
        Self {
            file: Arc::new(Mutex::new(file)),
        }
    }
}

/// One event being formatted; written on drop.
pub struct Record {
    buf: Vec<u8>,
    file: Arc<Mutex<RotatingFile>>,
}

impl Write for Record {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for Record {
    fn drop(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        let text = lemusync_engine::redact(&String::from_utf8_lossy(&self.buf));
        let mut file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        // Logging must never take the daemon down; a full disk just loses lines.
        let _ = file.write_record(text.as_bytes());
    }
}

impl<'a> MakeWriter<'a> for RedactedLog {
    type Writer = Record;

    fn make_writer(&'a self) -> Record {
        Record {
            buf: Vec::new(),
            file: Arc::clone(&self.file),
        }
    }
}

/// Installs the global subscriber writing to `<data_dir>/logs/lemusyncd.log`. Returns the
/// file path.
///
/// # Errors
/// I/O errors opening the file; a subscriber was already installed.
pub fn init(data_dir: &Path) -> io::Result<PathBuf> {
    let path = data_dir.join("logs").join(FILE_NAME);
    let log = RedactedLog::new(RotatingFile::open(&path, MAX_BYTES, KEEP)?);
    tracing_subscriber::fmt()
        .with_writer(log)
        .with_ansi(false)
        .with_target(false)
        .with_max_level(tracing::Level::INFO)
        .try_init()
        .map_err(|e| io::Error::other(e.to_string()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_at_the_cap_and_keeps_three() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join(FILE_NAME);
        let mut f = RotatingFile::open(&path, 10, 3).unwrap();
        for i in 0..6u8 {
            f.write_record(&[b'0' + i; 6]).unwrap();
        }
        let read = |p: &Path| std::fs::read(p).unwrap();
        assert_eq!(read(&path), b"555555");
        assert_eq!(read(&f.rotated(1)), b"444444");
        assert_eq!(read(&f.rotated(3)), b"222222");
        assert!(!f.rotated(4).exists());
        // Reopening appends and remembers the size.
        drop(f);
        let mut f = RotatingFile::open(&path, 10, 3).unwrap();
        f.write_record(b"ab").unwrap();
        assert_eq!(read(&path), b"555555ab");
    }

    #[test]
    fn records_are_redacted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let log = RedactedLog::new(RotatingFile::open(&path, MAX_BYTES, KEEP).unwrap());
        let mut w = log.make_writer();
        w.write_all(b"refresh failed: refresh_token=abc123secret\n")
            .unwrap();
        drop(w);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("abc123secret"), "{text}");
        assert!(text.contains("refresh failed"));
    }
}
