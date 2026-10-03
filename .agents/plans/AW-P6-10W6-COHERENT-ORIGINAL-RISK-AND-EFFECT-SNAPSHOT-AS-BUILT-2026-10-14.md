# AW-P6-10W6 — coherent original risk and effect read snapshot (14 October 2026)

## Boundary

Live source starting at SSH-signed `5106d0cffd20c376a447125ed9497c26cbb8baf5`. Continuing W1–W5, NOT introducing runnable restored-State activation, an executable scheduling/Tool/provider service, fresh Host/Actor/WriterEpoch authority, model-facing surface, second ledger or migration. Four original primitives Runtime/Actor/Message/Envelope, Hook Next/Defer, original Agent/TurnMachine/ToolPolicy/ToolRegistry/immutable TurnJournal, SQLite v33, JSONL v1 and aw 0.15.0 unchanged. No push/tag/release, no UX-owned path access or shared-index mutation.

## Source changes

- In `crates/aw-store/src/cold_quarantine_review.rs`, extracted `inspect_original_cold_risks_on_connection(&Connection, generation)` from the original T implementation. `inspect_quarantined_cold_restore` still verifies original R/S/T archive, secret custody and dual filesystem/SQLite cold quarantine FIRST, then calls the same original inspection function. Original T business logic (all 14 categories, strict bounded queue/Actor audit and immutable Tool journal matching) was retained rather than cloned.
- In `crates/aw-store/src/cold_effect_reconciliation.rs`, W6 pins a single original SQLite DEFERRED read transaction, checks original restore generation, computes T's exact original risk counts INSIDE that transaction, then enumerates W2–W4 exact Run/provider, Tool journal, Queue/Actor and recovery/lease identities on the SAME SQLite snapshot. It reuses the old W5 post-read original `verify_cold_for_inspection` to recheck published source and both quarantine denial guards. The output is private and non-runnable; no SQL writes, no effect settlement, no retry or replay.
- Added a test-only hook after original risk counting and before identity enumeration. W6 real two-connection WAL native regression inserts a new unfinished Run from the second SQLite writer during the first inspector's pinned snapshot. First inspection remains consistent with the old snapshot; the second inspection observes both the new risk count and original Run key. Separate regression checks restore-generation drift and that an ordinarily newly committed Run appears simultaneously in count and identity under one read snapshot.

## Observed actual validation

- Focused W1–W6 original Store native tests **12/12 PASS** including the real two-SQLite-connection WAL writer race.
- Full `cargo test --locked -p aw-store --lib -- --test-threads=1`: **148/148 PASS**.
- `cargo check --locked --workspace --all-targets --quiet`: **PASS**.
- Initial strict Clippy run revealed needless borrows in newly extracted T function; corrected all; `cargo clippy --locked --workspace --all-targets -- -D warnings`: **PASS** on rerun.
- No real commercial provider billing, malicious filesystem replacement, multi-host clone uniqueness, live human-origin independent attestation, external security certification or cross-target release proven. CLI integration, native RPC/QUIC and E3–E5 were not rerun for W6.

## Known boundaries / next W7

The original source and quarantine verification occur before and after the SQLite read snapshot; they are not a single atomic filesystem+SQLite transaction. Race-safe access against malicious OS actors replacing database file descriptors or original secrets is not proven. Original Actor generations/Host SourceKind and Process incarnation must remain requirements at an actual irreversible effect boundary; W6 is inspection-only and grants none of those. Do not elevate an archived U signature/V dry-run, copied identifier or zero outstanding-risk count to independent live-human authorization. Cross-clone global authority and nonce uniqueness, crash-safe transfer and verified first-open no-replay remain OPEN. W7 candidate: harden file identity and post-inspection fencing under adversarial concurrent owner/process/marker mutation, extend typed exact Actor/source provenance without model exposure, and validate with real races before any non-executing owner disposition audit protocol.

## Successor prompt

Verify actual SSH-signed HEAD and inspect this as-built and the frozen October 14 master; respect `.agents/plans/ux/` exclusive ownership and use only an isolated exact-file Git index with expected-parent CAS. Continue only non-executing W7 tests and hardening, preserve unknown paid/Tool/OS/Queue outcomes and original R/S/T dual quarantine. Do not activate restored State, push, tag, or release.
