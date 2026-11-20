# DL-200 — phased implementation plan

Base design: [SPEC.md](SPEC.md)

Status: **SEALED / IMPLEMENTATION PLAN READY — NOT PROMOTED**

Seal: explicit user approval on 2026-11-20.

This file authorizes **planning only**. Do not change implementation source until the user
separately and explicitly promotes DL-200 implementation.

## Operating constraints

- Source/Git work uses **L5 only**.
- Canonical AW checkout: `/home/pvl/spaces/abird/src/aw`.
- Canonical dotlink checkout: `/home/pvl/spaces/abird/src/dotlink`.
- Read-only release reference: `/home/pvl/spaces/abird/src/dotlink-017-mounts`.
- Never use A1 for source authoring.
- Do not create another permanent worktree.
- Preserve unrelated dirty/staged work.
- Do **not** inspect or modify AW `.agents/plans/ux/**`.
- The frozen `017-sandbox-mount-convergence-2026-11-18` implementation remains frozen
  unless the user separately promotes it.
- Prefer shared AW ownership first; dotlink consumes shared facts and retains only
  configuration, transport, exact wire projection, operator wording/logging and deployment.
- Keep Rust abstractions zero-cost/small where possible. Prefer simple types and one owner
  over compatibility duplication.
- Implementation turns should aim for one coherent ~30-minute checkpoint and pause.
- Keep qualification focused on semantic regressions and real owner boundaries; do not
  over-granulate fixtures/evidence.
- No commit, push, tag, release or deploy unless separately requested.

## Phase 0 — shared Process settlement + dotlink 0.17.6 convergence

This phase is intentionally first. It fixes current reusable Process semantics before
DL-200 builds more Jobs/Agent behavior on them.

### AW

1. Add the one shared semantic fact in `aw-tools`:

   ```rust
   enum Tree {
       Empty,
       Live,
       Unknown,
   }
   ```

2. Add minimal platform observation for directly managed containment:

   ```rust
   ManagedProcessGuard::observe() -> io::Result<bool>
   ```

   Mapping belongs in the adapter:

   ```text
   Ok(true)  -> Tree::Live
   Ok(false) -> Tree::Empty
   Err(_)    -> Tree::Unknown
   ```

3. Refine `ProcessOutputMeta` in place:

   ```text
   stdout_bytes
   stderr_bytes
   stdout_truncated
   stderr_truncated
   stdout_complete
   stderr_complete
   ```

   Derive aggregate completeness; do not store a second aggregate truth.

4. Make `ProcessTerminal` the one final factual terminal value:

   ```text
   exit
   stop: Option<StopRecord>
   tree: Tree
   output
   ```

5. Make `ProcessReceipt` / `ProcessSettlement` derive compatibility/status fields from
   `ProcessTerminal`; remove redundant stored truth where compatibility permits.

6. Converge normal exit, timeout, cancel, runtime error and explicit termination on one
   settlement sequence:

   ```text
   exit-ready / stop boundary
   -> observe contained tree
   -> cleanup containment
   -> one immediate retry if cleanup is Unknown
   -> reap/finalize command
   -> bounded per-stream drain
   -> publish ProcessTerminal
   ```

7. Unix: hold the waited leader unreaped until final group observation/cleanup; use
   pidfd / `waitid(WNOWAIT)` / equivalent as appropriate.

8. Linux observation:
   - positive member => Live;
   - negative only with correct PID namespace + complete procfs visibility;
   - hidden/partial/wrong procfs => Unknown;
   - zombies alone are empty;
   - live threads under an exited/zombie leader are live.

9. macOS: use process-group observation.

10. Windows: query Job Object active membership separately from termination.

11. Preserve independently observed physical command exit across a racing forced cleanup.

12. Keep Bubblewrap/PID-namespace observation behind the shared execution backend. Do not
    promote the frozen 017 mount plan.

### dotlink

1. Consume the shared terminal/tree/output facts; do not recreate a semantic Guard/model.

2. Fix the current ordinary-exit Unknown-cleanup/disarm edge: an unresolved cleanup may
   never be hidden by a successful output drain.

3. Converge Bash launch with AW:
   `--noprofile --norc -c` for direct and Bubblewrap execution.

4. Project shared facts in one place:
   - `Tree::Live -> tree="leftovers"`;
   - `Tree::Unknown -> tree="unknown"`;
   - `Tree::Empty -> omit tree`;
   - per-stream drain-incomplete flags;
   - signal names via AW helper;
   - `stopped_by` from semantic StopRecord;
   - one concise lifecycle log line.

5. Product result policy:
   - leftovers may clear dotlink `success`;
   - leftovers do not rewrite the physical exit code;
   - leftovers alone do not turn the MCP call into a generic tool error.

6. Restore concise lifetime guidance:
   - foreground daemon idiom: `server & wait`;
   - Windows equivalent: `Wait-Process`;
   - state process-group/Job containment boundaries honestly.

7. Restore dotlink-owned bounded conversational runtime policy:
   - recommended coherent checkpoint ≈ **30 minutes**;
   - hard conversational command horizon = **35 minutes**;
   - one runtime-profile source of truth drives model/server guidance, shell schema maximum
     and runtime validation;
   - effective shell deadline = min(configured shell policy, hard turn horizon);
   - profiles without a bounded conversational turn keep the configured Process deadline
     and omit turn-horizon guidance;
   - `background=true` changes observation, not the command lifetime bound.

8. Keep the turn horizon out of AW Process semantics.

### Phase-0 qualification

Use focused semantic tests, not a large fixture expansion:

- clean exit / nonzero exit / explicit exit 137 / external SIGKILL;
- timeout / caller cancel / connector shutdown;
- parent exit with live child whose stdout/stderr are redirected away;
- child holding inherited output pipe past drain grace;
- cleanup Unknown then immediate retry;
- no disarm after unresolved cleanup;
- Unix leader held until final group signal;
- Linux zombie-only vs live-thread-under-zombie;
- incomplete/filtered/wrong procfs => Unknown;
- Windows active Job membership where platform is available;
- Bubblewrap namespace observation only where current trusted launcher policy permits;
- direct + Bubblewrap non-login Bash;
- 30-minute guidance / 35-minute hard-horizon projection in bounded dotlink runtime;
- unbounded runtime profile retains configured Process limit.

Checkpoint when AW shared semantics are green and dotlink is a thin consumer again.

---

## Phase 1 — Agent identity, history and access foundation

Land the durable owner model before broad public API cutover.

1. Persist `agent_state_current` with owner-local `rev`, lifecycle, admission, Project,
   Sandbox binding and immutable parent/delegation lineage.
2. Seed Agent rev once from the legacy Session-audit bridge, then stop deriving rev from
   audit.
3. Add Domain principal registry and user/group kinds under one `i:` handle namespace.
4. Add groups/membership + group rev/security generation.
5. Add Agent ACL + `acl_rev` + derived `access_generation`.
6. Keep Project as the existing Workspace-backed projection; do not create a Project
   service/database.
7. Add immutable `HistorySpan`, `HistoryBase`, `HistoryGrant` and
   `DisclosureManifest` storage/retention.
8. Implement lazy canonical HistoryBase sealing with no Agent rev/Event change.
9. Preserve stable immutable history-fact identity and original provenance across forks.
10. Add minimal Domain/Principal/Project/Agent snapshot reads with privacy-aware metadata.

Qualification:
- migration from live legacy Session state;
- direct ACL/group-generation behavior;
- no Agent rev fanout from group membership;
- exact fork attribution;
- disclosure-closure expansion denial;
- lazy sealing idempotence/reachability.

---

## Phase 2 — generic Events substrate

Build one Event owner before replacing legacy Job-only observer APIs.

1. Project canonical resource refs to Event subjects, including reversible handle-token
   encoding such as literal `.` -> `%2E`.
2. Reuse immutable `commit_log.event_id` as `Event.id`; keep canonical subject/envelope
   in bounded payload/projection data rather than legacy compact `entity_id`.
3. Add source-local `event_rev` and transaction-coupled Event emission.
4. Add rebuildable subject projection/index.
5. Add `events.read` cursors, replay floor, hidden-row privacy and wait/wake behavior.
6. Add durable Agent subscriptions with generation/baseline/lag semantics.
7. Deliver replay-safe EventInput through the existing Agent input owner.
8. Enforce exact source + recipient audience + DisclosureManifest authorization.
9. Persist causal root/hop/delivery safety projection so loop bounds survive restart/replay.
10. Add custom `#name` publication + typed EventType schema tag.
11. Add model `events_subscribe` / `events_publish`.
12. Migrate v36 SourceEventBook/pending-delivery state without a parallel long-lived Event
    runtime.

Qualification:
- wildcard cardinality/terminal semantics;
- encoded literal handles;
- hidden-source cursor privacy;
- replay/dedupe/lag;
- source+manifest disclosure;
- self-delivery suppression;
- causal 16-hop / 256-delivery persistence;
- custom typed-vs-generic Event decoding.

---

## Phase 3 — logical Sandbox, Sources, Checkpoints and Vault grants

Implement the logical resource layer without promoting frozen physical mount/config-v13
work.

1. Add `sandboxes_current`, named SourceBindings and durable Agent SandboxGrant evidence.
2. Keep Agent->Sandbox binding immutable.
3. Add host `sandbox.list/get/create/close` and `sandbox.sources.list/set`.
4. Implement open/closed lifecycle and quiescent source replace/derive.
5. Add SourceDriver trait/registry with Git as mandatory first driver.
6. Implement Git clean/dirty checkpoint anchors:
   - private-index synthetic commit for dirty state;
   - private retention ref for both clean and dirty anchors;
   - exact tracked + nonignored untracked regular-file behavior;
   - explicit submodule gitlink semantics.
7. Add Checkpoint/SourceSet storage with captured access/checkpoint-policy layout metadata.
8. Add multi-source barrier/capture/revalidate + crash reconciliation.
9. Add `checkpoint.list/get/create/restore/delete`.
10. Restore always creates new Agent + new Sandbox; never mutates an existing environment.
11. Add VaultProvider/VaultGrant storage and `agent.vault.list/set`.
12. Keep projection kinds provider/host-defined safe profiles; no arbitrary env/path/mount
    target language.
13. Keep immediate revocation factual: already-materialized capability may require Process
    termination.

Qualification:
- clean/dirty Git exactness and retention after reset/rebase;
- submodule behavior;
- multi-source all-or-nothing;
- cross-Project source authorization;
- Sandbox sharing vs authority;
- grant actor ceilings and ACL broadening;
- checkpoint history-only/selected/auto coverage;
- no credential/lease inheritance.

---

## Phase 4 — unified Agent lifecycle, input plan and Jobs surface

1. Add public host `agent.create`, `list/get/edit/close/admission`,
   `access.list/set`.
2. Implement single fresh/fork constructor with exact settled-Turn history boundary.
3. Implement private prepare -> atomic AW publication -> reconciled cleanup for
   Agent/Sandbox/checkpoint construction.
4. Reuse one authenticated mutation replay identity/fingerprint from existing tool/RPC
   envelopes; do not add per-method request IDs.
5. Implement model `agent(task, ..., from?)` with inherited source Project/ACL default and
   host-selected Sandbox policy.
6. Add host staged input plan:
   `list/add/edit/delete/send`, `inputs_rev`, creator/editor provenance, exact
   steer/interrupt Turn target and atomic Send.
7. Freeze execution-adjacent mutation at closing; retain closed history/access/checkpoint
   management.
8. Finalize model Jobs as exactly:
   `jobs_list/jobs_get/jobs_read/jobs_send/jobs_ack/jobs_cancel`.
9. Row-authorize child/tree traversal; lineage never grants visibility.
10. Do not fabricate destination Jobs for inherited historical Process/Graph/tool facts.
11. Converge Process stdin/EOF/cancel into Jobs while retaining lower CAS/Prepared/Unknown
    evidence.
12. Retire model queue CRUD, pause-after-turn and Jobs-specific subscribe.

Qualification:
- constructor replay;
- fork source-vs-history defaults;
- parent vs history ancestry;
- staged input CAS/send atomicity;
- close lifecycle;
- Jobs exact-target authorization;
- inherited history reads/no ghost Jobs;
- legacy compatibility during migration.

---

## Phase 5 — native typed ExtensionRegistry

1. Rename current hook-only `Extension` role to `Hook`; reclaim `Extension` for the
   installed unit.
2. Add one typed `ExtensionRegistry` composition root with typed subregistries.
3. Keep registration transactional and extension-owned; installed definitions are inert
   until explicitly activated/bound.
4. Add Tool/Hook/Action/Resource/View/EventType/EventHandler/Provider/ProviderHook/
   ProcessHook/UI/Setting/State/Compactor + native SourceDriver/SandboxDriver/VaultProvider
   registries.
5. Keep typed handles registration-local; unrelated root generations do not stale them.
6. Add scoped runtime `ctx.tools/state/events/settings/models/process/ui` ports.
7. Add typed State keys with per-key seq/CAS for snapshot-relative updates.
8. Add View enrichment with per-View errors that never fail the core snapshot.
9. Add durable EventHandler activation/baseline/cursor/lag/suspend semantics over commit_log.
10. Keep Compactor as context projection only; never rewrite HistoryBase.
11. Validate Histate-style fully typed registration without JSON/stringly Rust domain types.

Qualification:
- atomic registration rollback;
- definition vs activation;
- typed-handle reload behavior;
- State CAS;
- View isolation;
- EventHandler replay/retention;
- provider/process hook authority;
- source/vault/sandbox driver registration without granting use authority.

---

## Phase 6 — dynamic Process Extensions + TypeScript SDK

1. Generalize existing `aw-extensions` describe/manifest to the bounded DL-200 registry
   subset.
2. Extend existing bounded JSONL `v/id/method/params` envelope symmetrically; do not add
   HTTP/gRPC/JSON-RPC daemon machinery.
3. One top-level callback per process connection; nested host `ctx.*` calls only under an
   opaque host-minted invocation scope.
4. Reject unsolicited child calls and same-process synchronous re-entry.
5. Preserve 256 KiB frame / 128 KiB manifest scale, max 8 nested callback levels and 256
   nested ctx calls per root deadline.
6. Child manifest identity must match host-selected Extension ID.
7. Process exit/poison -> definitions unavailable + durable handlers suspended; restart
   requires fresh describe/validation/new registry generation.
8. V1 dynamic subset only:
   Tools, Hooks, Actions, Resources, Views, EventTypes, EventHandlers, UI, Settings, State.
9. Keep Provider/ProviderHook/ProcessHook/Compactor/SourceDriver/SandboxDriver/
   VaultProvider/TrustPolicy native-only until separately justified.
10. Ship a small TypeScript/JavaScript SDK + example.
11. Keep npm/pip/cargo package lifecycle outside AW.
12. No embedded Wasmtime; WASI experimentation runs as an external process speaking the
    same protocol.

Qualification:
- TS typed author path + plain JS runtime validation;
- framing/reconnect/poison;
- no blind replay of uncertain effects;
- no ambient credentials/filesystem/network authority;
- extension reload/handler suspend-resume;
- WASI-through-process smoke if convenient, not a release gate.

---

## Phase 7 — migration cleanup and final qualification

1. Remove public Session vocabulary/surfaces while retaining internal SessionStore.
2. Remove Signal type/terminology.
3. Remove Jobs-specific subscribe and obsolete queue/pause helpers.
4. Remove temporary duplicate owner/projection layers only after callers are cut over.
5. Migrate persisted legacy state with explicit one-time adapters; no forever-parallel
   runtime.
6. Keep temporary SDK source wrappers client-only; one semantic wire/runtime surface.
7. Update docs/handoffs/runtime-health guidance to the final API.
8. Run focused cross-repo qualification, then the appropriately broad Rust/Nix gates.
9. Recheck frozen 017 mount plan remains untouched unless separately promoted.
10. Recheck no duplicate Session/Event/Source/replay/cache/transaction owner was introduced.

## Cross-repo publication/repin rule

When implementation is separately promoted:

- keep AW shared semantic commits coherent and independently testable;
- dotlink consumes published AW facts rather than copying them;
- only repin dotlink after an AW commit/push when the user has separately authorized those
  Git actions;
- do not bundle dotlink product projection with unrelated AW semantic changes merely to
  reduce commit count.

## Promotion gate

DL-200 is sealed and implementation-planned, but **not promoted**.

The next source-editing turn must begin only after a separate explicit instruction to
implement/promote DL-200 (or a clearly scoped phase of it). Until then, no implementation
source changes are authorized.
