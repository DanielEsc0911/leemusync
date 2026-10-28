//! Frames: u32 big-endian length, then that many bytes of JSON (architecture §IPC).

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::IpcError;

/// Largest frame body, in bytes.
pub const MAX_FRAME: u32 = 1 << 20;

/// Reads one frame. `Ok(None)` on a clean EOF before the first byte of a frame. The length is
/// checked before anything is allocated.
///
/// # Errors
/// I/O errors (a frame cut short included), [`IpcError::FrameTooLarge`], [`IpcError::Json`].
pub async fn read_frame<T, R>(r: &mut R) -> Result<Option<T>, IpcError>
where
    T: DeserializeOwned,
    R: AsyncRead + Unpin,
{
    let mut len = [0u8; 4];
    if r.read(&mut len[..1]).await? == 0 {
        return Ok(None);
    }
    r.read_exact(&mut len[1..]).await?;
    let len = u32::from_be_bytes(len);
    if len > MAX_FRAME {
        return Err(IpcError::FrameTooLarge(len.into()));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).await?;
    Ok(Some(serde_json::from_slice(&body)?))
}

/// Writes one frame and flushes.
///
/// # Errors
/// I/O errors, [`IpcError::FrameTooLarge`] when the JSON is over `MAX_FRAME`.
pub async fn write_frame<T, W>(w: &mut W, value: &T) -> Result<(), IpcError>
where
    T: Serialize + ?Sized,
    W: AsyncWrite + Unpin,
{
    let body = serde_json::to_vec(value)?;
    let len = u32::try_from(body.len())
        .ok()
        .filter(|l| *l <= MAX_FRAME)
        .ok_or(IpcError::FrameTooLarge(body.len() as u64))?;
    w.write_all(&len.to_be_bytes()).await?;
    w.write_all(&body).await?;
    w.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lemusync_api::Request;

    async fn read(bytes: &[u8]) -> Result<Option<Request>, IpcError> {
        let mut r = bytes;
        read_frame(&mut r).await
    }

    #[tokio::test]
    async fn round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &Request::Sync { dry_run: true })
            .await
            .unwrap();
        write_frame(&mut buf, &Request::Status).await.unwrap();
        let mut r = &buf[..];
        assert_eq!(
            read_frame::<Request, _>(&mut r).await.unwrap(),
            Some(Request::Sync { dry_run: true })
        );
        assert_eq!(
            read_frame::<Request, _>(&mut r).await.unwrap(),
            Some(Request::Status)
        );
        assert_eq!(read_frame::<Request, _>(&mut r).await.unwrap(), None);
    }

    #[tokio::test]
    async fn oversized_length_is_refused_before_the_body() {
        // Only the header is there: a reader that tried to read the body would hit EOF instead.
        let header = (MAX_FRAME + 1).to_be_bytes();
        assert!(
            matches!(read(&header).await, Err(IpcError::FrameTooLarge(n)) if n == u64::from(MAX_FRAME) + 1)
        );
    }

    #[tokio::test]
    async fn truncated_frames_are_errors() {
        let mut bytes = 10u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(b"{\"cmd\"");
        assert!(matches!(read(&bytes).await, Err(IpcError::Io(_))));
        assert!(matches!(read(&[0, 0]).await, Err(IpcError::Io(_))));
    }

    #[tokio::test]
    async fn garbage_json_is_an_error() {
        let mut bytes = 4u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(b"\xff{]x");
        assert!(matches!(read(&bytes).await, Err(IpcError::Json(_))));
        let mut bytes = 2u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(b"{}");
        assert!(matches!(read(&bytes).await, Err(IpcError::Json(_))));
    }
}
