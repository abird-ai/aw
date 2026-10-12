//! Length-prefixed bounded JSON. Reader progress survives a cancelled poll:
//! only `read`, never cancellation-unsafe `read_exact`, is awaited internally.
use crate::{Error, Result};
use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Default)]
pub struct Reader {
    header: [u8; 4],
    header_read: usize,
    body: Vec<u8>,
    body_read: usize,
}
impl Reader {
    pub fn new() -> Self {
        Self::default()
    }
    pub async fn next<R: AsyncRead + Unpin, T: DeserializeOwned>(
        &mut self,
        input: &mut R,
        max: usize,
    ) -> Result<Option<T>> {
        while self.header_read < 4 {
            let count = input.read(&mut self.header[self.header_read..]).await?;
            if count == 0 {
                return if self.header_read == 0 {
                    Ok(None)
                } else {
                    Err(Error::Protocol("truncated frame header"))
                };
            }
            self.header_read += count;
        }
        if self.body.is_empty() {
            let size = u32::from_be_bytes(self.header) as usize;
            if size == 0 || size > max {
                return Err(Error::Protocol("frame exceeds limit or is empty"));
            }
            self.body = vec![0; size];
        }
        while self.body_read < self.body.len() {
            let count = input.read(&mut self.body[self.body_read..]).await?;
            if count == 0 {
                return Err(Error::Protocol("truncated frame body"));
            }
            self.body_read += count;
        }
        let result = serde_json::from_slice(&self.body)
            .map_err(|_| Error::Protocol("invalid JSON or unsupported fields"));
        self.header_read = 0;
        self.body_read = 0;
        self.body.clear();
        result.map(Some)
    }
}
/// Cancelling a write may leave a partial frame. The caller MUST close that
/// connection rather than reuse it or automatically resend the command.
pub async fn write<W: AsyncWrite + Unpin, T: Serialize>(
    output: &mut W,
    value: &T,
    max: usize,
) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Protocol("cannot serialize frame"))?;
    if bytes.is_empty() || bytes.len() > max || bytes.len() > u32::MAX as usize {
        return Err(Error::Protocol("outgoing frame exceeds limit"));
    }
    output
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .await?;
    output.write_all(&bytes).await?;
    output.flush().await?;
    Ok(())
}
