//! AW-P6-8C: optional framed bidirectional DATA port over the EXISTING
//! host-authorized, journaled PipeProcess. It never spawns a new process,
//! samples devices, starts a provider stream, or creates a fifth primitive.
//! This is a bounded frame codec/owner adapter, not a realtime voice backend.
use crate::{process::PipeProcess, Error, Result};

pub const MAX_FRAME_BODY: usize = 4096;
pub const MAX_FRAME_TOTAL: usize = 48 * 1024;
const HEADER_LEN: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameKind {
    Text,
    Binary,
    Image,
    /// Explicitly disabled: no microphone is owned or activated in 8C.
    Microphone,
    /// Explicitly disabled: no speaker is owned or activated in 8C.
    Speaker,
    /// Explicitly disabled: no camera/live-video source in 8C.
    VideoCapture,
}
impl FrameKind {
    fn wire(self) -> Result<u8> {
        match self {
            Self::Text => Ok(1),
            Self::Binary => Ok(2),
            Self::Image => Ok(3),
            Self::Microphone | Self::Speaker | Self::VideoCapture => Err(Error::Denied(
                "media capture/playback disabled; explicit owner consent and certified device backend required",
            )),
        }
    }
    fn decode(byte: u8) -> Result<Self> {
        match byte {
            1 => Ok(Self::Text),
            2 => Ok(Self::Binary),
            3 => Ok(Self::Image),
            _ => Err(Error::Denied("unknown or disabled media frame kind")),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub kind: FrameKind,
    pub body: Vec<u8>,
}
impl Frame {
    pub fn new(kind: FrameKind, body: Vec<u8>) -> Result<Self> {
        kind.wire()?;
        if body.is_empty() || body.len() > MAX_FRAME_BODY {
            return Err(Error::Limit(
                "media Process frame body must be 1..4096 bytes",
            ));
        }
        if kind == FrameKind::Text && std::str::from_utf8(&body).is_err() {
            return Err(Error::Denied("text Process frame must be UTF-8"));
        }
        Ok(Self { kind, body })
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        Self::new(self.kind, self.body.clone())?;
        let mut out = Vec::with_capacity(HEADER_LEN + self.body.len());
        out.extend_from_slice(&(self.body.len() as u32).to_be_bytes());
        out.push(self.kind.wire()?);
        out.extend_from_slice(&self.body);
        Ok(out)
    }
}

/// Ephemeral owned reference to an already authorized *existing* Process.
/// The original host is responsible for Process intent/effect journal and
/// source/Envelope checks before handing out that process. This adapter does
/// NOT create a new source grant or change ToolPolicy. On any undecidable
/// transport error the owner must reconcile; it cannot automatically retry.
pub struct FramePort<'a> {
    process: &'a mut PipeProcess,
    read_offset: usize,
    consumed_frames: usize,
}
impl<'a> FramePort<'a> {
    pub fn attach(process: &'a mut PipeProcess) -> Self {
        Self {
            process,
            read_offset: 0,
            consumed_frames: 0,
        }
    }
    pub async fn write(&mut self, frame: &Frame) -> Result<()> {
        let body = frame.encode()?;
        // Every PipeProcess write is a distinct externally effectful write.
        // A successful write is not a paid/service or peer ACK receipt.
        self.process.write(&body).await
    }
    /// Read snapshot head ONLY; no ambiguous head/tail concatenation. If more
    /// than 48KiB has been emitted, the original ring capture may have
    /// dropped bytes and exact framing must fail rather than fabricate data.
    pub fn take_available(&mut self) -> Result<Vec<Frame>> {
        let snapshot = self.process.snapshot().stdout;
        if snapshot.read_failed
            || snapshot.truncated
            || snapshot.total_bytes > MAX_FRAME_TOTAL as u64
            || snapshot.head.len() < self.read_offset
        {
            return Err(Error::Conflict("Process framed output lost exact contiguous bytes; inspect original Process receipt"));
        }
        let mut parsed = Vec::new();
        let mut offset = self.read_offset;
        while snapshot.head.len().saturating_sub(offset) >= HEADER_LEN {
            let head = &snapshot.head[offset..offset + HEADER_LEN];
            let len = u32::from_be_bytes(
                head[..4]
                    .try_into()
                    .map_err(|_| Error::Protocol("malformed Process frame header"))?,
            ) as usize;
            if len == 0 || len > MAX_FRAME_BODY {
                return Err(Error::Limit("invalid Process frame size"));
            }
            let kind = FrameKind::decode(head[4])?;
            if snapshot.head.len() - offset < HEADER_LEN + len {
                break;
            }
            let body = snapshot.head[offset + HEADER_LEN..offset + HEADER_LEN + len].to_vec();
            parsed.push(Frame::new(kind, body)?);
            offset += HEADER_LEN + len;
            if self.consumed_frames.saturating_add(parsed.len()) > 1024 {
                return Err(Error::Limit(
                    "too many Process port frames for one bounded transport",
                ));
            }
        }
        // Once stdout reached EOF any remaining incomplete prefix/frame is
        // permanently undecidable. Fail closed rather than silently discard
        // truncated binary data or invent a successful stream settlement.
        if snapshot.closed && offset != snapshot.head.len() {
            return Err(Error::Conflict(
                "Process framed output ended with incomplete frame; inspect original receipt",
            ));
        }
        self.consumed_frames += parsed.len();
        self.read_offset = offset;
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aw_p6_8c_voice_capture_and_playback_are_disabled_for_all_plain_frames() {
        for kind in [
            FrameKind::Microphone,
            FrameKind::Speaker,
            FrameKind::VideoCapture,
        ] {
            assert!(Frame::new(kind, b"not an authorized device sample".to_vec()).is_err());
        }
        assert!(Frame::new(FrameKind::Text, vec![255]).is_err());
        assert!(Frame::new(FrameKind::Binary, Vec::new()).is_err());
        assert!(Frame::new(FrameKind::Binary, vec![42; MAX_FRAME_BODY + 1]).is_err());
        assert_eq!(
            Frame::new(FrameKind::Text, b"data".to_vec())
                .unwrap()
                .encode()
                .unwrap(),
            vec![0, 0, 0, 4, 1, b'd', b'a', b't', b'a']
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn aw_p6_8c_incomplete_or_unapproved_wire_rejected_from_real_process() {
        use crate::process::PipeSpec;
        use std::{collections::BTreeMap, time::Duration};
        let program = std::env::var_os("PATH")
            .into_iter()
            .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
            .map(|path| path.join("cat"))
            .find(|path| path.is_file())
            .expect("test needs cat in PATH");
        let mut process = PipeProcess::spawn(PipeSpec {
            program,
            root: std::env::temp_dir(),
            args: vec![],
            env: BTreeMap::new(),
            stdin_open: true,
        })
        .await
        .unwrap();
        process.write(&[0, 0, 0, 0, 2]).await.unwrap();
        let mut port = FramePort::attach(&mut process);
        let start = std::time::Instant::now();
        let mut denied = false;
        while start.elapsed() < Duration::from_secs(2) {
            if port.take_available().is_err() {
                denied = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            denied,
            "zero-size malformed process wire cannot be accepted"
        );
        // The malformed content is not a frame receipt. Original process
        // ownership stays alive for the caller to inspect/terminate safely.
        process.close_stdin();
        let receipt = process
            .wait(std::future::pending(), Duration::from_secs(3))
            .await;
        assert!(receipt.finished());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn aw_p6_8c_real_existing_process_framed_bidirectional_pipe_keeps_device_disabled() {
        use crate::process::PipeSpec;
        use std::{collections::BTreeMap, time::Duration};
        let root = std::env::temp_dir();
        let program = std::env::var_os("PATH")
            .into_iter()
            .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
            .map(|dir| dir.join("cat"))
            .find(|candidate| candidate.is_file())
            .expect("native test needs an ordinary cat fixture in PATH");
        let spec = PipeSpec {
            program,
            root,
            args: vec![],
            env: BTreeMap::new(),
            stdin_open: true,
        };
        let mut process = PipeProcess::spawn(spec).await.unwrap();
        {
            let mut port = FramePort::attach(&mut process);
            let payload = Frame::new(FrameKind::Binary, vec![0, 0, 255, 4, 2]).unwrap();
            port.write(&payload).await.unwrap();
            let start = std::time::Instant::now();
            let mut seen = Vec::new();
            while seen.is_empty() && start.elapsed() < Duration::from_secs(2) {
                seen = port.take_available().unwrap();
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert_eq!(seen, vec![payload]);
            assert!(
                port.take_available().unwrap().is_empty(),
                "consumed stdout bytes cannot be emitted twice"
            );
            assert!(port
                .write(&Frame {
                    kind: FrameKind::Microphone,
                    body: vec![1]
                })
                .await
                .is_err());
        }
        process.close_stdin();
        let result = process
            .wait(std::future::pending(), Duration::from_secs(3))
            .await;
        assert!(result.finished());
    }
}
