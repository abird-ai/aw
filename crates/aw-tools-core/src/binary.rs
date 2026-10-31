//! Bounded binary editing of original-relative ranges. No file-sized
//! allocation is needed when the host supplies source/staging streams.
use anyhow::{anyhow, bail, Result};
use sha2::{Digest as _, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
const CHUNK: usize = 64 * 1024;
#[derive(Clone, Copy, Debug)]
pub struct ByteSplice<'a> {
    pub offset: usize,
    pub end: usize,
    pub data: &'a [u8],
}
pub fn digest_limited(source: &mut impl Read, limit: usize) -> Result<(usize, [u8; 32])> {
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; CHUNK];
    let mut total = 0usize;
    loop {
        let requested = limit
            .saturating_sub(total)
            .saturating_add(1)
            .min(buffer.len());
        let read = source.read(&mut buffer[..requested])?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read)
            .ok_or_else(|| anyhow!("binary input size overflow"))?;
        if total > limit {
            bail!("binary source exceeds {limit} byte limit");
        }
        hasher.update(&buffer[..read]);
    }
    Ok((total, hasher.finalize().into()))
}
/// Recheck the complete pinned source after inspecting candidate no-op
/// spans. Matching only the edited ranges is insufficient if an external
/// writer changed unrelated bytes between the initial digest and return.
pub fn verify_preimage(
    source: &mut (impl Read + Seek),
    expected: (usize, [u8; 32]),
    limit: usize,
) -> Result<()> {
    source.seek(SeekFrom::Start(0))?;
    let current = digest_limited(source, limit)?;
    if current != expected {
        bail!("binary source changed during no-op verification");
    }
    Ok(())
}
pub fn splices_are_unchanged(
    source: &mut (impl Read + Seek),
    splices: &[ByteSplice<'_>],
) -> Result<bool> {
    let mut scratch = [0u8; CHUNK];
    for splice in splices {
        if splice.end.checked_sub(splice.offset) != Some(splice.data.len()) {
            return Ok(false);
        }
        source.seek(SeekFrom::Start(splice.offset as u64))?;
        for chunk in splice.data.chunks(scratch.len()) {
            source.read_exact(&mut scratch[..chunk.len()])?;
            if &scratch[..chunk.len()] != chunk {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
pub fn stream_splice(
    source: &mut impl Read,
    output: &mut impl Write,
    expected: (usize, [u8; 32]),
    splices: &[ByteSplice<'_>],
) -> Result<bool> {
    stream_splice_with_stop(source, output, expected, splices, &|| false)
}
pub fn stream_splice_with_stop(
    source: &mut impl Read,
    output: &mut impl Write,
    expected: (usize, [u8; 32]),
    splices: &[ByteSplice<'_>],
    stop: &dyn Fn() -> bool,
) -> Result<bool> {
    fn copy_chunk(
        source: &mut impl Read,
        out: &mut impl Write,
        mut remaining: usize,
        old_hash: &mut Sha256,
        new_hash: &mut Sha256,
        copy: bool,
        stop: &dyn Fn() -> bool,
    ) -> Result<()> {
        let mut buffer = [0u8; CHUNK];
        while remaining > 0 {
            if stop() {
                bail!("binary edit cancelled");
            }
            let take = remaining.min(buffer.len());
            let len = source.read(&mut buffer[..take])?;
            if len == 0 {
                bail!("source changed during staged edit");
            }
            old_hash.update(&buffer[..len]);
            if copy {
                out.write_all(&buffer[..len])?;
                new_hash.update(&buffer[..len]);
            }
            remaining -= len;
        }
        Ok(())
    }
    if stop() {
        bail!("binary edit cancelled");
    }
    let mut old = Sha256::new();
    let mut new = Sha256::new();
    let (size, digest) = expected;
    let (mut cursor, mut written) = (0usize, 0usize);
    for splice in splices {
        if splice.offset < cursor || splice.end < splice.offset || splice.end > size {
            bail!("invalid or overlapping binary edit ranges");
        }
        let span = splice.offset - cursor;
        copy_chunk(source, output, span, &mut old, &mut new, true, stop)?;
        written = written
            .checked_add(span)
            .ok_or_else(|| anyhow!("output size overflow"))?;
        copy_chunk(
            source,
            output,
            splice.end - splice.offset,
            &mut old,
            &mut new,
            false,
            stop,
        )?;
        if stop() {
            bail!("binary edit cancelled");
        }
        output.write_all(splice.data)?;
        new.update(splice.data);
        written = written
            .checked_add(splice.data.len())
            .ok_or_else(|| anyhow!("output size overflow"))?;
        cursor = splice.end;
    }
    copy_chunk(
        source,
        output,
        size - cursor,
        &mut old,
        &mut new,
        true,
        stop,
    )?;
    written = written
        .checked_add(size - cursor)
        .ok_or_else(|| anyhow!("output size overflow"))?;
    let mut extra = [0u8];
    if source.read(&mut extra)? != 0 || <[u8; 32]>::from(old.finalize()) != digest {
        bail!("binary source changed during staged edit");
    }
    Ok(written != size || <[u8; 32]>::from(new.finalize()) != digest)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn streamed_edits_and_verified_preimage() {
        let original = b"abcdefghijkl";
        let expected = (original.len(), sha2::Sha256::digest(original).into());
        let patches = [
            ByteSplice {
                offset: 2,
                end: 5,
                data: b"XX",
            },
            ByteSplice {
                offset: 8,
                end: 8,
                data: b"!",
            },
        ];
        let mut output = Vec::new();
        assert!(
            stream_splice(&mut Cursor::new(original), &mut output, expected, &patches).unwrap()
        );
        assert_eq!(output, b"abXXfgh!ijkl");
        assert!(stream_splice(
            &mut Cursor::new(b"mutated"),
            &mut Vec::new(),
            expected,
            &patches
        )
        .is_err());
    }
    #[test]
    fn cancelling_a_streamed_splice_never_reports_success() {
        let original = b"abcdef";
        let digest = (original.len(), sha2::Sha256::digest(original).into());
        let splice = [ByteSplice {
            offset: 0,
            end: 0,
            data: b"!",
        }];
        let mut staged = Vec::new();
        assert!(stream_splice_with_stop(
            &mut Cursor::new(original),
            &mut staged,
            digest,
            &splice,
            &|| true
        )
        .is_err());
        assert!(staged.is_empty());
    }
    #[test]
    fn unchanged_splice_does_not_hide_unrelated_source_changes() {
        let original = b"abcdef".to_vec();
        let expected = (original.len(), Sha256::digest(&original).into());
        let edit = [ByteSplice {
            offset: 2,
            end: 4,
            data: b"cd",
        }];
        let mut input = std::io::Cursor::new(original);
        assert!(splices_are_unchanged(&mut input, &edit).unwrap());
        input.get_mut()[0] = b'Z';
        assert!(splices_are_unchanged(&mut input, &edit).unwrap());
        assert!(verify_preimage(&mut input, expected, 64).is_err());
    }
    #[test]
    fn noop_verified() {
        let mut original = Cursor::new(b"abcd");
        assert!(splices_are_unchanged(
            &mut original,
            &[ByteSplice {
                offset: 1,
                end: 3,
                data: b"bc"
            }]
        )
        .unwrap());
        assert_eq!(digest_limited(&mut Cursor::new(b"hello"), 5).unwrap().0, 5);
        assert!(digest_limited(&mut Cursor::new(b"hello!"), 5).is_err());
    }
}
