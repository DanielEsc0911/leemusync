//! `lemusync logs [--follow]`: prints the daemon's log (`<data>/logs/lemusyncd.log`, already
//! redacted when written). Reading a file needs no daemon. `--follow` keeps printing new
//! lines, and starts over when the log rotates (it shrinks).

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Duration;

use lemusync_engine::EngineError;

/// How often `--follow` looks for new lines.
const FOLLOW_POLL: Duration = Duration::from_millis(500);

/// Prints the log from byte `from` (from 0 when it shrank) to its end; the new position.
fn print_from(path: &Path, from: u64, out: &mut impl Write) -> std::io::Result<u64> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let start = if from > len { 0 } else { from };
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let end = start + u64::try_from(buf.len()).unwrap_or(0);
    // Control characters other than newlines and tabs are dropped (B2: no terminal escapes).
    let clean: Vec<u8> = buf
        .into_iter()
        .filter(|b| *b == b'\n' || *b == b'\t' || !b.is_ascii_control())
        .collect();
    out.write_all(&clean)?;
    out.flush()?;
    Ok(end)
}

/// # Errors
/// `Usage("no_log:<path>")` when the daemon never wrote a log; I/O errors.
pub async fn run(data_dir: &Path, follow: bool) -> Result<u8, EngineError> {
    let path = data_dir.join("logs").join("lemusyncd.log");
    if !path.is_file() {
        return Err(EngineError::Usage(format!("no_log:{}", path.display())));
    }
    let mut stdout = std::io::stdout();
    let mut pos = print_from(&path, 0, &mut stdout)?;
    if !follow {
        return Ok(0);
    }
    loop {
        tokio::time::sleep(FOLLOW_POLL).await;
        let len = std::fs::metadata(&path).map_or(0, |m| m.len());
        if len != pos {
            pos = print_from(&path, if len < pos { 0 } else { pos }, &mut stdout)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_new_bytes_and_starts_over_after_a_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("l.log");
        std::fs::write(&p, "one\n\x1b[2Jtwo\n").unwrap();
        let mut out = Vec::new();
        let pos = print_from(&p, 0, &mut out).unwrap();
        assert_eq!(out, b"one\n[2Jtwo\n");
        assert_eq!(pos, 12);
        std::fs::write(&p, "one\n\x1b[2Jtwo\nthree\n").unwrap();
        let mut out = Vec::new();
        print_from(&p, pos, &mut out).unwrap();
        assert_eq!(out, b"three\n");
        // Rotated: shorter than where we were.
        std::fs::write(&p, "new\n").unwrap();
        let mut out = Vec::new();
        print_from(&p, 100, &mut out).unwrap();
        assert_eq!(out, b"new\n");
    }
}
