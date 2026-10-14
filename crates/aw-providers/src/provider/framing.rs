//! Bounded UTF-8 line framing independent of TCP chunk boundaries.
use crate::{Error, Result};

pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub struct Lines {
    buffer: Vec<u8>,
    after_cr: bool,
    started: bool,
}
impl Lines {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>> {
        let mut output = Vec::new();
        for &b in bytes {
            if self.after_cr {
                self.after_cr = false;
                if b == b'\n' {
                    continue;
                }
            }
            if b == b'\r' || b == b'\n' {
                output.push(self.take_line()?);
                self.after_cr = b == b'\r';
            } else {
                if self.buffer.len() >= MAX_FRAME_BYTES {
                    return Err(Error::Limit("provider line exceeds 1 MiB"));
                }
                self.buffer.push(b);
            }
        }
        Ok(output)
    }
    fn take_line(&mut self) -> Result<String> {
        let data = std::mem::take(&mut self.buffer);
        let mut line =
            String::from_utf8(data).map_err(|_| Error::Protocol("invalid UTF-8 in response"))?;
        if !self.started {
            self.started = true;
            if line.starts_with('\u{feff}') {
                line.drain(..'\u{feff}'.len_utf8());
            }
        }
        Ok(line)
    }
    pub fn finish(&mut self) -> Result<Option<String>> {
        if self.buffer.is_empty() {
            Ok(None)
        } else {
            self.take_line().map(Some)
        }
    }
}

#[derive(Default)]
pub struct Sse {
    data: String,
}
impl Sse {
    pub fn line(&mut self, line: &str) -> Result<Option<String>> {
        if line.is_empty() {
            if self.data.is_empty() {
                return Ok(None);
            }
            let mut data = std::mem::take(&mut self.data);
            data.pop(); // one appended separator, not arbitrary trimming
            return Ok(Some(data));
        }
        if let Some(value) = line.strip_prefix("data:") {
            let value = value.strip_prefix(' ').unwrap_or(value);
            if self.data.len() + value.len() + 1 > MAX_FRAME_BYTES {
                return Err(Error::Limit("SSE event exceeds 1 MiB"));
            }
            self.data.push_str(value);
            self.data.push('\n');
        } else if line == "data" {
            if self.data.len() >= MAX_FRAME_BYTES {
                return Err(Error::Limit("SSE event exceeds 1 MiB"));
            }
            self.data.push('\n');
        }
        // Comments, event:, retry: and id: do not become model output.
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utf8_at_every_split() {
        let data = "data: héllo 🐦\r\n\r\n".as_bytes();
        for split in 0..=data.len() {
            let mut f = Lines::default();
            let mut lines = f.push(&data[..split]).unwrap();
            lines.extend(f.push(&data[split..]).unwrap());
            assert_eq!(lines, vec!["data: héllo 🐦", ""]);
        }
    }
    #[test]
    fn all_line_endings() {
        assert_eq!(
            Lines::default().push(b"a\rb\r\nc\n").unwrap(),
            vec!["a", "b", "c"]
        );
    }
    #[test]
    fn only_leading_bom_removed() {
        assert_eq!(
            Lines::default()
                .push("\u{feff}a\nb\u{feff}\n".as_bytes())
                .unwrap(),
            vec!["a", "b\u{feff}"]
        );
    }
    #[test]
    fn invalid_utf8_rejected() {
        assert!(Lines::default().push(&[0xff, b'\n']).is_err());
    }
    #[test]
    fn bounded_line() {
        assert!(Lines::default()
            .push(&vec![b'x'; MAX_FRAME_BYTES + 1])
            .is_err());
    }
    #[test]
    fn multiline_sse() {
        let mut s = Sse::default();
        assert!(s.line(":ping").unwrap().is_none());
        s.line("data: one").unwrap();
        s.line("data:two").unwrap();
        assert_eq!(s.line("").unwrap(), Some("one\ntwo".into()));
    }
    #[test]
    fn empty_data_and_comments() {
        let mut s = Sse::default();
        s.line("data:").unwrap();
        assert_eq!(s.line("").unwrap(), Some(String::new()));
        assert!(s.line(":x").unwrap().is_none());
    }
    #[test]
    fn unterminated_ndjson_tail() {
        let mut f = Lines::default();
        f.push(b"abc").unwrap();
        assert_eq!(f.finish().unwrap(), Some("abc".into()));
    }
}
