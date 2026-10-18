# AW-P6-10W2 — exact original provider/run evidence (2026-10-14)

## Boundary and ownership

Started at verified SSH-signed `5cc2f671d38d393d21f0016add67322322beb194` (W1). W2 is only a PRIVATE, Linux, data-only subset of original-owner reconciliation PREPARE, NOT full reconciliation or an activation protocol. Do not touch `.agents/plans/ux/` or another agent's staging; original Runtime/Actor/Message/Envelope, Next/Defer, Agent/TurnMachine/ToolPolicy/ToolRegistry/TurnJournal, SQLite v33 and JSONL v1 remain unchanged. No new executable authority or schema migration.

## Source changes

Extended original private `crates/aw-store/src/cold_effect_reconciliation.rs` with exact key provenance for four original classes: unfinished `runs.id`, uncertain original `invocation_receipts(run_id, ordinal)`, paid/possibly dispatched `llm_attempts.id` bound to `run_id`, and `hazards.run_id`. The new source only reads authenticated R/S/T quarantined v33 State through existing T inspection and private read-only SQLite, NEVER `SessionStore::open`. Original risk counts MUST agree exactly with enumerated bounded original facts. Empty keys and more than 32,768 records fail closed. Return records remain non-model-visible and non-executable; no provider account, payer, private prompt, credential, queue ciphertext or tool parameters exposed. W1 all-14-class aggregate obligations remain unchanged, and incomplete identity coverage remains explicitly unresolved.

## Native regressions and actual gates

Two NEW targeted tests verify exact original keys, all four evidence kinds, reject a mismatched original risk count and reject an empty original key. Four W1+W2 tests passed; `cargo test --locked -p aw-store --lib -- --test-threads=1`: 140/140 PASS. `cargo check --locked --workspace --all-targets --quiet`: PASS. `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS. External paid-provider settlement, full original CLI, E3-E5 portable runtime, real signed QUIC and five-platform release/security matrix were NOT revalidated in W2. Prior as-built results are historical evidence only.

## Open correctness and authority limitations

W2 only validates identity/count consistency on a separate read-only view after the authoritative original T inspection; this does not certify atomic snapshot consistency in the face of a malicious local actor with SQLite write access. It does not cryptographically attest each row, verify external provider charge reality, resolve cost ambiguity, or perform a transactional owner-origin disposition. No source identity enumeration for original Tool journal starts/finishes, Queue Pending/Claimed/Accepted, Actor admissions/Control, Process incarnations, WriterEpoch, retries or recovery hints yet. Do not infer any of those are resolved from absent provider/run rows. No approval, human-origin authentication, globally unique cross-clone grant, authority handoff, replay, or SessionStore activation exists.

## Next AW-P6-10W3

Implement bounded immutable Tool/Queue/Actor original identity enumeration, with full exact source kind/epoch/generation where it exists, and tests for contradiction, tamper, truncation, missing receipts, and copy/race scenarios. Before any audit write or human review action, separately plan `BEGIN IMMEDIATE` re-verification, original-owner checks and immutable append only. Keep quarantine and all execution disabled; independently authenticated originating-human authorization and cross-host uniqueness must precede any future runnable restore work.
