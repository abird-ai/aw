# Phase 34 — local SQLite coordination leases and adversarial WAL/CAS/crash recovery

Phase 34 implements and proves the coordination layer that must exist **before** `aw` may consider allowing more than one trusted local process to open the same SQLite store.

The production boundary remains unchanged:

> **`owner.lock` is still authoritative. Ordinary `SessionStore::open` still permits one local SQLite-owning process.**

Phase 34 deliberately does **not** expose a public/direct multi-process store mode. Independent WAL connections exist only in the Rust adversarial test harness so the coordination/fencing rules can be proven without weakening the shipping boundary first.

## 1. Why this phase exists

SQLite WAL can serialize database writers, but SQLite locking alone does not protect the higher-level invariants that matter to `aw`:

- a provider/tool effect may already have happened when a process dies;
- a stale process must not resume after restore;
- migration/backup/restore must not race admitted writers/effects;
- request IDs must remain exact/idempotent under contention;
- branch head + revision CAS must remain authoritative;
- Workspace writer epoch fencing must remain authoritative;
- completed paid results must survive a lost attachment CAS;
- `SQLITE_BUSY` must never become a reason to repeat inference or an external effect.

Phase 34 adds durable application-level coordination around those boundaries while retaining the existing single-owner product mode.

## 2. Schema v19

Migration `019_sqlite_coordination.sql` advances SQLite to **user_version 19**.

### `local_process_incarnations`

Durable local process/store-generation identity:

- opaque `process_id`;
- permanent `store_id`;
- `store_generation` bound to `store_meta.restore_generation`;
- random incarnation nonce;
- process kind:
  - `owner_lock`;
  - `trusted_local` (currently test-only);
- OS PID as diagnostic provenance only, never liveness authority;
- active/stopped/fenced state;
- start/heartbeat/expiry/stop timestamps.

A successful new `owner.lock` acquisition is authoritative evidence that an older owner process no longer owns the store. Old active owner rows are fenced during registration. Trusted-local test processes use bounded heartbeat expiry.

### `local_coordination_leases`

Generation-fenced scoped leases:

- `write`;
- `execution`.

Each lease binds:

- scope kind/key;
- holder process incarnation;
- store generation;
- monotonic lease generation;
- optional run ID;
- expiry;
- state:
  - active;
  - released;
  - hazard.

Important distinction:

- stale/dead **write** lease → may be released after fencing;
- stale/dead **execution** lease → becomes a **hazard**, because expiry/process death is not proof that an external effect did not occur.

### `local_maintenance_lease`

One exclusive maintenance lease for:

- migration;
- backup;
- restore;
- maintenance.

It cannot be acquired while active write/execution leases or unresolved coordination hazards exist.

While a live maintenance lease is active, normal `SessionStore::connection()` access fails closed. Maintenance internals deliberately operate through the already-owned raw connection, preventing ordinary APIs from slipping into the maintenance window.

### `local_coordination_events`

Immutable bounded process/lease/maintenance provenance, including:

- process registration/stop;
- lease acquire/release/hazard;
- hazard reconciliation;
- provider-attempt lease reconciliation;
- maintenance acquire/release;
- restore-generation advance.

The table is append-only through immutable update/delete triggers.

## 3. v18 → v19 migration

The migration remains transactional and runs only after production `owner.lock` acquisition.

The v19 upgrader:

1. refuses ambiguous live legacy Phase-12 `client_runtimes` / active-or-hazard `execution_leases`;
2. claims the legacy singleton migration generation;
3. creates the Phase-34 coordination records;
4. advances `schema_meta` / `store_meta.schema_epoch` to 19;
5. preserves permanent `store_id`;
6. preserves existing `restore_generation`;
7. runs foreign-key validation before commit.

A populated v18 regression proves store identity/generation survive the upgrade. A second regression proves ambiguous old live coordination state fails closed and leaves v18 intact.

## 4. Process/store-generation fencing

Every persistent store handle carries the process incarnation and the exact restore generation observed at open.

Before normal store access:

1. current `store_meta.restore_generation` must match the handle;
2. the process row must still be active;
3. a trusted-local process heartbeat must still be live;
4. no current exclusive maintenance barrier may block ordinary access.

A stale process therefore cannot resume writes merely because its SQLite connection object still exists.

The production owner process has no heartbeat expiry because `owner.lock` is the stronger liveness/ownership boundary. Trusted-local processes in the adversarial harness use explicit TTLs.

## 5. Scoped write and execution leases

Phase 34 exposes narrow store primitives:

- `acquire_local_write_lease`;
- `acquire_local_execution_lease`;
- `settle_local_lease`;
- `resolve_local_execution_hazard`;
- process heartbeat/introspection;
- exclusive maintenance lease acquisition/release.

Lease acquisition is a short `BEGIN IMMEDIATE` transaction and is generation-fenced by both process incarnation and restore generation.

The same live holder may renew its exact lease. Another process cannot take an active generation. A stale write lease may advance to the next generation only after fencing. An execution hazard cannot be stolen merely because time passed.

## 6. Real provider dispatch is execution-leased

The provider journal path now uses the Phase-34 execution lease, not only synthetic coordination tests.

For a persistent store, each real provider invocation uses an exact scope equivalent to:

```text
provider:<run_id>:invocation:<ordinal>
```

### Dispatch intent

Inside the same SQLite transaction as `InvocationStarted`:

1. model-context authority is revalidated;
2. an exact generation-fenced execution lease is acquired;
3. the invocation receipt is inserted as `started`;
4. the durable attempt becomes `provider_started`;
5. the journal event commits.

Only after that transaction succeeds may provider code be entered.

### Normal provider completion

`InvocationFinished` updates the exact started receipt and releases the exact process/store-generation execution lease in the same transaction.

### Process death / ambiguous provider result

If the process dies after dispatch intent:

1. new owner acquisition fences the dead process;
2. its active provider execution lease becomes a hazard;
3. startup recovery classifies the durable provider attempt as **unknown**;
4. retry policy/recovery hint is persisted;
5. only after that unknown/no-replay state is durable is the provider coordination hazard released.

Releasing the coordination hazard does **not** authorize a retry. Retry authority remains the pre-existing explicit interactive/persisted autonomous operation policy.

The kill test also writes a one-time external-effect witness after `InvocationStarted`, kills the process, and proves restart records exactly one unknown attempt without repeating that effect.

## 7. Paid result persistence is now a real crash boundary

The earlier implementation stated that a paid result was persisted before branch CAS, but result persistence and branch attachment still occurred inside one SQLite transaction. A process crash between those statements would roll the whole transaction back.

Phase 34 corrects that.

### Transaction A — durable result settlement

`persist_finish_in` commits:

- immutable final turn/result node;
- run final status/node/time;
- invocation receipt settlement/unknown state;
- LLM attempt settlement;
- logical operation state;
- retry/recovery projection;
- tool/effect hazard state;
- web request settlement;
- semantic `llm.attempt_settled` event.

The run remains:

```text
attach_status = pending
branch.active_run = this run
```

Transaction A **commits before** attachment is attempted.

### Transaction B — exact canonical attachment

`attach_settled_run` then performs only the canonical ref attachment:

- recheck exact Workspace writer epoch pinned at admission;
- require exact branch active run;
- require exact prompt head;
- update branch head/revision by CAS;
- mark run `attached`; or
- preserve result and mark `conflict`, clear stale active-run marker, create recovery guidance.

It emits either:

- `llm.result_attached`; or
- `llm.result_attachment_conflict`.

### Crash between A and B

Startup recovery scans settled runs with `attach_status='pending'` and performs Transaction B **without provider inference**.

The kill test pauses after Transaction A commits, kills the process, restarts, and proves:

- one attempt only;
- one durable final result only;
- no retry schedule;
- branch attaches/reconciles from the existing result;
- no second provider call is invented.

## 8. SQLite busy/backoff semantics

The normal connection has a bounded SQLite busy timeout.

Phase 34 additionally maps `SQLITE_BUSY` / `SQLITE_LOCKED` to an explicit conflict:

> retry transaction acquisition explicitly; no external effect was replayed

This is important: contention may retry **database acquisition**, but never provider/tool work.

The adversarial test holds a competing `BEGIN IMMEDIATE`, forces another process into the busy path, and proves no effect counter/dispatch is repeated.

Online backup/restore copy uses the SQLite backup API with bounded page steps and bounded Busy/Locked sleeps. The copy loop never invokes providers/tools.

## 9. Exclusive migration/maintenance barrier

A live maintenance lease excludes coordinated write/effect admission.

The Phase-34 tests prove:

- active write lease blocks migration maintenance;
- active write lease blocks backup;
- unresolved execution hazard blocks maintenance;
- after safe release/reconciliation, migration maintenance can be acquired;
- while migration maintenance is active:
  - another write lease is rejected;
  - an ordinary `SessionStore::create` is also rejected.

Thus the barrier applies to normal store APIs, not only callers that voluntarily use the coordination lease API.

Migrations themselves still run under production `owner.lock`; the test-only independent direct connection refuses to migrate a non-current store.

## 10. Backup and restore generation fencing

### Backup

Backup now:

1. acquires exclusive `backup` maintenance authority;
2. creates a new private destination;
3. runs SQLite `VACUUM INTO` while ordinary store access is fenced;
4. removes the partial destination on failure;
5. releases maintenance authority.

An active writer prevents backup.

### Restore

Restore is same-store only in Phase 34:

- regular non-symlink source file;
- application ID must match;
- source must already be schema v19;
- source `store_id` must equal the live store;
- source must pass `quick_check` and FK validation.

Cross-store raw restore is refused; use semantic import/replication instead.

The restore protocol avoids the dangerous window where old snapshot bytes could become live before a new generation is durable.

It performs:

1. acquire exclusive `restore` maintenance lease on the live store;
2. copy the source snapshot into an in-memory staging SQLite database;
3. in staging, compute:
   `new_generation = max(live_generation, source_generation) + 1`;
4. fence every old active process;
5. release old write leases and retain unresolved generic execution effects as hazards;
6. clear stale maintenance ownership;
7. register the replacement owner process at the new generation;
8. append the generation-advance event;
9. integrity-check the staged image;
10. use SQLite Online Backup to atomically make that **already-fenced** staged image live.

If the copy fails before SQLite reports DONE, SQLite backup finalization rolls the incomplete destination back.

If the process dies after DONE but before its in-memory handle is updated, restart still sees the new generation already present in the live database. A kill test proves this exact boundary.

Any pre-restore process handle subsequently fails with a store-generation conflict.

## 11. Adversarial independent-WAL test harness

Phase 34 includes a private `#[cfg(test)]` helper that opens independent SQLite connections to the same current v19 store without taking `owner.lock`.

This exists **only** to prove the future protocol. There is no shipping flag/API that exposes it.

The adversarial matrix covers:

- two process incarnations and lease generations;
- stale write lease takeover;
- stale execution lease → hazard;
- dead process effect hazard reconciliation;
- SQLite busy/locked acquisition;
- exact request-receipt contention;
- prompt admission contention;
- result settlement contention;
- context apply contention;
- Workspace writer failover contention;
- migration/backup vs active writer;
- ordinary write vs maintenance barrier;
- kill before WAL transaction commit;
- kill after provider dispatch intent / external-effect witness;
- kill after provider result commit but before branch attach;
- backup → mutate → restore generation fence;
- kill after restored image atomically commits but before process-memory update.

The tests use real separate SQLite connections and child processes for crash boundaries rather than only mocking lock state.

## 12. Exact CAS/contention results

The matrix preserves the pre-existing higher-level authorities:

### Request receipt

Two connections race the same exact request ID/fingerprint and converge on one durable request receipt. Rebinding the ID remains a conflict.

### Prompt admission

Competing prompt admissions still use exact branch head/revision/active-run CAS. Only one can admit the canonical branch mutation.

### Result settlement

A duplicate/concurrent settlement cannot create two paid final results or advance the branch twice.

### Context apply

The exact context application receipt remains idempotent and target head/revision CAS remains authoritative.

### Workspace failover

Competing failovers from the same expected writer epoch yield one valid epoch advance. The stale competitor fails; no last-write-wins behavior is introduced.

## 13. owner.lock remains the product boundary

The successful adversarial matrix is a prerequisite for considering direct mode, not a reason to silently turn it on.

At the Phase-34 checkpoint:

- `SessionStore::open` still takes `owner.lock`;
- actual binary smoke proves a second ordinary owner is refused;
- no CLI flag enables trusted multi-process direct DB access;
- remote clients still use typed application protocols;
- network-mounted SQLite is still unsupported;
- one logical Workspace writer and writer epoch/head/revision CAS remain unchanged.

The test-only `trusted_local` process kind is not a product claim.

## 14. Validation

Validated on the Phase-34 implementation tree before final documentation sync:

- Phase 34 focused/reference checks: **99/99**;
- Phase 33: **119/119**;
- Phase 32: **120/120**;
- Phase 31: **159/159**;
- Phase 30: **142/142**;
- Phase 29: **75/75**;
- `aw-store`: **49 + 29 + 22 + 28**;
- default `aw-cli`: **90 + 50 + 6**;
- minimal: **4 + 0 + 6**;
- web-only: **57 + 0 + 6**;
- net-only: **4 + 18 + 6**;
- OIDC-only: **7 + 0 + 6**;
- Phase 29–33 actual-binary smokes: green;
- Phase 34 actual-binary smoke:
  - schema v19;
  - owner.lock retained;
  - second owner refused;
  - process/generation lifecycle durable;
  - backup succeeds;
  - trusted multi-process product mode absent;
- full `cargo check --locked --workspace --all-targets`: green;
- full `cargo test --locked --workspace --all-targets`: green with explicit trusted `AW_GIT`/`AW_BASH` paths in this connector.

Documentation-synchronized source/static validation: **984 active files**, 24 workspace manifests, 80 Python sources parsed, 28 TOML files parsed, 103 JSON files parsed, 4 shell scripts syntax-checked, 8 JavaScript sources checked, 653 Rust test attributes authored, 702 local links checked, **0 errors, 0 warnings**. The complete **36-step reference profile** passes; as designed, that profile reports `rust_executed=false` / `nix_executed=false` and is not a replacement for native Rust validation.

The separate full-host Clippy/Nix release gate remains environment-pending in this connector.

## 15. Next boundary

The evidence required by the Phase-33 handoff is now present. A future phase may **consider** a trusted opt-in direct local multi-process store mode, but it must remain explicit and owner/service mode must remain the default.

Recommended next boundary:

> **Phase 35 — fresh clean-room review of Phase-34 coordination, then (only if that review remains green) a guarded trusted-local direct-mode product/API surface using the already-proven process generations, execution leases, maintenance barrier, bounded busy handling and exact CAS semantics.**

Phase 35 must not:

- make direct mode the default;
- bypass process/store-generation fencing;
- weaken the real provider execution lease;
- treat lease expiry as proof an external effect stopped;
- replace Workspace writer epochs with SQLite writer ordering;
- relax request/head/revision CAS;
- allow migration/backup/restore through active coordination authority;
- use raw WAL as synchronization;
- expose network-mounted SQLite;
- replay unknown provider/tool effects.

The full-host Clippy/Nix gate remains a separate pending release gate.
