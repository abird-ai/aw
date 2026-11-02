# AW shared `aw-tools` convergence plan — 2026-10-31

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-5.6 Sol
Owner: @pvl
Status: plan only; implementation unchanged by this review

## Authority

The model-facing/runtime semantic authority remains dotlink DL-150 at sibling
repo:

```text
../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/README.md
```

The cross-repository implementation architecture is:

```text
../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/SHARED-IMPLEMENTATION.md
```

This document is the **AW implementation/migration delta**. It must not invent a
second semantic contract.

Reviewed AW state:

- `e337f0d` — shared `aw-tools-core`, secured Unix/Windows backend extraction,
  current AW adapter and Process/task primitives;
- `6f7845d` — c34ea5a parity audit, canonical reference snapshots and current
  validation/handoff.

The current worktree was clean before writing this plan.

## Target architecture

Rename the packages so package names describe their final responsibilities:

```text
current                              target

aw-tools-core                    ->  aw-tools
aw-tools                         ->  aw-tools-adapter
aw-tools-fs-unix                     aw-tools-fs-unix
aw-tools-fs-win                      aw-tools-fs-win
```

Target responsibilities:

```text
aw-tools
  canonical filesystem/tool semantics
  Graph compiler + NodePlan
  shared JobManager
  Task projection
  replay/output/retention semantics
  backend traits

aw-tools-fs-unix / aw-tools-fs-win
  secure native filesystem implementation

aw-tools-adapter
  AW ToolRegistry/ToolSpec/ToolPolicy projection
  Actor/source/site/epoch authority
  original TurnJournal/Session integration
  AW Process/PTY custody and runner
  compatibility parsing
  ToolOutput/provider/UI integration
```

## Review of current AW implementation

The latest AW source is a strong base and should be evolved, not replaced.
Already shared and validated:

- canonical schema registry;
- metadata/filter/sort/discovery engine;
- requested-root `.gitignore` behavior and bounded coverage;
- invalid-UTF8 `path_encoding` reporting;
- UTF-8 line/byte read paging;
- exact snapshot edit planning;
- write-only canonical write path;
- patch/mutation helpers;
- binary splice/digest primitives;
- lifecycle planning and substantial native cp/mv/rm;
- secured Unix/Windows backend crates;
- pure Graph compiler;
- source-bound Job transition projection;
- task fence/checkpoint/retention/spool primitives;
- AW Process spooling, cancellation and exact-source bridge.

Do not rebuild these as a new layer.

## Resolved P0 convergence gaps

### P0 — authority/native correctness

A3/A4 now implement the sealed operation-class model: final content symlinks
perform lexical then resolved-target authorization, ordinary hardlinks are
accepted without weakening private-state single-link checks, reversible UTF-8
control-character labels are supported, private stage/tombstone state is exact
identity-owned, recursive lifecycle operations use bounded descendant/exclusion
preflight, and restart reconciliation acts only on recorded matching identities.

### P0 — Job/runtime convergence

A5/A6 now use one shared `JobManager` plus AW's single `JobRuntime`. Immutable
NodePlans capture semantic budgets/authority/runtime/footprints; batch, job_start,
job_* and direct trusted shells share one ID/output/ACK/restart lifecycle. AW's
outer TurnJournal remains the durable replay fence and `PipeProcess` remains the
only OS process custodian. Per-node Graph state is deliberately live-session-only
and is never reconstructed/replayed after process replacement.

### P1 — adapter/core boundary

1. Complete grep execution/paging still lives substantially in current
   `aw-tools/src/search.rs` rather than the shared engine.
2. Binary request range preparation and some mutation/result orchestration still
   live in the adapter.
3. Patch compatibility and canonical execution are interwoven.
4. Current adapter package owns AW-specific Process and browse APIs used outside
   the model tool registry; these must be deliberately retained as AW host APIs,
   not accidentally moved just because of the rename.

## Package rename phase

The rename is intentionally **mechanical and isolated**.

### Rename `aw-tools-core` -> `aw-tools`

- move directory/package name;
- crate path becomes `aw_tools`;
- preserve every feature and module exactly for this commit;
- change `aw-tools-fs-unix`/`aw-tools-fs-win` dependencies/imports to `aw-tools`;
- update descriptions/docs mechanically;
- no status/schema/path/runtime semantic changes.

### Rename current `aw-tools` -> `aw-tools-adapter`

- move directory/package name;
- crate path becomes `aw_tools_adapter`;
- dependency on canonical `aw-tools`;
- update AW root, `aw-surfaces`, workspace/UI, subagent and Process consumers to
  use `aw_tools_adapter` where they need AW integration;
- preserve root-level public reexports from `aw` where useful so user-facing AW
  SDK churn is minimized;
- preserve Process/browse behavior exactly;
- no compatibility removal in the rename commit.

### Rename acceptance

Before semantic work continues, rerun the current validation baseline and prove
behavior/source surface did not change except package/import names.

## Final crate boundary

### `aw-tools`

The package becomes the implementation, not a helper library. Move all
non-AW-specific canonical behavior into it.

Suggested logical modules after convergence (organization may remain flatter if
that is clearer):

```text
schema
status
path
metadata
filesystem/{backend,walk,discovery,operations,lifecycle}
search/grep
read
mutation
edit
patch
binary
graph/{grammar,plan,footprint}
job/{state,manager,output,replay,task}
progress
media
sandbox (only genuinely reusable planning)
```

Do not reorganize merely for visual symmetry; move modules only where ownership
becomes clearer or duplicate code disappears.

### `aw-tools-adapter`

Keep:

- `configure` and AW `ToolSpec` mapping;
- `ToolContext` cancellation and Agent callbacks;
- ToolPolicy/Envelope checks;
- TurnJournal bridge;
- compatibility input translation;
- AW-specific provider/media projection;
- AW Process/PTY host implementation;
- AW-specific `browse`/WorkspaceFiles helper used by UI/surfaces;
- AW CLI/runtime wiring.

Move out:

- canonical grep scanning/rendering;
- canonical path/filter semantics;
- canonical result fitting/status grammar;
- canonical mutation/lifecycle decisions;
- canonical binary range semantics;
- canonical Graph/Job/Task lifecycle logic.

A large process runner in the adapter is acceptable if it is AW-specific native
custody. A second Job state machine is not.

## Prepared filesystem authority

The current `Root` remains a useful secured backend, but final-link semantics
need a stronger prepared-target boundary.

Plan one capability flow rather than tool-specific canonicalize/reopen paths:

1. adapter checks lexical AW protected/policy requirements;
2. secured backend resolves/pins according to operation class;
3. adapter rechecks the resolved target under the same Actor/source/site/effect;
4. canonical operation receives an authorized prepared target/plan;
5. backend performs the effect through retained native identity.

AW currently has a single workspace-root filesystem model, while dotlink has
multiple allow/deny roots. The shared interface must support both without making
AW emulate dotlink policy or vice versa.

### Final symlink matrix

Tests must prove:

- read/bread follow authorized final symlink to regular target;
- write/bwrite/edit/bedit/patch Update replace the resolved regular-file entry
  and preserve the link;
- resolved target outside current authority fails;
- lexical protected path fails even if link target would otherwise be allowed;
- dangling final link is not create-missing;
- create-only Add/write loses safely to concurrent link insertion;
- cp/mv/rm operate on the final link itself;
- child symlinks in discovery/copy/delete are never traversed accidentally;
- symlinked discovery root displays the caller's lexical prefix.

## Hard-link convergence

Remove the historical “singly-linked regular files only” rule from canonical AW
model-tool behavior.

Retain single-link checks only for **internal protected spool/control artifacts**
where link count is itself part of the private-state integrity invariant.

That distinction is important: `task_spool` may legitimately require one link;
a user workspace regular file may not.

Update discovery metadata/kind classification, read/open helpers, staging,
lifecycle copy/remove and tests accordingly.

## Path convergence

Unix backend path validation should reject only structural/native-invalid path
syntax, not display-unsafe UTF-8. Control/newline/tab names remain legal Unix
filenames and are quoted by the canonical renderer.

Keep:

- no NUL;
- no empty, `.` or `..` component;
- canonical `/` separators;
- invalid-UTF8 omission with `incomplete path_encoding`;
- platform-specific Windows invalid component rules.

## Private mutation state

Adopt the canonical exact private-artifact protocol from dotlink's shared
implementation plan.

AW host responsibilities:

- provide bounded protected storage under the existing AW state/Session owner;
- commit/remove exact artifact records durably;
- reconcile before model filesystem discovery becomes available after restart;
- never hide/delete an identity mismatch.

Native backend responsibilities:

- create restrictive empty named stages before user content;
- return stable parent/object identity;
- publish/detach atomically;
- never use broad prefix ownership as proof.

Core responsibilities:

- record state machine and quotas;
- exact private-name registry presented to the shared walker;
- cleanup/recovery decisions;
- known/mixed/unknown outcome classification.

## Shared JobManager target

The JobManager moves into canonical `aw-tools` and becomes the only live-session execution
lifecycle state machine for Graph Jobs/Tasks used by AW and dotlink. It is **not** a persistent workflow database: process/session replacement ends the live Job namespace. AW's existing unresolved outer `ToolStarted` journal intent prevents cross-process redispatch; dotlink uses the sealed transport proof-or-minimal-tombstone rule where required. Graphs, NodePlans and retained Job results are not resurrected across processes.

Do **not** add a new manager beside current modules. Refactor/reuse:

- `filesystem/job.rs` -> canonical Job/Node transition state;
- `task.rs` / `task_fence.rs` / `task_checkpoint.rs` -> identity, durable
  transition and recovery primitives;
- `task_spool.rs` -> shared output store primitives;
- `task_retention.rs` / `task_ops.rs` -> retention/control/Task projection;
- AW `process.rs` -> AW `ShellRunner`/Process host implementation;
- dotlink current JobManager behavior -> source of proven runtime semantics to
  port into the shared manager, not code to preserve separately.

### Shared manager owns

- Job registry/ID namespace;
- immutable Graph/NodePlans;
- process-wide runner permits + per-Job width;
- deterministic native conflict gate;
- journal transition ordering;
- cancellation/revocation intent;
- output writer leases, raw paging, ACK and owners;
- exposure/handoff;
- Task projection;
- restart orchestration;
- bounded replay/control/output admission.

### AW host supplies

- Job identity admission under the original Session;
- exact TurnJournal CAS/checkpoint;
- Actor/source/site/epoch/turn/call liveness;
- prepared native filesystem authority;
- AW Process/Process Actor launch, inspect and stop proof;
- protected output/replay/private-state store roots;
- provider/ToolOutput projection.

No second AW journal/database is introduced.

## Immutable NodePlan

Add one canonical prepared NodePlan before model-visible Job execution. It must
capture:

- tool/registry eligibility version;
- canonical validated arguments;
- semantic work/result ceilings;
- conservative resolved read/write footprint;
- authority generation/fingerprint;
- shell interpreter/runtime identity when applicable;
- replay/node identity;
- Task/result eligibility.

The same plan drives execution and deterministic conflict admission; do not
re-parse path footprints separately.

## AW Job execution sequencing

For an executable node:

```text
pending
  -> TurnJournal claim CAS
  -> revalidate source + exact node grant
  -> durable running/launch checkpoint when required
  -> execute via canonical native engine or AW Process runner
  -> durable terminal checkpoint
  -> only then unlock dependents
```

A journal callback panic/ambiguous commit remains `unknown`/fenced and never
causes speculative replay.

## AW direct calls

Do not force all direct filesystem calls through retained Jobs.

- existing direct filesystem tools call canonical `aw-tools` operations;
- `batch`, `job_start`, `job_list/get/output/ack/cancel/restart` and direct trusted shells use one shared live `JobRuntime`/`JobManager`;
- direct `bash`/`pwsh`/`powershell` are one-ShellNode optimistic Jobs, so foreground completion, abandonment handoff, cancellation, retained output and restart share the same lifecycle;
- native exact results and ShellNode stdout/stderr/result streams are retained by bounded JobManager-owned memory stores under one session byte quota; direct legacy Process capture remains available only for adapter-local compatibility/tests;
- Task projection uses a one-node Job when applicable, and Task result ownership is added only when an actual Task projection exists;
- AW `PipeProcess` remains the OS custody/stop-proof implementation; Graph execution does not introduce a second process supervisor.

## Compatibility strategy

Compatibility parsing remains adapter-local until coordinated removal:

- read offset/limit;
- conditional write options;
- old edit object forms;
- best-effort patch;
- asymmetric grep fields;
- historical shell configuration aliases.

Canonical schemas from `aw-tools` never advertise these fields.

Remove a compatibility path only after live AW callers/tests have migrated; do
not let compatibility shape shared canonical semantics.

## AW implementation phases

### A0 — record and seal

- record this plan and cross-repo canonical plan;
- no Rust change;
- after dotlink folds the small pre-implementation spec clarifications, snapshot
  the new exact canonical README/TODO into this owned plan reference directory;
- update the parity matrix once, not via another competing handoff tree.

### A1 — mechanical rename

- `aw-tools-core -> aw-tools`;
- current `aw-tools -> aw-tools-adapter`;
- manifests/imports/reexports only;
- preserve features/API behavior;
- complete validation before A2.

### A2 — adapter thinning

- move remaining grep execution into shared package;
- move binary range planner and canonical result handling;
- separate strict patch execution from AW compatibility translation;
- eliminate duplicate canonical validation/status rendering;
- keep Process/browse/provider/config AW-specific.

### A3 — native path correctness

- prepared operation-class targets;
- final symlink dual authorization;
- hard-link support for user regular files;
- Unix quoted-control path support;
- lexical-prefix discovery roots;
- descendant policy/race tests;
- preserve no-cross-mount/reparse rules.

### A4 — shared mutation/private lifecycle

- exact private artifact records with an explicit volatile-vs-durable Store
  capability; volatile ownership is sufficient for transient stage/backup names,
  never for a namespace-detaching `rm` tombstone;
- durable stage/backup/tombstone reconciliation; multi-root `rm` reserves every
  cleanup record before its first detach and revalidates source identity before
  each publication;
- cp/mv/rm complete policy semantics;
- failure/crash injection;
- native Unix/Windows/macOS coverage.

### A5 — shared JobManager

- consolidate current pure Job + task primitives into one manager;
- add immutable NodePlan and conflict footprints;
- capture node semantic ceilings;
- shared output store/ACK/owners;
- shared replay state machine;
- restart and paged listing;
- no model registration yet.

### A6 — original AW host binding

**Implemented checkpoint:** complete for the live-session design. Mixed native/ShellNode Graphs are admitted through one configured node registry; prepared shell executable identity is pinned and revalidated; direct shells and `job_start` share the same runner; cancellation/revocation terminate the same process tree with fail-closed unknown classification when stop/setup cannot be proven. AW's outer durable tool journal is the replay fence for the whole Graph invocation. Per-node Job transitions remain intentionally live-session state—there is no persistent Graph database or reconstruction after process loss.

- outer TurnJournal replay/no-replay fence for Graph-producing tool calls;
- source/grant liveness and exact per-grant generation watches;
- AW `PipeProcess` runner, process-tree stop proof and post-spawn unknown fencing;
- bounded JobManager-owned native/ShellNode retained streams;
- cancellation/revocation and Session teardown;
- Task projection;
- host/process/output fault injection.

Canonical `batch/job_*` and direct one-ShellNode trusted shells are enabled behind the intended AW tool policy. Internal Graph state is not a restart database; an interrupted outer invocation remains uncertain and is never automatically replayed.

### A7 — shared-consumer readiness

**Implemented checkpoint:** public dependency/API hygiene and the reusable fixture are complete. The reviewed external-consumer pin is `656ce69` (semantic convergence `75f6f00`; ShellNode spool correction `c0d8ee3`). `aw-tools`, `aw-tools-fs-unix` and `aw-tools-fs-win` have no normal dependency on AW Agent/adapter crates. The opt-in `aw-tools/conformance` feature provides versioned host-neutral schema/request/Graph/status vectors. Matching platform `CapabilitySet` APIs accept prepared multi-root grants, independent read/write exclusions and host-owned mutation registries while implementing the same shared `SecuredFilesystem` contract as a single AW `Root`; `rm` additionally requires that registry to attest durable recovery storage before any tombstone reservation can succeed. Cross-root lifecycle and cross-grant final symlinks stay in shared code. Disposable standalone consumers have executed the fixture, a write-only-destination cross-root copy and an exact Git-revision dependency on `656ce69` using public APIs only. Direct host adapters pass their actual presentation/result budget into direct rendering, while immutable Graph NodePlans keep separately captured semantic ceilings. Shared engine maxima remain deliberately above both product defaults: 64 MiB render/read/per-stream bounds and an 8 GiB aggregate session spool permit dotlink's current configured ceilings while AW narrows them in `LocalToolLimits`. Windows UNC/network-share model paths are represented canonically as `//server/share/...`; prior Windows cross-builds were green, but the current 2026-11-03 environment lacks the Windows GNU std target, so a fresh matching-toolchain/native Windows gate remains required before release.

Freeze a reviewed shared-crate commit for dotlink consumption:

- stable canonical schemas;
- no AW Agent types in `aw-tools` public semantic API;
- secured backends independently consumable;
- exact pinned revision documented;
- conformance fixture reusable by dotlink.

### A8 — dotlink differential support

AW side supplies only shared test fixtures/API fixes discovered by dotlink
integration. Do not add dotlink transport/policy types to the shared crate to
make adaptation easier.

## Validation gates

Keep all existing successful gates and add convergence-specific tests.

### Mechanical rename gate

- all current shared RO/RW/full tests;
- current AW native tool tests;
- secured Unix backend + platform;
- Agent core;
- CLI binary/lib;
- workspace all-target check;
- strict Clippy;
- Windows GNU cross compile;
- no_std kernel;
- rustfmt + diff check.

### Semantic convergence gate

Add:

- final symlink read/write/edit/binary/patch/lifecycle matrix;
- lexical deny + resolved allow/deny race cases;
- regular hardlink read/write/cp/rm/mv semantics;
- Unix control/newline/tab UTF-8 path round-trip;
- invalid-UTF8 path_encoding;
- exact private artifact identity/restart tests, volatile-tombstone refusal and
  all-reservations-before-first-detach failure injection;
- recursive protected descendant tests;
- native mount/reparse boundary tests;
- Graph conflict-order determinism;
- direct live-result-budget paging/receipt fit plus outer Graph result-budget
  invariance;
- journal ambiguous-commit fencing;
- process cancellation unknown-stop proof;
- raw output offset/ACK/multi-owner retention;
- restart current-authority recompilation;
- paged job listing.

### Platform acceptance

Do not call the shared backend complete from cross compilation alone:

- Linux native adversarial tests;
- Windows native NTFS ACL/reparse/hardlink/case-only/ReplaceFile tests;
- macOS native ACL/xattr/no-follow/rename tests;
- successful Bubblewrap runtime where host namespaces allow it;
- full AW workspace target matrix with terminal results; under bounded connector transports this may be partitioned by package without reducing target coverage.

## Commit strategy

Keep changes reviewable and reversible:

1. plan/docs only;
2. mechanical package rename only;
3. adapter-thinning moves;
4. native path/security semantics;
5. private mutation/lifecycle semantics;
6. JobManager consolidation;
7. AW host binding;
8. dotlink-consumer fixes.

Never combine the rename with final-symlink/Job behavior changes.

## Completion criteria

AW side is ready for dotlink cutover only when:

- package `aw-tools` is the canonical implementation;
- `aw-tools-adapter` has no independent canonical tool/Job semantics;
- secured backends accept both hosts without AW Agent dependencies;
- shared JobManager is real, not merely a pure projection;
- AW binds it through the original TurnJournal/Process authority;
- canonical direct tools and Graph nodes share the same operation engine;
- all semantic/platform gates relevant to AW pass;
- a pinned shared revision can be consumed by a standalone dotlink checkout.
