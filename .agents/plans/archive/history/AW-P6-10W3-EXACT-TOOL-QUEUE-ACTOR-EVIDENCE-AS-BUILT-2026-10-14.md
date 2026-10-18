# AW-P6-10W3 — exact original Tool / Queue / Actor evidence (2026-10-14)

## Verified baseline and scope

Starting signed parent `a7fbf781c7d680f9b1131b068f822a142598f786`. Only private Linux original-owner, non-executing inspection in `crates/aw-store/src/cold_effect_reconciliation.rs`. This does NOT implement writable reconciliation dispositions, prove operator presence, mint a writer/Actor grant or activate an S-restored Store. All existing R/S/T quarantine verification remains the mandatory front door; both filesystem and SQLite denial fences still prevent `SessionStore::open`. Four Runtime primitives, two Hooks, original Agent/TurnMachine/ToolPolicy/ToolRegistry/TurnJournal, aw 0.15.0, SQLite v33, JSONL v1 are unchanged. No touch to concurrent `.agents/plans/ux/` or shared index.

## Actual implementation

- `exact_actor_evidence` uses the ORIGINAL audit decoders (`actor_queue::events`, `actor_admission::events`, `actor_control::events`) and the same final per-request-id reduction as original T review, for all Sessions. Enumerates exact per-Session Queue `Pending`, `Claimed`, `Accepted`, Actor Input needs-inspection and Actor Control needs-inspection. Includes original Session, branch, request and WriterEpoch in private typed results. No body/hash/SecretStore reference is returned. Rejects cross-Session audit facts, empty authority identifiers, zero WriterEpoch, changed Session counts, mismatched original T risk counts and over-budget enumeration.
- `exact_tool_evidence` uses original immutable `run_events` ToolStarted/ToolFinished JSON decoded as `aw_agent_core::JournalEntry`; matches original T per-(Run, ToolCall.id) start/finish accounting. Returns exact Run/Tool call IDs and factual multiplicities ONLY for incomplete or mismatched original receipts. Rejects unexpected journal variant, empty identities, changed original T counts and excessive Tool rows. No Tool execution or fabricated terminal success.
- Separate read-only private `prepare_exact_actor_evidence` and `prepare_exact_tool_evidence` again call authentic cold archive/quarantine review FIRST, then inspect original SQLite with READ_ONLY connection. They leave runnable and replay authority false. Existing W1/W2 source and original execution services untouched.

## Negative-first and validation evidence

Four NEW tests reject Queue/Actor count mismatch, missing Session inventory, cross-Session Queue fact, Tool journal missing factual start and unknown/malformed immutable journal payload (two each for Actor and Tool). Total focused W1–W3 tests **8/8 PASS**. `cargo test --locked -p aw-store --lib -- --test-threads=1`: **144/144 PASS**. `cargo check --locked --workspace --all-targets --quiet`: **PASS**. `cargo clippy --locked --workspace --all-targets -- -D warnings`: **PASS**. Tests use local SQLite in-memory fixtures and mock-free pure fact decoding; no real commercial invoice, cross-host live user-presence or OS power-loss evidence.

## Remaining risks and following AW-P6-10W4

W3 exact fact identity does not imply evidence of an external effect's success/failure. Both Tool and Actor data are read in a separate connection after T verification: under a malicious local SQL writer they are not guaranteed one transactionally consistent snapshot. SourceKind/Host binding, Actor generation, Queue claim Process incarnation and optional owner_store_generation are present in original source facts but not yet fully incorporated in the reduced W3 projection; W4 should retain/validate these as appropriate, close per-evidence snapshot-race windows and enumerate original execution leases / Process / retry and recovery hints. Before any immutable reconciliation audit write: require original owner and exact transactional identity checks, separate real initiating-human presence, and cross-clone unique authority. Never turn a local U/V signature into global authority, settle unknown charges at zero, assume absent Tool finish means safe replay, reopen executable Queue entries, or delete cold quarantine markers. No current implementation supports restored State activation.

## Successor prompt

Revalidate latest signed HEAD and original P6 full architecture; continue W4 only from actual source. Work in owned original Store files and non-UX as-builts, keep commits using private exact-file index and expected-parent CAS. Start with negative native tests, preserve immutable original effect evidence, and do not introduce any runnable restored-State path.
