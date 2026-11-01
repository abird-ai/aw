//! Transport-independent, bounded UTF-8 line windows for pinned file handles.
//! Zero persistent state, no implicit IO authority or async runtime.

use std::{
    fmt,
    io::{self, BufRead},
};

pub const MAX_OFFSET_SCAN_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_WINDOW_BYTES: usize = crate::MAX_READ_WINDOW_BYTES;

#[derive(Debug)]
pub enum ReadError {
    Io(io::Error),
    Denied(&'static str),
    Limit(&'static str),
    Cancelled,
}
impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "read IO: {error}"),
            Self::Denied(reason) | Self::Limit(reason) => f.write_str(reason),
            Self::Cancelled => f.write_str("read cancelled"),
        }
    }
}
impl std::error::Error for ReadError {}
impl From<io::Error> for ReadError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Validate UTF-8 incrementally without retaining an arbitrary skipped
/// prefix. At most three bytes can straddle two BufRead chunks.
#[derive(Default)]
struct Utf8Probe {
    pending: [u8; 4],
    len: usize,
}
impl Utf8Probe {
    fn check(&mut self, bytes: &[u8]) -> Result<(), ReadError> {
        let mut offset = 0;
        if self.len > 0 {
            let head = self.pending[0];
            let need = match head {
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => return Err(ReadError::Denied("file is not UTF-8 text; use bread")),
            };
            let take = (need - self.len).min(bytes.len());
            self.pending[self.len..self.len + take].copy_from_slice(&bytes[..take]);
            self.len += take;
            offset += take;
            if self.len < need {
                return Ok(());
            }
            std::str::from_utf8(&self.pending[..need])
                .map_err(|_| ReadError::Denied("file is not UTF-8 text; use bread"))?;
            self.len = 0;
        }
        if offset < bytes.len() {
            if let Err(error) = std::str::from_utf8(&bytes[offset..]) {
                if error.error_len().is_some() {
                    return Err(ReadError::Denied("file is not UTF-8 text; use bread"));
                }
                let trailing = &bytes[offset + error.valid_up_to()..];
                if trailing.len() > 3 {
                    return Err(ReadError::Denied("file is not UTF-8 text; use bread"));
                }
                self.pending[..trailing.len()].copy_from_slice(trailing);
                self.len = trailing.len();
            }
        }
        Ok(())
    }
    fn complete(&self) -> Result<(), ReadError> {
        if self.len == 0 {
            Ok(())
        } else {
            Err(ReadError::Denied(
                "file contains incomplete UTF-8; use bread",
            ))
        }
    }
}
fn skip_line(
    reader: &mut impl BufRead,
    scanned: &mut usize,
    stop: &dyn Fn() -> bool,
) -> Result<bool, ReadError> {
    let mut seen = false;
    let mut utf8 = Utf8Probe::default();
    loop {
        if stop() {
            return Err(ReadError::Cancelled);
        }
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            utf8.complete()?;
            return Ok(seen);
        }
        let newline = buf.iter().position(|&b| b == b'\n');
        let take = newline.map_or(buf.len(), |at| at + 1);
        if super::text::has_binary_control(&buf[..take]) {
            return Err(ReadError::Denied("binary file; use bread"));
        }
        utf8.check(&buf[..take])?;
        *scanned = scanned.saturating_add(take);
        if *scanned > MAX_OFFSET_SCAN_BYTES {
            return Err(ReadError::Limit(
                "read offset requires scanning more than 32 MiB",
            ));
        }
        reader.consume(take);
        if newline.is_some() {
            utf8.complete()?;
            return Ok(true);
        }
        seen = true;
    }
}
fn next_line(reader: &mut impl BufRead, cap: usize) -> Result<Option<Vec<u8>>, ReadError> {
    let mut line = Vec::new();
    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            return Ok(if line.is_empty() { None } else { Some(line) });
        }
        let newline = buf.iter().position(|&b| b == b'\n');
        let take = newline.map_or(buf.len(), |at| at + 1);
        if take > cap.saturating_sub(line.len()) {
            return Err(ReadError::Limit("one line exceeds tool byte limit"));
        }
        line.extend_from_slice(&buf[..take]);
        reader.consume(take);
        if newline.is_some() {
            return Ok(Some(line));
        }
    }
}
/// The caller opens a safe pinned regular file and owns cancellation/limits.
pub fn window(
    reader: &mut impl BufRead,
    offset: usize,
    limit: usize,
    cap: usize,
    numbered: bool,
    stop: &dyn Fn() -> bool,
) -> Result<(String, bool), ReadError> {
    if offset == 0 || limit == 0 || cap == 0 || cap > MAX_WINDOW_BYTES {
        return Err(ReadError::Limit("invalid file window"));
    }
    let mut scanned = 0usize;
    for _ in 1..offset {
        if !skip_line(reader, &mut scanned, stop)? {
            return Err(ReadError::Denied("read offset is past the end of file"));
        }
    }
    let mut output = Vec::new();
    let mut truncated = false;
    for row in 0..=limit {
        if stop() {
            return Err(ReadError::Cancelled);
        }
        if row == limit {
            truncated = !reader.fill_buf()?.is_empty();
            break;
        }
        let Some(line) = next_line(reader, cap)? else {
            if row == 0 && offset != 1 {
                return Err(ReadError::Denied("read offset is past the end of file"));
            }
            break;
        };
        let prefix = if numbered {
            format!("{}\t", offset.saturating_add(row)).into_bytes()
        } else {
            Vec::new()
        };
        let body = if numbered {
            let mut end = line.len();
            if end > 0 && line[end - 1] == b'\n' {
                end -= 1;
            }
            if end > 0 && line[end - 1] == b'\r' {
                end -= 1;
            }
            let mut result = line[..end].to_vec();
            result.push(b'\n');
            result
        } else {
            line
        };
        if output
            .len()
            .saturating_add(prefix.len())
            .saturating_add(body.len())
            > cap
        {
            truncated = true;
            break;
        }
        output.extend(prefix);
        output.extend(body);
    }
    let text =
        String::from_utf8(output).map_err(|_| ReadError::Denied("file is not UTF-8 text"))?;
    Ok((text, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn windows_preserve_crlf_and_numbered_windows() {
        let text = b"alpha\r\nbeta\r\ngamma";
        let exact = window(&mut Cursor::new(text), 2, 1, 1024, false, &|| false).unwrap();
        assert_eq!(exact.0, "beta\r\n");
        assert!(exact.1);
        let numbered = window(&mut Cursor::new(text), 2, 2, 1024, true, &|| false).unwrap();
        assert_eq!(numbered.0, "2\tbeta\n3\tgamma\n");
        assert!(!numbered.1);
    }
    #[test]
    fn reject_binary_skipped_prefix_and_past_eof() {
        assert!(matches!(
            window(&mut Cursor::new(b"hi\0\nok\n"), 2, 1, 1024, false, &|| {
                false
            }),
            Err(ReadError::Denied(_))
        ));
        assert!(matches!(
            window(&mut Cursor::new(b"one\n"), 4, 1, 1024, false, &|| false),
            Err(ReadError::Denied(_))
        ));
    }
    #[test]
    fn immediate_cancel_refused() {
        assert!(matches!(
            window(&mut Cursor::new(b"a\n"), 1, 1, 1024, true, &|| true),
            Err(ReadError::Cancelled)
        ));
    }
}

/// Continuation inside one UTF-8 logical line, with the byte offset relative
/// to the beginning of that line (never a renderer/number prefix offset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub line: usize,
    pub byte: usize,
}
#[derive(Debug, Clone, Copy)]
pub struct PageRequest {
    pub line: usize,
    pub lines: usize,
    pub byte: usize,
    pub cap: usize,
    pub numbered: bool,
}
#[derive(Debug)]
pub struct Page {
    pub data: String,
    pub next: Option<Cursor>,
    /// The source returned an I/O failure after useful text was obtained.
    /// Never present such a prefix as a complete successful file read.
    pub io_fault: bool,
}
const STATUS_RESERVE: usize = 160;
fn end_capped(
    reader: &mut impl BufRead,
    data: String,
    mut cursor: Cursor,
    pending: usize,
) -> Result<Page, ReadError> {
    cursor.byte = cursor.byte.saturating_sub(pending);
    match reader.fill_buf() {
        Ok(bytes) if bytes.first() == Some(&b'\n') && data.ends_with('\r') => {
            // A CRLF terminator cannot be split into a cursor inside its
            // two-byte line ending. The next independent read will resume
            // at CR; exclude that byte from this page, too.
            let mut fragment = data;
            fragment.pop();
            cursor.byte = cursor.byte.saturating_sub(1);
            Ok(Page {
                data: fragment,
                next: Some(cursor),
                io_fault: false,
            })
        }
        Ok(bytes) if bytes.is_empty() && pending > 0 => Err(ReadError::Denied(
            "file contains incomplete UTF-8; use bread",
        )),
        Ok(bytes) => Ok(Page {
            data,
            next: (!bytes.is_empty()).then_some(cursor),
            io_fault: false,
        }),
        Err(error) if data.is_empty() => Err(ReadError::Io(error)),
        Err(_) => Ok(Page {
            next: if data.ends_with('\r') {
                None
            } else {
                Some(cursor)
            },
            data,
            io_fault: true,
        }),
    }
}

/// Bounded, model-readable source page. Does not require seek, a path or an
/// OS handle; callers supply already-authorized BufRead and hard byte budget.
/// Long lines are sliced only on UTF-8 boundaries. The source may be
/// incomplete even when a valid prefix has been obtained.
pub fn page(
    reader: &mut impl BufRead,
    line: usize,
    lines: usize,
    byte: usize,
    cap: usize,
    numbered: bool,
    stop: &dyn Fn() -> bool,
) -> Result<Page, ReadError> {
    if line == 0
        || lines == 0
        || lines > 65_536
        || !(STATUS_RESERVE + 16..=MAX_WINDOW_BYTES).contains(&cap)
        || byte > MAX_OFFSET_SCAN_BYTES
    {
        return Err(ReadError::Limit(
            "invalid read line/lines/byte or result ceiling",
        ));
    }
    let mut scanned = 0usize;
    for _ in 1..line {
        if !skip_line(reader, &mut scanned, stop)? {
            return Err(ReadError::Denied("read line is past end of file"));
        }
    }
    // Zero-copy skip to the byte cursor, refusing a cursor inside a UTF-8
    // scalar, a newline or an arbitrarily large skipped source prefix.
    let mut advanced = 0usize;
    let mut last_skipped_cr = false;
    let mut skipped_utf8 = Utf8Probe::default();
    while advanced < byte {
        if stop() {
            return Err(ReadError::Cancelled);
        }
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            return Err(ReadError::Denied("read byte past line EOF"));
        }
        let take = (byte - advanced).min(buf.len());
        if buf[..take].contains(&b'\n') {
            return Err(ReadError::Denied("read byte past logical line boundary"));
        }
        if super::text::has_binary_control(&buf[..take]) {
            return Err(ReadError::Denied("binary file; use bread"));
        }
        skipped_utf8.check(&buf[..take])?;
        last_skipped_cr = buf[take - 1] == b'\r';
        reader.consume(take);
        advanced += take;
    }
    skipped_utf8
        .complete()
        .map_err(|_| ReadError::Denied("byte cursor is inside UTF-8 scalar or invalid text"))?;
    if byte > 0 {
        let next = reader.fill_buf()?;
        if next.first().is_some_and(|v| v & 0xc0 == 0x80) {
            return Err(ReadError::Denied("byte cursor is inside UTF-8 scalar"));
        }
        if last_skipped_cr && next.first() == Some(&b'\n') {
            return Err(ReadError::Denied(
                "byte cursor must not include CR of CRLF line terminator",
            ));
        }
    }
    let mut output = String::new();
    let mut cursor = Cursor { line, byte };
    let mut lines_left = lines;
    let mut pending = Vec::<u8>::with_capacity(4);
    let mut scratch = Vec::<u8>::with_capacity(16 * 1024);
    let mut began = false;
    let mut useful = false;
    let effective = cap - STATUS_RESERVE;
    loop {
        if stop() {
            return Err(ReadError::Cancelled);
        }
        let chunk = match reader.fill_buf() {
            Ok(value) => value,
            Err(error) => {
                if !useful {
                    return Err(ReadError::Io(error));
                }
                cursor.byte = cursor.byte.saturating_sub(pending.len());
                let safe = if output.ends_with('\r') {
                    None
                } else {
                    Some(cursor)
                };
                return Ok(Page {
                    data: output,
                    next: safe,
                    io_fault: true,
                });
            }
        };
        if chunk.is_empty() {
            if !pending.is_empty() {
                return Err(ReadError::Denied("file contains incomplete UTF-8"));
            }
            if !began && line > 1 && byte == 0 {
                return Err(ReadError::Denied("read line is past end of file"));
            }
            return Ok(Page {
                data: output,
                next: None,
                io_fault: false,
            });
        }
        began = true;
        // Start each source line with an optional numbered prefix. Never
        // prefix a continuation that has not successfully emitted text.
        if numbered && (output.is_empty() || output.ends_with('\n')) {
            let prefix = format!("{}\t", cursor.line);
            if output.len() + prefix.len() + 4 > effective {
                if output.is_empty() {
                    return Err(ReadError::Limit("result ceiling cannot fit line prefix"));
                }
                return end_capped(reader, output, cursor, pending.len());
            }
            output.push_str(&prefix);
        }
        if output.len() + 4 >= effective {
            return end_capped(reader, output, cursor, pending.len());
        }
        let max_take = effective - output.len() - 4;
        let len = chunk.len().min(max_take);
        let line_end = chunk[..len].iter().position(|b| *b == b'\n');
        let take = line_end.map_or(len, |i| i + 1);
        if super::text::has_binary_control(&chunk[..take]) {
            return Err(ReadError::Denied("binary file; use bread"));
        }
        scratch.clear();
        scratch.extend_from_slice(&pending);
        scratch.extend_from_slice(&chunk[..take]);
        let valid = match std::str::from_utf8(&scratch) {
            Ok(_) => scratch.len(),
            Err(error) => {
                if error.error_len().is_some() {
                    return Err(ReadError::Denied("file is not UTF-8 text; use bread"));
                }
                error.valid_up_to()
            }
        };
        // Buffer the trailing 1..3 incomplete UTF-8 bytes, preserving
        // source-coordinate semantics across BufRead buffer boundaries.
        let readable = std::str::from_utf8(&scratch[..valid])
            .map_err(|_| ReadError::Denied("file is not UTF-8 text"))?;
        useful |= !readable.is_empty();
        output.push_str(readable);
        pending.clear();
        pending.extend_from_slice(&scratch[valid..]);
        reader.consume(take);
        cursor.byte = cursor
            .byte
            .checked_add(take)
            .ok_or(ReadError::Limit("read cursor overflows"))?;
        if line_end.is_some() {
            if !pending.is_empty() {
                return Err(ReadError::Denied("invalid UTF-8 before newline"));
            }
            cursor.line += 1;
            cursor.byte = 0;
            lines_left -= 1;
            if lines_left == 0 {
                let more = match reader.fill_buf() {
                    Ok(rest) => !rest.is_empty(),
                    Err(_) => {
                        return Ok(Page {
                            data: output,
                            next: Some(cursor),
                            io_fault: true,
                        })
                    }
                };
                return Ok(Page {
                    data: output,
                    next: more.then_some(cursor),
                    io_fault: false,
                });
            }
        }
        // Must never give a continuation cursor pointing *after* an
        // incomplete UTF-8 prefix. This uses the start of the scalar.
        if output.len() + 4 >= effective {
            return end_capped(reader, output, cursor, pending.len());
        }
    }
}

#[cfg(test)]
mod page_tests {
    use super::*;
    use std::io::{Cursor as IoCursor, Read};
    #[test]
    fn long_unicode_line_reconstructs_exact_bytes_from_cursors() {
        let source = format!("{}\nend\n", "🙂é".repeat(9000));
        let mut next = Cursor { line: 1, byte: 0 };
        let mut gathered = String::new();
        for n in 0..500 {
            let mut input = IoCursor::new(source.as_bytes());
            let page = page(&mut input, next.line, 10, next.byte, 1024, false, &|| false).unwrap();
            assert!(!page.io_fault);
            gathered.push_str(&page.data);
            match page.next {
                Some(cursor) => {
                    assert!(
                        cursor.line > next.line || cursor.byte > next.byte,
                        "no cursor progress on iteration {n}"
                    );
                    next = cursor;
                }
                None => {
                    assert_eq!(gathered, source);
                    return;
                }
            }
        }
        panic!("no terminal page");
    }
    #[test]
    fn small_buffer_utf8_split_keeps_original_line_cursors() {
        let source = format!("{}\n", "🙂".repeat(1200));
        let mut next = Cursor { line: 1, byte: 0 };
        let mut gathered = String::new();
        for _ in 0..100 {
            let mut input = io::BufReader::with_capacity(7, IoCursor::new(source.as_bytes()));
            let chunk = page(&mut input, next.line, 2, next.byte, 512, false, &|| false).unwrap();
            gathered.push_str(&chunk.data);
            if let Some(cursor) = chunk.next {
                assert!(cursor.byte > next.byte, "cursor must advance");
                next = cursor;
            } else {
                assert_eq!(gathered, source);
                return;
            }
        }
        panic!("small source buffers did not terminate");
    }
    #[test]
    fn line_limit_and_byte_offsets_are_exact() {
        let source = b"first\nsecond\nthird\n";
        let mut input = IoCursor::new(source);
        let one = page(&mut input, 1, 2, 0, 4096, true, &|| false).unwrap();
        assert_eq!(one.data, "1\tfirst\n2\tsecond\n");
        assert_eq!(one.next, Some(Cursor { line: 3, byte: 0 }));
        let mut input = IoCursor::new(source);
        let two = page(&mut input, 3, 2, 0, 4096, false, &|| false).unwrap();
        assert_eq!(two.data, "third\n");
        assert_eq!(two.next, None);
        let mut input = IoCursor::new(source);
        let partial = page(&mut input, 2, 1, 3, 4096, false, &|| false).unwrap();
        assert_eq!(partial.data, "ond\n");
        assert_eq!(partial.next, Some(Cursor { line: 3, byte: 0 }));
    }
    #[test]
    fn crlf_cursor_excludes_line_terminator_and_preserves_source_bytes() {
        let input = b"abc\r\nnext\r\n";
        let mut page_start = IoCursor::new(input);
        assert!(
            matches!(
                page(&mut page_start, 1, 1, 4, 512, false, &|| false),
                Err(ReadError::Denied(_))
            ),
            "a cursor after CR is inside CRLF ending"
        );
        let mut page_start = IoCursor::new(input);
        let suffix = page(&mut page_start, 1, 2, 3, 512, false, &|| false).unwrap();
        assert_eq!(suffix.data, "\r\nnext\r\n");
        assert_eq!(suffix.next, None);
        let mut begin = IoCursor::new(input);
        let all = page(&mut begin, 1, 2, 0, 512, true, &|| false).unwrap();
        assert_eq!(all.data, "1\tabc\r\n2\tnext\r\n");
    }
    #[test]
    fn crlf_mid_terminator_page_never_emits_invalid_cursor() {
        let body = "x".repeat(512 - STATUS_RESERVE - 5);
        let source = format!("{body}\r\nnext\n");
        let mut reader = IoCursor::new(source.as_bytes());
        let first = page(&mut reader, 1, 4, 0, 512, false, &|| false).unwrap();
        assert!(first.next.is_some());
        let cursor = first.next.unwrap();
        assert_eq!(cursor.line, 1);
        assert!(cursor.byte <= body.len());
        let mut second = IoCursor::new(source.as_bytes());
        let next = page(
            &mut second,
            cursor.line,
            4,
            cursor.byte,
            512,
            false,
            &|| false,
        )
        .unwrap();
        let complete = format!("{}{}", first.data, next.data);
        assert_eq!(complete, source);
    }
    #[test]
    fn skipped_prefix_must_also_be_valid_utf8() {
        let data = [0xff, b'\n', b'o', b'k', b'\n'];
        let mut input = IoCursor::new(data);
        assert!(matches!(
            page(&mut input, 2, 1, 0, 512, false, &|| false),
            Err(ReadError::Denied(_))
        ));
        let data = [0xff, b'o', b'k', b'\n'];
        let mut input = IoCursor::new(data);
        assert!(matches!(
            page(&mut input, 1, 1, 1, 512, false, &|| false),
            Err(ReadError::Denied(_))
        ));
    }
    #[test]
    fn invalid_byte_cursor_and_binary_bytes_fail() {
        let mut input = IoCursor::new("🙂\n".as_bytes());
        assert!(matches!(
            page(&mut input, 1, 1, 1, 512, false, &|| false),
            Err(ReadError::Denied(_))
        ));
        let mut input = IoCursor::new(b"valid\ninvalid\0\n");
        assert!(matches!(
            page(&mut input, 2, 1, 0, 512, false, &|| false),
            Err(ReadError::Denied(_))
        ));
    }
    struct FailsAfterData {
        state: u8,
    }
    impl Read for FailsAfterData {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.state == 0 {
                self.state = 1;
                buf[..5].copy_from_slice(b"hello");
                Ok(5)
            } else {
                Err(io::Error::other("disconnected"))
            }
        }
    }
    #[test]
    fn exact_page_capacity_at_eof_is_not_fake_more() {
        let data = "a".repeat(512 - STATUS_RESERVE - 4);
        let mut input = IoCursor::new(data.as_bytes());
        let result = page(&mut input, 1, 100, 0, 512, false, &|| false).unwrap();
        assert_eq!(result.data, data);
        assert_eq!(
            result.next, None,
            "EOF after complete page has no continuation"
        );
    }
    #[test]
    fn mid_read_fault_returns_useful_prefix_with_error_state() {
        let reader = FailsAfterData { state: 0 };
        let mut buf = io::BufReader::with_capacity(8, reader);
        let result = page(&mut buf, 1, 100, 0, 512, false, &|| false).unwrap();
        assert_eq!(result.data, "hello");
        assert!(result.io_fault);
        assert_eq!(result.next, Some(Cursor { line: 1, byte: 5 }));
    }
}
