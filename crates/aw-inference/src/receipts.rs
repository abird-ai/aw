//! A small, private, single-writer device ledger. Commit a reservation BEFORE
//! inference. A started/failed/unknown ID is never automatically dispatched again.
//! It stores hashes and metadata, not prompt text, provider keys or raw outputs.
use crate::{
    grant::Grant,
    private,
    protocol::{self, Request, RECEIPT_LIMIT},
    Error, Result,
};
use aw_agent_core::CompletionMeta;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    path::{Path, PathBuf},
};
const LEDGER_LIMIT: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Started,
    Finished,
    Failed,
    Interrupted,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub request_id: String,
    pub request_digest: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub state: State,
    pub reserved_output_bytes: usize,
    pub observed_output_bytes: usize,
    pub meta: Option<CompletionMeta>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ledger {
    pub version: u32,
    pub grant_id: String,
    pub grant_digest: String,
    pub profile_digest: String,
    pub last_observed_time: u64,
    pub receipts: BTreeMap<String, Receipt>,
}
/// Holds an OS lock for its lifetime. Not an anti-tamper mechanism against the
/// same UID/root; do not put this file in a shared worktree or source bundle.
pub struct ReceiptBook {
    path: PathBuf,
    _lock: File,
    state: Ledger,
    file_digest: String,
}
impl ReceiptBook {
    pub fn inspect(path: &Path) -> Result<Ledger> {
        let bytes = private::read(path, LEDGER_LIMIT)?;
        serde_json::from_slice(&bytes).map_err(|_| Error::Storage("invalid device receipt file"))
    }
    pub fn open(path: &Path, grant: &Grant, profile_digest: &str) -> Result<Self> {
        private::parent(path)?;
        protocol::unhex(profile_digest)?;
        let grant_digest = protocol::digest(grant)?;
        let lock_path = path.with_extension("device-lock");
        let guard = lock(&lock_path)?;
        let bytes = match private::read(path, LEDGER_LIMIT) {
            Ok(bytes) => bytes,
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                let initial = Ledger {
                    version: 1,
                    grant_id: grant.binding.grant_id.clone(),
                    grant_digest: grant_digest.clone(),
                    profile_digest: profile_digest.into(),
                    last_observed_time: grant.issued_at,
                    receipts: BTreeMap::new(),
                };
                let bytes = serde_json::to_vec(&initial)
                    .map_err(|_| Error::Storage("cannot encode initial device ledger"))?;
                private::create(path, &bytes)?;
                bytes
            }
            Err(error) => return Err(error),
        };
        let state: Ledger = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Storage("invalid device receipt file"))?;
        if state.version != 1
            || state.grant_id != grant.binding.grant_id
            || state.grant_digest != grant_digest
            || state.profile_digest != profile_digest
            || state.receipts.len() > RECEIPT_LIMIT
        {
            return Err(Error::Denied(
                "receipt ledger does not match this immutable grant and local profile",
            ));
        }
        let mut reserved = 0usize;
        for (id, receipt) in &state.receipts {
            protocol::unhex(&receipt.request_digest)?;
            if id != &receipt.request_id
                || id.len() > 180
                || receipt.observed_output_bytes > receipt.reserved_output_bytes
                || receipt.reserved_output_bytes != grant.max_output_bytes
            {
                return Err(Error::Storage("invalid device receipt reservation"));
            }
            reserved = reserved
                .checked_add(receipt.reserved_output_bytes)
                .ok_or(Error::Storage("receipt budget overflow"))?;
        }
        if reserved > grant.max_total_output_bytes
            || state.receipts.len() > grant.max_calls as usize
        {
            return Err(Error::Storage("device ledger exceeds its grant"));
        }
        let mut book = Self {
            path: path.into(),
            _lock: guard,
            state,
            file_digest: protocol::digest(&bytes)?,
        };
        // A new process can inspect prior partial work but cannot turn it into a
        // second paid call. No resumed transport/context is guessed from a receipt.
        let mut recovered = book.state.clone();
        let mut changed = false;
        for receipt in recovered.receipts.values_mut() {
            if receipt.state == State::Started {
                receipt.state = State::Interrupted;
                changed = true;
            }
        }
        if changed {
            book.commit(recovered)?;
        }
        Ok(book)
    }
    pub fn summary(&self) -> &Ledger {
        &self.state
    }
    pub fn lookup(&self, id: &str) -> Option<&Receipt> {
        self.state.receipts.get(id)
    }
    pub fn begin(&mut self, request: &Request, grant: &Grant, time: u64) -> Result<()> {
        grant.admit(request, time)?;
        if time < self.state.last_observed_time {
            return Err(Error::Denied(
                "device clock moved backwards; inspect and restart deliberately",
            ));
        }
        let request_digest = request.request_digest()?;
        if let Some(prior) = self.lookup(&request.id) {
            return Err(if prior.request_digest == request_digest {
                Error::Conflict("invocation already reserved/attempted; no replay")
            } else {
                Error::Conflict("invocation ID reused for different context")
            });
        }
        let reserved: usize = self
            .state
            .receipts
            .values()
            .map(|r| r.reserved_output_bytes)
            .sum();
        if self.state.receipts.len() >= grant.max_calls as usize
            || reserved.saturating_add(grant.max_output_bytes) > grant.max_total_output_bytes
        {
            return Err(Error::Limit(
                "device grant call/output reservation budget exhausted",
            ));
        }
        let mut next = self.state.clone();
        next.last_observed_time = time;
        next.receipts.insert(
            request.id.clone(),
            Receipt {
                request_id: request.id.clone(),
                request_digest,
                started_at: time,
                finished_at: None,
                state: State::Started,
                reserved_output_bytes: grant.max_output_bytes,
                observed_output_bytes: 0,
                meta: None,
            },
        );
        self.commit(next)
    }
    pub fn finish(
        &mut self,
        id: &str,
        state: State,
        bytes: usize,
        meta: Option<CompletionMeta>,
        time: u64,
    ) -> Result<()> {
        if state == State::Started {
            return Err(Error::Storage("invalid terminal receipt state"));
        }
        let mut next = self.state.clone();
        let receipt = next
            .receipts
            .get_mut(id)
            .ok_or(Error::Storage("device start receipt missing"))?;
        if receipt.state != State::Started || bytes > receipt.reserved_output_bytes {
            return Err(Error::Storage(
                "device receipt is already settled or exceeds reservation",
            ));
        }
        receipt.state = state;
        receipt.observed_output_bytes = bytes;
        receipt.meta = meta;
        receipt.finished_at = Some(time);
        next.last_observed_time = next.last_observed_time.max(time);
        self.commit(next)
    }
    fn commit(&mut self, next: Ledger) -> Result<()> {
        // Refuse unexpected concurrent same-user file edits rather than silently
        // overwriting them. This check is not an OS security boundary.
        let current = private::read(&self.path, LEDGER_LIMIT)?;
        if protocol::digest(&current)? != self.file_digest {
            return Err(Error::Storage(
                "device receipt file changed outside its owner",
            ));
        }
        let bytes = serde_json::to_vec(&next)
            .map_err(|_| Error::Storage("cannot encode device receipts"))?;
        if bytes.len() > LEDGER_LIMIT {
            return Err(Error::Limit("device receipt file limit"));
        }
        let parent = self
            .path
            .parent()
            .ok_or(Error::Storage("receipt parent missing"))?;
        let temporary = parent.join(format!(".aw-receipt-{}", protocol::random_id()?));
        private::create(&temporary, &bytes)?;
        if let Err(error) = fs::rename(&temporary, &self.path) {
            let _ = fs::remove_file(&temporary);
            return Err(error.into());
        }
        File::open(parent)?.sync_all()?;
        self.file_digest = protocol::digest(&bytes)?;
        self.state = next;
        Ok(())
    }
}
fn lock(path: &Path) -> Result<File> {
    private::parent(path)?;
    let file = aw_platform::open_private_rw_create(path)
        .map_err(|_| Error::Denied("receipt lock requires platform-verified owner-only custody"))?;
    FileExt::try_lock_exclusive(&file)
        .map_err(|_| Error::Conflict("another local device process owns this receipt file"))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    #[test]
    fn reserve_then_reject_replay() {
        let dir = private_dir();
        let g = grant();
        let r = request(&g);
        let path = dir.join("receipts.json");
        let mut b = ReceiptBook::open(&path, &g, &"ab".repeat(32)).unwrap();
        b.begin(&r, &g, g.issued_at).unwrap();
        assert!(b.begin(&r, &g, g.issued_at).is_err());
        drop(b);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn reserve_survives_restart_as_interrupted() {
        let dir = private_dir();
        let g = grant();
        let r = request(&g);
        let path = dir.join("receipts.json");
        {
            let mut b = ReceiptBook::open(&path, &g, &"ab".repeat(32)).unwrap();
            b.begin(&r, &g, g.issued_at).unwrap();
        }
        let mut b = ReceiptBook::open(&path, &g, &"ab".repeat(32)).unwrap();
        assert_eq!(b.lookup(&r.id).unwrap().state, State::Interrupted);
        assert!(b.begin(&r, &g, g.issued_at).is_err());
        drop(b);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn different_profile_cannot_reuse_ledger() {
        let dir = private_dir();
        let g = grant();
        let path = dir.join("receipts.json");
        drop(ReceiptBook::open(&path, &g, &"ab".repeat(32)).unwrap());
        assert!(ReceiptBook::open(&path, &g, &"cd".repeat(32)).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn finite_calls_are_durable() {
        let dir = private_dir();
        let mut g = grant();
        g.max_calls = 1;
        let path = dir.join("receipts.json");
        let mut b = ReceiptBook::open(&path, &g, &"ab".repeat(32)).unwrap();
        let mut r = request(&g);
        b.begin(&r, &g, g.issued_at).unwrap();
        r.context.turn_id = 2;
        r.id = Request::identity(&r.context).unwrap();
        assert!(b.begin(&r, &g, g.issued_at).is_err());
        drop(b);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn lock_is_exclusive() {
        let dir = private_dir();
        let g = grant();
        let path = dir.join("receipts.json");
        let b = ReceiptBook::open(&path, &g, &"ab".repeat(32)).unwrap();
        assert!(ReceiptBook::open(&path, &g, &"ab".repeat(32)).is_err());
        drop(b);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn finished_record_has_no_prompt_or_key() {
        let dir = private_dir();
        let g = grant();
        let path = dir.join("receipts.json");
        let mut r = request(&g);
        r.messages[0].content = "PRIVATE_PROMPT_DO_NOT_PERSIST".into();
        let mut b = ReceiptBook::open(&path, &g, &"ab".repeat(32)).unwrap();
        b.begin(&r, &g, g.issued_at).unwrap();
        b.finish(&r.id, State::Finished, 2, None, g.issued_at + 1)
            .unwrap();
        let data = std::fs::read_to_string(&path).unwrap();
        assert!(!data.contains("PRIVATE_PROMPT"));
        assert!(!data.contains("api_key"));
        drop(b);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn changed_grant_cannot_reset_budget() {
        let dir = private_dir();
        let mut g = grant();
        let path = dir.join("receipts.json");
        drop(ReceiptBook::open(&path, &g, &"ab".repeat(32)).unwrap());
        g.max_calls += 1;
        assert!(ReceiptBook::open(&path, &g, &"ab".repeat(32)).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn double_settlement_rejected() {
        let dir = private_dir();
        let g = grant();
        let r = request(&g);
        let path = dir.join("receipts.json");
        let mut b = ReceiptBook::open(&path, &g, &"ab".repeat(32)).unwrap();
        b.begin(&r, &g, g.issued_at).unwrap();
        b.finish(&r.id, State::Failed, 0, None, g.issued_at + 1)
            .unwrap();
        assert!(b
            .finish(&r.id, State::Finished, 0, None, g.issued_at + 1)
            .is_err());
        drop(b);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn malformed_file_does_not_become_empty_budget() {
        let dir = private_dir();
        let g = grant();
        let path = dir.join("receipts.json");
        private::create(&path, b"bad").unwrap();
        assert!(ReceiptBook::open(&path, &g, &"ab".repeat(32)).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
