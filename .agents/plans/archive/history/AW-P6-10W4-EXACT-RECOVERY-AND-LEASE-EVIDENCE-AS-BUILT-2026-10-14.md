# AW-P6-10W4 — exact original recovery and execution-lease evidence (14 October 2026)

## Boundary

Verified original HEAD `200fdbf02d8ca248ac12afae9082252f45c17852`. W4 extends the private Linux-only, non-executing reconciliation PREPARE. Never creates a new executor, durable ledger or schema. Four Runtime primitives, Next/Defer, original Agent/TurnMachine/ToolRegistry/ToolPolicy/TurnJournal remain intact. SQLite v33, JSONL v1, aw 0.15.0 unchanged; both S/T quarantine guards remain required. No access to UX-owned subtree or shared staging.

## Changes

`crates/aw-store/src/cold_effect_reconciliation.rs`: new typed `OriginalRecoveryIdentity` with original kind, primary key, related ID, generation and optional original Process holder. `exact_recovery_evidence` enumerates original hazardous execution leases (`scope_key`, `run_id`, generation, `holder_process_id`), original open recovery hints (`id`, `operation_id`), and original waiting/scheduled/claimed/blocked retry schedules (`operation_id`, claimed client and generation). Uses original v33 read-only database and compares each category's exact count with prior authenticated T original quarantine inspection; bounded 32,768 aggregate entries. Empty keys, absent original Process holder on hazardous lease, invalid generation and count mismatch fail closed. No model, CLI, SDK, RPC, Web or Agent exposure. No setter, retry, activation, Process signal or immutable fact rewrite.

## Native evidence

Two new negative-first native unit tests for exact original keys/generation/Process and denial on original count mismatch, missing Process or invalid generation. W1–W4 focused suite 10/10 PASS. Full Store lib `cargo test --locked -p aw-store --lib -- --test-threads=1`: 146/146 PASS. `cargo check --locked --workspace --all-targets --quiet`: PASS. Strict `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS. Previous W3 tests remain green. No production provider invoice, hostile remote host, independent human-presence assertion, cross-clone exclusion or five-platform certification performed.

## Important limitations and W5

W4 inventory is read after T's authenticated snapshot via a separate READ_ONLY SQLite connection; not proven atomic under a malicious concurrent local SQL writer. W4 does not affirm external charge settlement or authoritative Process incarnation, global WriterEpoch uniqueness, SourceKind identity for every effect, or original Process lifetime outside SQLite. Original cold-restored State cannot run. Next W5 should unify atomic read snapshot and fresh quarantine marker re-verification, preserve exact source site/Actor generations, add negative native concurrent transaction tests, and only then design separate immutable owner-controlled reconciliation review with independent human and global uniqueness requirements. Never reexecute original provider/Tool/OS/claimed Queue effects automatically; never erase unknown or represent unknown billing as free.
