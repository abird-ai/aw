# AW-P6-10W5 — one read-only SQLite reconciliation snapshot (14 October 2026)

## Verified baseline and non-negotiable limits

Signed parent `ba41a1e92094d2a87a0a56813ff7370d25104948`. W5 is a private Linux-only non-executing diagnostic. The four original Runtime primitives, two Hook verbs, original Agent, TurnMachine, ToolPolicy, ToolRegistry and immutable TurnJournal remain unchanged. SQLite stays v33, JSONL stays v1, aw stays 0.15.0. Quarantined State remains inaccessible to original `SessionStore::open`; no Agent/Queue/provider/Tool/OS effect is executed or retried. No new activation, authority or user-facing API. Concurrent `.agents/plans/ux/` ownership and shared staging remain untouched.

## As built

In `crates/aw-store/src/cold_effect_reconciliation.rs`, added private `ColdEffectSnapshot`, `collect_single_snapshot` and `prepare_consistent_cold_effect_snapshot`. The original trusted P/Q/R/S/T quarantine inspection runs first; then one original v33 read-only SQLite DEFERRED transaction pins one SQLite read snapshot and enumerates *all* existing W2 original provider/Run, W3 Actor/Queue, original immutable Tool journal and W4 recovery/lease evidence without separate per-category SQLite snapshots. The transaction checks original `store_meta.restore_generation` against the inspection generation and existing exact original risk category counts. After ending the read, the original source + BOTH cold quarantine guards are reverified through `verify_cold_for_inspection`. Any detected generation/count inconsistency or changed authenticated quarantine is rejected. No second inspection/ledger/engine is created, no SQL write and no original fact is considered settled.

## Actual tests

New native negative regression `w5_snapshot_is_fail_closed_on_generation_and_count_drift` verifies an empty valid snapshot stays non-runnable, rejects newly inserted original Run facts with stale inspection count, and rejects restored generation drift. W1–W5 focused 11/11 PASS. Entire native `cargo test --locked -p aw-store --lib -- --test-threads=1`: **147/147 PASS**. `cargo check --locked --workspace --all-targets --quiet`: PASS. Strict `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS. This suite does not certify real external commercial billing, independent human presence, cross-host cloned State authority or crash/power-loss durability. Full CLI, RPC/QUIC and E3–E5 were not rerun for W5.

## Unresolved correctness/security work

An SQLite read snapshot is consistent **within the opened DB**; T inspection and the first filesystem/SQLite verification happen before that transaction, and a second filesystem check after it. This is not an indivisible OS-filesystem-and-SQLite snapshot under hostile writers; in-place mutation preserving counts may escape the counts-only comparison if not caught by original verifier. Further work should unify T's inspection counts with the exact original facts from the SAME snapshot, add real concurrent WAL writer native tests and enforce file-descriptor and original Process incarnation fences for genuinely hostile local writes. W5 keeps source-/Actor-generation evidence only to the extent already present in original W2–W4 projections; it does not authenticate a new writer or live human. An independently authenticated originating-human grant, external clone uniqueness/nonce authority, transactional reconciliation and first-open no-replay proof remain prerequisites to ANY future runnable restore.

## Suggested W6

Prove a single coherent original v33 inspection + exact evidence snapshot rather than compare with T's earlier one; add competing SQLite writer and quarantine-mutator tests, close stale file handle/clone issues, and only then consider owner-only non-executing reviewed dispositions. No activation and no automatic replay.
