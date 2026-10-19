//! Linux-only, bounded replay checkpoint for an original, paused PTY.
//!
//! Filtered VT bytes and resize events are replayed through the same VTE
//! pipeline. Unlike a visible-only screen dump, this retains the inactive
//! screen, scrollback, attributes, mouse/input modes and partial escape/UTF-8
//! parser state. Blocked OSC payloads are never retained or persisted.
//! Once the limit is reached, checkpointing permanently fails closed while
//! the original shell continues unaffected.

use crate::{filter::Filter, Error, Replies, Result, SCROLLBACK};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Per-PTY memory budget. A long-running/high-output session may be ineligible
/// for migration; silently dropping historical VT bytes is never allowed.
pub const MAX_CHECKPOINT_BYTES: usize = 8 * 1024 * 1024;
const MAX_EVENTS: usize = 32 * 1024;
const OUTPUT_CHUNK: usize = 64 * 1024;
/// Worst-case JSON encoding of 8 MiB of byte arrays plus bounded resize events.
pub const MAX_CHECKPOINT_WIRE_BYTES: usize = 48 * 1024 * 1024;
const CHECKPOINT_FORMAT: u8 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
enum Event {
    Bytes(Vec<u8>),
    Resize { rows: u16, cols: u16 },
}

/// Bound event count *during* deserialization, before allocating millions of
/// tiny Resize records from an otherwise size-limited input message.
fn bounded_events<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> std::result::Result<Vec<Event>, D::Error> {
    struct Bounded;
    impl<'de> serde::de::Visitor<'de> for Bounded {
        type Value = Vec<Event>;
        fn expecting(&self, format: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            format.write_str("bounded terminal checkpoint event sequence")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut sequence: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut events = Vec::new();
            while let Some(event) = sequence.next_element::<Event>()? {
                if events.len() == MAX_EVENTS {
                    return Err(<A::Error as serde::de::Error>::custom(
                        "terminal checkpoint has too many events",
                    ));
                }
                events.push(event);
            }
            Ok(events)
        }
    }
    decoder.deserialize_seq(Bounded)
}

/// Private, memory-only journal from the instant the PTY was spawned.
/// Only the original driver appends; it must record successful size changes
/// in the same order in which its parser applies them.
pub(crate) struct Journal {
    rows: u16,
    cols: u16,
    events: Vec<Event>,
    bytes: usize,
    complete: bool,
}

impl Journal {
    pub(crate) fn new(rows: u16, cols: u16) -> Self {
        Self {
            rows,
            cols,
            events: Vec::new(),
            bytes: 0,
            complete: true,
        }
    }

    pub(crate) fn output(&mut self, sanitized: &[u8]) {
        if sanitized.is_empty() || !self.complete {
            return;
        }
        if sanitized.len() > MAX_CHECKPOINT_BYTES.saturating_sub(self.bytes) {
            self.invalidate();
            return;
        }
        // Limit per-event allocations without retaining an unbounded number
        // of vectors for each short PTY read.
        if let Some(Event::Bytes(previous)) = self.events.last_mut() {
            if sanitized.len() <= OUTPUT_CHUNK.saturating_sub(previous.len()) {
                previous.extend_from_slice(sanitized);
                self.bytes += sanitized.len();
                return;
            }
        }
        if self.events.len() >= MAX_EVENTS {
            self.invalidate();
            return;
        }
        self.events.push(Event::Bytes(sanitized.to_vec()));
        self.bytes += sanitized.len();
    }

    pub(crate) fn resize(&mut self, rows: u16, cols: u16) {
        if !self.complete {
            return;
        }
        if self.events.len() >= MAX_EVENTS {
            self.invalidate();
            return;
        }
        // Even a successful size change with no subsequent output is part of
        // the parser's private state and must be replayed in order.
        self.events.push(Event::Resize { rows, cols });
    }

    fn invalidate(&mut self) {
        self.complete = false;
        self.bytes = 0;
        self.events.clear(); // do not retain a useless 8 MiB buffer
    }

    pub(crate) fn checkpoint(
        &self,
        id: &str,
        root: &Path,
        owner: &str,
        site: &str,
        epoch: u64,
        filter: &Filter,
    ) -> Result<Checkpoint> {
        if !self.complete {
            return Err(Error::Unavailable(
                "PTY history exceeded bounded checkpoint; original shell retained",
            ));
        }
        Ok(Checkpoint {
            format: CHECKPOINT_FORMAT,
            id: id.into(),
            root: root.to_path_buf(),
            owner: owner.into(),
            site: site.into(),
            epoch,
            rows: self.rows,
            cols: self.cols,
            events: self.events.clone(),
            bytes: self.bytes,
            pending_filter: filter.clone(),
        })
    }
}

/// A replayable state snapshot, *not* a live terminal, child-process lease,
/// standalone authority credential or commit/release acknowledgment.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    format: u8,
    id: String,
    root: PathBuf,
    owner: String,
    site: String,
    epoch: u64,
    rows: u16,
    cols: u16,
    #[serde(deserialize_with = "bounded_events")]
    events: Vec<Event>,
    bytes: usize,
    pending_filter: Filter,
}

impl Checkpoint {
    /// Bounded transfer encoding for the original owner and authenticated
    /// recipient. This envelope is data, never permission to read PTY output
    /// or to disarm the original child lifecycle.
    pub fn to_wire(&self) -> Result<Vec<u8>> {
        let _ = self.replay()?; // refuse incomplete, malformed or over-budget data
        let encoded = serde_json::to_vec(self)
            .map_err(|_| Error::Unavailable("PTY checkpoint cannot be encoded"))?;
        if encoded.len() > MAX_CHECKPOINT_WIRE_BYTES {
            return Err(Error::Unavailable("PTY checkpoint wire size exceeded"));
        }
        Ok(encoded)
    }

    /// Reject oversized data before deserialization; enforce all parser and
    /// filter constraints before any caller can stage the original PTY.
    pub fn from_wire(encoded: &[u8]) -> Result<Self> {
        if encoded.len() > MAX_CHECKPOINT_WIRE_BYTES {
            return Err(Error::Unavailable("PTY checkpoint frame too large"));
        }
        let checkpoint: Self = serde_json::from_slice(encoded)
            .map_err(|_| Error::Unavailable("PTY checkpoint envelope invalid"))?;
        let _ = checkpoint.replay()?;
        Ok(checkpoint)
    }

    pub fn terminal_id(&self) -> &str {
        &self.id
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }

    /// Reconstruct the *exact processing state* within this bounded history.
    /// Device-status replies generated during replay are discarded: the
    /// original PTY driver already sent them and must never send them twice.
    pub fn replay(&self) -> Result<Replayed> {
        if self.format != CHECKPOINT_FORMAT
            || !self.pending_filter.valid_checkpoint()
            || self.id.is_empty()
            || self.id.len() > 128
            || self.owner.is_empty()
            || self.owner.len() > 128
            || self.site.len() > 128
            || !self.root.is_absolute()
            || self.events.len() > MAX_EVENTS
            || self.bytes > MAX_CHECKPOINT_BYTES
        {
            return Err(Error::Unavailable("invalid original PTY checkpoint"));
        }
        crate::valid_size(self.rows, self.cols)?;
        let mut restored = Replayed {
            parser: vt100::Parser::new_with_callbacks(
                self.rows,
                self.cols,
                SCROLLBACK,
                Replies::default(),
            ),
            filter: self.pending_filter.clone(),
        };
        let mut used = 0_usize;
        let mut sanitize_check = Filter::default();
        for event in &self.events {
            match event {
                Event::Bytes(sanitized) => {
                    used = used
                        .checked_add(sanitized.len())
                        .ok_or(Error::Unavailable("PTY replay length overflow"))?;
                    if used > MAX_CHECKPOINT_BYTES {
                        return Err(Error::Unavailable("PTY replay exceeds bound"));
                    }
                    if sanitize_check.push(sanitized) != *sanitized {
                        return Err(Error::Unavailable(
                            "unfiltered PTY checkpoint data rejected",
                        ));
                    }
                    restored.parser.process(sanitized);
                    restored.parser.callbacks_mut().bytes.clear();
                }
                Event::Resize { rows, cols } => {
                    crate::valid_size(*rows, *cols)?;
                    restored.parser.screen_mut().set_size(*rows, *cols);
                }
            }
        }
        if used != self.bytes {
            return Err(Error::Unavailable("PTY replay byte-count mismatch"));
        }
        Ok(restored)
    }
    pub fn original_root(&self) -> &PathBuf {
        &self.root
    }
    pub fn original_owner(&self) -> &str {
        &self.owner
    }
    pub fn original_site(&self) -> &str {
        &self.site
    }
}

/// Receiver-only parser and filter; has no file descriptor, PTY, child,
/// executable, tool or authority to perform shell input.
pub struct Replayed {
    pub(crate) parser: vt100::Parser<Replies>,
    pub(crate) filter: Filter,
}
impl Replayed {
    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }
    /// Read-only inspection. Never return terminal protocol replies when no
    /// live PTY custody exists.
    pub fn process(&mut self, raw: &[u8]) {
        let _ = self.process_filtered(raw, false);
    }
    /// Process *new* bytes only after a separately acknowledged ownership
    /// cutover. Replies are for the original PTY writer, not the checkpoint
    /// recipient's stdout. A mere received checkpoint grants no input right.
    pub fn process_live(&mut self, raw: &[u8]) -> Vec<u8> {
        self.process_filtered(raw, true)
    }
    fn process_filtered(&mut self, raw: &[u8], allow_replies: bool) -> Vec<u8> {
        let sanitized = self.filter.push(raw);
        self.parser.process(&sanitized);
        let replies = std::mem::take(&mut self.parser.callbacks_mut().bytes);
        if allow_replies {
            replies
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal() -> Journal {
        Journal::new(7, 21)
    }
    fn checkpoint(log: &Journal, state: &Replayed) -> Checkpoint {
        log.checkpoint(
            "pty_test",
            &PathBuf::from("/tmp"),
            "synthetic",
            "local",
            4,
            &state.filter,
        )
        .unwrap()
    }
    fn record(log: &mut Journal, state: &mut Replayed, raw: &[u8]) {
        let sanitized = state.filter.push(raw);
        log.output(&sanitized);
        state.parser.process(&sanitized);
        state.parser.callbacks_mut().bytes.clear();
    }
    fn formatted_cell(screen: &vt100::Screen, row: u16, col: u16) -> (String, vt100::Color, bool) {
        let cell = screen.cell(row, col).unwrap();
        (cell.contents().into(), cell.fgcolor(), cell.bold())
    }

    #[test]
    fn replay_keeps_alternate_main_scrollback_attributes_and_input_modes() {
        let mut log = journal();
        let mut original = Replayed {
            parser: vt100::Parser::new_with_callbacks(7, 21, SCROLLBACK, Replies::default()),
            filter: Filter::default(),
        };
        let chunks: &[&[u8]] = &[
            b"main\x1b[31;1m RED\x1b[0m\r\n",
            b"line02\r\nline03\r\nline04\r\nline05\r\nline06\r\nline07\r\nline08\r\n",
            b"line09\r\nline10\r\nline11\r\nline12\r\n",
            b"\x1b[?1h\x1b[?2004h",
            b"\x1b[?1049h\x1b[1;32m ALT",
            b"\x1b[0m\x1b]52;c;hidden clipboard payload\x07",
            b"\x1b[3", // partial SGR must survive cutover
        ];
        for chunk in chunks {
            record(&mut log, &mut original, chunk);
        }
        assert!(original.screen().alternate_screen());
        assert!(original.screen().application_cursor());
        assert!(original.screen().bracketed_paste());

        let checkpoint = checkpoint(&log, &original);
        assert_eq!(checkpoint.epoch(), 4);
        assert_eq!(checkpoint.original_owner(), "synthetic");
        let mut restored = checkpoint.replay().unwrap();
        assert_eq!(restored.screen().contents(), original.screen().contents());
        assert!(restored.screen().alternate_screen());
        assert_eq!(
            restored.screen().application_cursor(),
            original.screen().application_cursor()
        );
        assert_eq!(
            restored.screen().bracketed_paste(),
            original.screen().bracketed_paste()
        );

        // Partial escape completion and normal-screen restoration use the
        // same parser state, not an approximation of the visible cells.
        original.process(b"4m!");
        restored.process(b"4m!");
        assert_eq!(restored.screen().contents(), original.screen().contents());
        assert_eq!(
            formatted_cell(restored.screen(), 0, 0),
            formatted_cell(original.screen(), 0, 0)
        );
        original.process(b"\x1b[?1049l");
        restored.process(b"\x1b[?1049l");
        assert!(!restored.screen().alternate_screen());
        assert_eq!(restored.screen().contents(), original.screen().contents());
        for scroll in [0, 1, 3, 5] {
            original.parser.screen_mut().set_scrollback(scroll);
            restored.parser.screen_mut().set_scrollback(scroll);
            assert_eq!(restored.screen().contents(), original.screen().contents());
        }
    }

    #[test]
    fn replay_preserves_split_utf8_csi_filtered_strings_and_resize_order() {
        let mut log = journal();
        let mut original = Replayed {
            parser: vt100::Parser::new_with_callbacks(7, 21, SCROLLBACK, Replies::default()),
            filter: Filter::default(),
        };
        let text = "界🙂".as_bytes();
        for chunk in [&text[..2], &text[2..]] {
            record(&mut log, &mut original, chunk);
        }
        let suppressed = b"\x1b]52;c;private\x1b";
        record(&mut log, &mut original, suppressed);
        assert!(
            log.events.iter().all(|event| match event {
                Event::Bytes(bytes) => !bytes.windows(b"private".len()).any(|w| w == b"private"),
                Event::Resize { .. } => true,
            }),
            "blocked OSC payload must not be journaled"
        );
        let snapshot = checkpoint(&log, &original);
        let mut restored = snapshot.replay().unwrap();
        assert_eq!(restored.screen().contents(), original.screen().contents());
        // The raw-byte filter is still mid-OSC. A split OSC terminator must
        // not leak private bytes into either screen.
        record(&mut log, &mut original, b"\\after\x1b[31m red");
        restored.process(b"\\after\x1b[31m red");
        assert_eq!(restored.screen().contents(), original.screen().contents());
        assert!(!restored.screen().contents().contains("private"));

        log.resize(4, 9);
        original.parser.screen_mut().set_size(4, 9);
        record(&mut log, &mut original, b"\x1b[2;2HX");
        let from_log = checkpoint(&log, &original).replay().unwrap();
        assert_eq!(from_log.screen().size(), (4, 9));
        assert_eq!(from_log.screen().contents(), original.screen().contents());
        assert_eq!(
            formatted_cell(from_log.screen(), 1, 1),
            formatted_cell(original.screen(), 1, 1)
        );
    }

    #[test]
    fn journal_is_bounded_and_never_silently_drops_history() {
        let mut log = journal();
        log.output(&vec![b'a'; MAX_CHECKPOINT_BYTES]);
        assert_eq!(
            log.checkpoint(
                "pty",
                &PathBuf::from("/tmp"),
                "owner",
                "site",
                1,
                &Filter::default()
            )
            .unwrap()
            .retained_bytes(),
            MAX_CHECKPOINT_BYTES
        );
        log.output(b"overflow");
        assert!(log
            .checkpoint(
                "pty",
                &PathBuf::from("/tmp"),
                "owner",
                "site",
                1,
                &Filter::default()
            )
            .is_err());
        assert_eq!(log.bytes, 0, "overflow releases recorded raw bytes");
        log.output(b"later");
        assert!(log
            .checkpoint(
                "pty",
                &PathBuf::from("/tmp"),
                "owner",
                "site",
                2,
                &Filter::default()
            )
            .is_err());
    }

    #[test]
    fn authenticated_wire_envelope_round_trips_without_blocked_osc_payloads() {
        let mut log = journal();
        let mut original = Replayed {
            parser: vt100::Parser::new_with_callbacks(7, 21, SCROLLBACK, Replies::default()),
            filter: Filter::default(),
        };
        record(
            &mut log,
            &mut original,
            b"MAIN\r\n\x1b[?1049h\x1b[?2004hALT",
        );
        record(
            &mut log,
            &mut original,
            b"\x1b]52;c;SECRET_NOT_AUTHORIZED\x07",
        );
        record(&mut log, &mut original, b"\x1b[31mR\x1b[");
        let state = checkpoint(&log, &original);
        let encoded = state.to_wire().unwrap();
        assert!(encoded.len() < MAX_CHECKPOINT_WIRE_BYTES);
        assert!(!encoded
            .windows(b"SECRET_NOT_AUTHORIZED".len())
            .any(|chunk| chunk == b"SECRET_NOT_AUTHORIZED"));
        let decoded = Checkpoint::from_wire(&encoded).unwrap();
        assert_eq!(decoded.terminal_id(), "pty_test");
        assert_eq!(decoded.original_root(), &PathBuf::from("/tmp"));
        assert_eq!(decoded.epoch(), 4);
        let mut replay = decoded.replay().unwrap();
        assert!(replay.screen().alternate_screen());
        assert!(replay.screen().bracketed_paste());
        replay.process(b"32mGREEN\x1b[?1049l");
        original.process(b"32mGREEN\x1b[?1049l");
        assert_eq!(replay.screen().contents(), original.screen().contents());
    }

    #[test]
    fn wire_decoder_rejects_forged_version_filter_and_event_flood() {
        let log = journal();
        let source = Replayed {
            parser: vt100::Parser::new_with_callbacks(7, 21, SCROLLBACK, Replies::default()),
            filter: Filter::default(),
        };
        let encoded = checkpoint(&log, &source).to_wire().unwrap();
        let mut forged: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        forged["format"] = serde_json::json!(99);
        assert!(Checkpoint::from_wire(&serde_json::to_vec(&forged).unwrap()).is_err());
        forged["format"] = serde_json::json!(CHECKPOINT_FORMAT);
        forged["pending_filter"]["state"] = serde_json::json!(255);
        assert!(Checkpoint::from_wire(&serde_json::to_vec(&forged).unwrap()).is_err());
        let mut flooding = checkpoint(&log, &source);
        flooding.events = vec![Event::Resize { rows: 7, cols: 21 }; MAX_EVENTS + 1];
        let raw = serde_json::to_vec(&flooding).unwrap();
        assert!(raw.len() < MAX_CHECKPOINT_WIRE_BYTES);
        assert!(Checkpoint::from_wire(&raw).is_err());
        assert!(Checkpoint::from_wire(b"not-json").is_err());
    }

    #[test]
    fn excessive_resize_events_fail_closed_without_losing_source_state() {
        let mut journal = journal();
        for _ in 0..=MAX_EVENTS {
            journal.resize(7, 21);
        }
        assert!(!journal.complete);
        assert!(journal.events.is_empty());
        assert!(journal
            .checkpoint(
                "pty",
                &PathBuf::from("/tmp"),
                "owner",
                "local",
                1,
                &Filter::default()
            )
            .is_err());
        journal.output(b"later still visible to source");
        assert!(
            journal.events.is_empty(),
            "overflow never creates a partial non-replayable checkpoint"
        );
    }

    #[test]
    fn replay_of_status_queries_must_never_emit_second_protocol_reply() {
        let mut log = journal();
        log.output(b"test\x1b[6n");
        let parsed = log
            .checkpoint(
                "pty",
                &PathBuf::from("/tmp"),
                "owner",
                "site",
                1,
                &Filter::default(),
            )
            .unwrap()
            .replay()
            .unwrap();
        assert_eq!(parsed.parser.callbacks().bytes, b"");
    }
}
