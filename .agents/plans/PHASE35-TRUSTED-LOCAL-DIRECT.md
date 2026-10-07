# Phase 35 — explicit trusted-local direct SQLite mode

Phase 35 exposes the previously test-only multi-process SQLite coordination model as a **separate explicit Rust API** after a fresh clean-room review of Phase 34.

The ordinary product boundary remains:

> **`SessionStore::open` is still owner/service mode and takes `owner.lock` exclusively.**

Trusted-local direct mode is opt-in:

```rust
SessionStore::open_trusted_local_direct(
    state_dir,
    TrustedLocalDirectOptions::default(),
)
```

It is intentionally not an implicit fallback and not a remote/network SQLite mode.

## 1. Clean-room review finding that blocked direct mode

The Phase-34 coordination primitives were correct, but the review found an exclusive-owner assumption in startup recovery:

- `SessionStore::recover()` scanned every `runs WHERE status='running'`;
- it also globally interrupted every started context operation and accepted web request.

That is correct when one exclusive owner exists. It is unsafe when multiple trusted direct processes are alive: a newly opened process could have “recovered” another process's still-running provider/context/web work.

Phase 35 therefore did **not** merely make the Phase-34 stress opener public. It first added durable process ownership to all in-progress state machines that recovery may mutate.

## 2. Schema v20

Migration `020_trusted_local_direct.sql` advances the store to **SQLite user_version 20**.

It adds process-incarnation provenance to:

- `runs`
  - `owner_process_id`
  - `owner_store_generation`
- `context_operations`
  - `owner_process_id`
  - `owner_store_generation`
- `web_requests`
  - `owner_process_id`
  - `owner_store_generation`

and indexes each live-state family by process ownership.

Historical settled facts are not rewritten. Nullable provenance exists only to permit a safe v19→v20 migration; a direct client refuses ambiguous unowned **live** rows and requires one authoritative owner-mode recovery pass.

The coordination marker becomes `coordination_schema=2`.

## 3. Direct mode is a separate file-lock ownership mode

Phase 35 uses the existing `owner.lock` as the outer process-mode fence:

- owner/service mode: **exclusive** lock;
- trusted-local direct mode: **shared** lock;
- many direct clients may coexist;
- owner/service and direct clients may not coexist.

This is stronger and simpler than trying to infer whether an exclusive owner process is alive from PID or heartbeat state.

Consequences:

- a direct opener that obtains the shared lock has proof that no owner/service process currently holds the store;
- stale owner-lock process rows can therefore be fenced before direct recovery;
- an owner opener that obtains the exclusive lock has proof that every previous owner/direct lock holder is gone, so it can fence all stale active process rows immediately;
- owner startup does not wait for a dead direct client's heartbeat TTL before recovery.

## 4. Current-schema-only, private-local-only direct open

`open_trusted_local_direct`:

- requires an **existing** initialized private state directory;
- never creates a store;
- never runs migrations;
- requires exact current schema v20 and the expected application ID;
- requires `coordination_schema=2`;
- validates directory/file ownership, mode, links and SQLite sidecars;
- is initially supported only on Linux;
- rejects network/FUSE/clustered/unknown filesystem families via a conservative local-filesystem allowlist;
- holds the shared outer file lock for the lifetime of the handle.

Remote clients still use typed application protocols. Raw WAL/page replication remains unsupported.

## 5. Process heartbeat and restore-generation fencing

Every direct process registers a normal Phase-34 `trusted_local` process incarnation bound to:

- permanent `store_id`;
- exact `restore_generation`;
- bounded heartbeat expiry.

Default heartbeat TTL is 15 seconds; the public option is bounded to 3 seconds–5 minutes.

A small dedicated thread only renews process heartbeat state. It never:

- migrates;
- runs recovery;
- calls a provider/tool;
- changes semantic application state.

If store generation changes or the process is fenced, heartbeat renewal stops.

## 6. Conservative ordinary-access lease

Phase 35 deliberately favors correctness over maximum local concurrency.

Every persistent ordinary `SessionStore` API call—owner mode or direct mode—takes the same short-lived coordination write scope:

```text
store:ordinary-access
```

The guard releases it automatically when the API connection guard drops.

Therefore:

- ordinary local store API calls are serialized across persistent processes;
- maintenance cannot begin after a caller passed an earlier check but before its transaction starts;
- active ordinary access blocks migration/backup/restore maintenance;
- maintenance blocks new ordinary access;
- SQLite Busy/Locked remains bounded and never authorizes provider/tool replay.

This can later be optimized into finer reader/resource scopes without changing the authority model.

## 7. Incarnation-aware recovery

New running/provider/context/web work records its exact process incarnation and store generation.

Direct recovery:

- never touches work owned by another currently active direct process;
- may recover only work whose owning process is missing, stopped, fenced, generation-stale, or heartbeat-expired;
- refuses unowned/pre-v20 live rows and asks for authoritative owner-mode recovery instead.

Owner-mode recovery:

- after obtaining the exclusive file lock, fences all prior persistent process rows;
- may recover historical unowned rows as well as abandoned process-owned work.

Provider recovery still obeys the Phase-34 order:

1. persist unknown/interrupted provider outcome;
2. persist retry/recovery/no-replay state;
3. persist effect hazard state;
4. only then reconcile the process-level execution lease;
5. never infer retry permission from lease expiry/process death.

## 8. Provider leases, paid-result split and exact CAS are unchanged

Phase 35 does not weaken any Phase-34 execution invariant.

Real provider dispatch still:

- acquires `provider:<run>:invocation:<ordinal>` execution authority in the same transaction as durable dispatch intent;
- enters provider code only after commit;
- settles the exact execution lease with the durable provider receipt.

Paid results still use:

- Transaction A: durable result/attempt/operation settlement;
- Transaction B: exact writer-epoch + branch head/revision attachment CAS.

A crash between them still reattaches the existing durable result without inference.

Request IDs, context apply, prompt admission, Workspace writer epochs and branch refs remain exact CAS/idempotent authorities.

## 9. Restore and backup

Restore remains **owner/service only**.

A direct handle cannot restore even though it holds a shared file-lock handle. Restore still requires the authoritative owner mode and uses the Phase-34 staged generation-fencing protocol.

Backup may use the normal maintenance barrier. It never replays effects.

## 10. Adversarial Phase-35 proof

New native tests prove:

- v19→v20 migration preserves store ID and restore generation;
- direct mode refuses an old schema and does not migrate it;
- two direct peers coexist and one does not recover the other's live provider-started run;
- after a direct process stops, another direct peer recovers only the abandoned run as unknown/no-replay;
- accepted web-request state remains live while its owner process is active and becomes interrupted only after that owner stops;
- ordinary-access serialization blocks a competing direct API call;
- ordinary access blocks maintenance and maintenance blocks ordinary access;
- direct clients hold the outer lock shared and exclude owner/service startup;
- owner/service mode reopens normally after the last direct holder exits;
- direct heartbeat renews the exact process incarnation;
- a killed direct process with a 30-second remaining heartbeat TTL is recovered **immediately** by a new owner because exclusive `owner.lock` proves the shared holder is gone;
- the recovered provider attempt remains exactly one unknown attempt, with no replay.

All Phase-34 crash/CAS/provider/restore tests remain mandatory.

## 11. Product boundary after Phase 35

Supported:

- default owner/service mode through `SessionStore::open`;
- explicit trusted-local direct mode through `SessionStore::open_trusted_local_direct`;
- multiple trusted direct processes on one supported local Linux filesystem;
- semantic remote protocols and replication.

Not supported:

- implicit direct mode;
- direct mode on network/FUSE/clustered/unknown filesystems;
- remote SQL access;
- raw SQLite WAL/page sync;
- owner/service and direct mode simultaneously;
- unknown-effect replay;
- last-write-wins branch refs;
- direct-client migrations;
- direct-client restore.

## 12. Validation

Validated on the Phase-35 implementation tree:

- Phase35 focused/reference checks: **47**;
- Phase34: **101** (historical v19 proof plus current-v20 compatibility checks);
- Phase33: **119**;
- Phase32: **120**;
- Phase31: **159**;
- Phase30: **142**;
- Phase29: **75**;
- `aw-store`: **56 + 29 + 22 + 28**;
- default `aw-cli`: **96 + 82 + 6**;
- minimal: **4 + 19 + 6**;
- web-only: **63 + 19 + 6**;
- net-only: **4 + 37 + 6**;
- OIDC-only: **7 + 19 + 6**;
- full `cargo check --locked --workspace --all-targets`: green;
- full `cargo test --locked --workspace --all-targets`: green with explicit trusted `AW_GIT` / `AW_BASH` paths in this connector;
- actual compiled-binary Phase-34 owner/backup smoke now initializes **schema v20** and keeps ordinary owner locking intact;
- actual inline TUI, live F6/F7, zero-model local-web/settings, and full-screen Git/PTY workspace smokes: green;
- source/static validation: **804 active files**, 24 manifests, 83 Python, 28 TOML, 102 JSON, 4 shell, 10 JavaScript, 704 authored Rust test attributes, 689 local links, **0 errors / 0 warnings**;
- design-doc reference validation: green;
- complete reference profile: **37/37**.

The reference profile intentionally reports `rust_executed=false` / `nix_executed=false`; it is static/reference evidence, not a replacement for the native Rust runs above. The separate full-host Clippy/Nix release gate remains environment-pending.

## 13. Next product work

The next product-facing boundary is the remaining CLI usability work already requested:

- clean Clap command/help architecture so `aw --help` exposes the real command surface;
- first-run interactive provider/model onboarding;
- provider/model management parity between native Settings and local web Settings;
- preserve zero-model local web startup and no-pair local-owner behavior.

Those changes must not weaken the Phase-35 storage ownership model.
