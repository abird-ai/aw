# CURRENT HANDOFF — AW live state + DL-200 R6

Canonical comprehensive handoff (**read this first after AGENTS.md**):
[AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-21.md](AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-21.md).

Copy-ready next-agent prompt:
[AW-DL200-R6-NEXT-AGENT-2026-11-21.md](AW-DL200-R6-NEXT-AGENT-2026-11-21.md).

Normative R6 design:
- [README](../plans/dl200-unified-jobs-agent-events-2026-11-20/README.md)
- [SPEC](../plans/dl200-unified-jobs-agent-events-2026-11-20/SPEC.md)
- [IMPLEMENTATION](../plans/dl200-unified-jobs-agent-events-2026-11-20/IMPLEMENTATION.md)
- [TODO](../plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md)

## Current authoritative snapshot — 2026-11-21

**R6 + the shared Process/release-correction amendment are SEALED and published. Phase 0B is
complete and published. Dotlink is repinned to the published corrective AW revision and the ordinary
Git-pinned consumer gate is green. The user explicitly deferred real-Bubblewrap worker-retirement
and Windows-native qualification, so Phase 1 is now underway.**

Current published refs:

```text
AW R6/Phase-0 publication       20a63a4d4547cc20b2a154787823125b82f5e385
dotlink R6 consumer publication 85ceccc1088e201c98c994a46a68834440eeab32
release/0.17                    576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2 = v0.17.9
dotlink AW pin                  20a63a4d4547cc20b2a154787823125b82f5e385
```

Pinned qualification is green: ordinary all-target/all-feature build, shell **86/86**, Graph
**13/13**, real queued Spawn-Lane cancellation, pre-spawn timeout/no-Process projection, canonical
spawn refusal, strict Clippy, fmt and diff-check. Real Bubblewrap worker-retirement and
Windows-native qualification remain explicitly deferred evidence, not claimed green.

Phase 1 foundation is underway. One private `StoreScope` loads the existing
`store_meta.store_id` + nonzero `restore_generation`; transparent `StorePos(u64)` carries internal
`commit_log.seq` ordering through commit insertion -> Job exposure projection; public/persisted
compatibility remains `u64`. Coordination loads identity+generation in one SQLite statement. Exact
Agent-ref lookup uses the existing `job_exposure_current` primary-key projection for both `a:0` and
non-root Agents, followed by the existing current authorization/binding recheck; it no longer scans
Sessions/branches to discover a canonical Agent.

A private zero-allocation `agent_owner` cutover seam now centralizes exact branch-bound Agent
revision reads and owner mutations **without changing legacy persistence yet**. Today `revision()`
still returns the historical `MAX(audit.seq)+1`; `created()`/`changed()` are in-transaction zero-work
markers that the exclusive cutover can replace with the authoritative row insert/rev bump. No audit
row or second state owner is created. Exact paths already crossing the seam are Agent create/fork,
Turn admission, durable result settlement + branch attachment settlement, changed-only Agent
subscriptions, delegated child publication (parent change + child creation with immutable parent
Agent lineage), stable-Agent close request, and exact stable-Agent settlement. Idempotent
subscription replacement remains revision-neutral. Session-only subagent runtime bookkeeping
(`running`, legacy failure/settlement) is deliberately not mislabeled as DL-200 Agent lifecycle.

Recovery is now exact too: the existing single recovery transaction records every recovered or
pending-attachment `(session, branch)`, deduplicates those keys in a cold-path `BTreeSet`, and marks
each Agent exactly once before the same recovery commit. Hot mutation paths stay allocation-free and
there is no generic transaction/mutation framework.

The committed-Input boundary is now exact too: `actor_input_prepare` remains proposal-only and
revision-neutral; only `Prepared -> Accepted`, after the live controller has actually queued the
message, marks the exact `(session, branch)` Agent changed in the same transaction. Dispatching,
Settled, Rejected, Unknown and lower ActorQueue staging/edit/claim/ACK/finalization do not add an
Agent mutation; Turn admission/settlement already own their respective Agent changes.

Legacy Session title/participation writers are now explicitly classified as backing compatibility
state, not Agent-owner mutations. Several Agents may share one Session, so broadcasting those writes
or arbitrarily choosing a branch would violate the final owner model. Session participation is only
a conservative seed source for the first direct Agent ACL; direct Agent ACL/title state evolves
independently after activation.

The next Phase-1 slice can therefore be the one exclusive Agent-state+ACL owner migration/cutover:
seed every exact Agent binding under write/schema exclusion, preserve the legacy initial revision,
freeze title/owner/lineage/access into the new owner, and switch Agent-facing title/ACL/revision reads
and writes atomically. Legacy Session metadata may remain for backing compatibility but must stop
defining public Agent state. Only add cursor/projection-head mechanics when a real consumer requires
them; do not prebuild Phase-2/4 engines.

Current focused evidence: StoreScope **2/2**, mixed Job exposure/root lookup green, coordination
**24/24** including killed-process result-before-attach recovery, restart unknown-model recovery
green, Actor Input **2/2** plus restart-durable admission regression green, Agent
Job-event/idempotent-subscription test green, delegated child relation/budget tests green,
stable-Agent survive/continue/close regression green, `aw-cli` all-feature check green, and strict
all-target/all-feature `aw-store` Clippy/fmt/diff green.

The user authorized this publication/repin/proceed sequence. Do not tag/release/deploy without a
separate explicit request. Use dotlink---l5 only; never touch `.agents/plans/ux/**`; 017
sandbox/mount/config-v13 remains frozen.

> The dated sections below are preserved implementation chronology. When their old status wording
> conflicts with the snapshot above or the canonical comprehensive handoff, the current snapshot +
> live source + sealed SPEC win.

## LIVE DL-200 R6 shared-Process cross-repo checkpoint — published source, 2026-11-21

The Phase-0 Process ownership cut is now published across both repos:

- AW runtime pin `7c16708...`: `Occupancy`, minimal `aw-process`, weak-Unix Unknown semantics,
  sole async Scope/Child/stdin/pipe/wait/stop/reap/drain ownership, semantic `stop_with`, one
  global drain bound, cancellation/deadline-preemptible interactive writes, and bounded reader
  quiescence that flushes captured partial sink batches before reporting incomplete output;
- AW main `59d67ba...` adds only the focused shared failure-boundary test; `aw-process` is **3/3**
  including `reader_stop_flushes_partial_sink_batch_before_incomplete`, strict Clippy green;
- dotlink source `16475e4` consumes `aw-process`, deletes `src/shell_process.rs` and all
  production `ManagedProcessGuard`/local Child/ChildStdin/stop-tree/drain ownership;
- dotlink `JobCompletion` stores the canonical optional `ProcessTerminal`; definite pre-spawn
  failure has no synthetic terminal. Occupancy projects compatibly as Empty=>no `tree`,
  Live=>`tree="leftovers"`, Unknown=>`tree="unknown"`;
- Live leftovers clear dotlink product `success` without rewriting factual exit/status into a
  generic tool error; weak-Unix escaped descendants remain Occupancy Unknown;
- direct + Bubblewrap Bash converge on non-login/non-profile `-c`; current dotlink main still has
  the historical pre-v0.17.8 30/35 bounded-turn projection. The target v0.17.8 convergence below
  supersedes it with one dotlink-owned 35m horizon / 10m report reserve / 25m command cap policy;
  AW remains horizon-free;
- dotlink product stdin authority/revision fencing remains local, but actual ChildStdin custody
  never leaves `aw-process`; initial stdin is chunked through the shared 64-KiB primitive.

Qualification stayed grouped: final dotlink all-target/all-feature source graph type-checks;
strict all-target/all-feature Clippy `-D warnings`, rustfmt and diff-check are green. The final
v0.17.6-era L5 executable rerun was green: shell filter **134/134** plus child-env **1/1**; Process
filter **18/18** plus its two process-related integration tests; Remote Start/replay **16/16**.
That remains valid evidence for unchanged semantics, but later SIGKILL attribution is amended by
the v0.17.7 audit below.

### Shared Process/release correction — AW + dotlink consumer source implemented locally, publication/platform gate pending

The current audited release tag is **v0.17.9** (`576d0a1`). Its exact `81ef891..v0.17.9` diff has
SHA-256 `1aa632e35d9c…`, identical to the staged candidate already imported into this plan. Its
underlying v0.17.7 Process fix (`64e3bd8`) is *stop a retiring Tokio worker from killing the
sandbox*. Bubblewrap `--die-with-parent` uses Linux
`PR_SET_PDEATHSIG`, whose parent is the **forking thread**, not merely the process. A command
spawned on a Tokio worker can therefore receive SIGKILL when that worker retires even though
dotlink remains alive. This matches the long-running L5 symptom pattern closely enough that prior
"external host SIGKILL" wording is withdrawn: those interrupted compile/link runs are
**inconclusive infrastructure failures**, not evidence of an outside sender and not AW test
failures.

Current uncommitted AW source now implements the sealed correction without sacrificing Tokio's
lightweight scheduling. `aw-process::Process::spawn` is async; Linux non-PTY launch crosses one
private bounded process-lifetime Spawn Facility with one stable lane initially. `Process: Send`
remains an O(n) movable async lifetime; only spawn + exact `DirectScope` attachment runs on the
stable lane. `block_in_place`/`spawn_blocking` are not used as lifetime anchors.

The implementation transfers private RAII `Spawned { DirectScope, Child, pid }`; a vanished reply
cleans the exact scope/child, lane panic poisons future launch admission without exiting the anchor,
and the lane never retires. The runner's absolute execution deadline is reused across Spawn-Lane
queueing + Process wait; a level-triggered cancellation probe is rechecked on the lane immediately
before fork, so stale pre-spawn cancellation creates no ProcessTerminal/StopRecord. Linux
`DirectScope::ready` now uses strict parent-owned non-reaping `waitid` with no ECHILD→pidfd-poll
cleanup fallback. Stop evidence is reconciled against physical exit before causal projection.
Windows source uses `CREATE_SUSPENDED | CREATE_NO_WINDOW` and a private Job termination marker;
`aw-process` contains the async interruptible occupancy-settle path. Native Windows execution is
not qualified on L5 because this toolchain has only the Linux target installed.

The dotlink-specific pieces remain at its edge: caller-vs-connector cancel origin, lifecycle prose,
`by=external` wording and wire projection. An unclaimed signal means only "no recorded shared stop",
never proof of an outside sender. The frozen 017 Sandbox/mount work and future PID-1 backend remain
unpromoted.

### v0.17.8 + post-release dotlink convergence — planned after AW Process repin

The currently audited release checkpoint is exact **v0.17.9 (`576d0a1`)**. Its committed
`81ef891..v0.17.9` diff SHA-256 is `1aa632e35d9c…`, exactly matching the staged candidate previously
audited. The release branch may continue moving, so the future dotlink-main port must still re-diff
from this exact tag before editing. At the actual dotlink-main convergence step, first re-diff
the then-current read-only `release/0.17` from this checkpoint and classify every new delta by
owner: shared Process/Job factual semantics go into AW first; transport/product policy and wire/
operator projection stay in dotlink over AW facts; frozen sandbox/config changes remain deferred
unless separately promoted; relevant tests/docs are reused. Never blind-cherry-pick a release
runtime implementation into main.

The v0.17.8/v0.17.9 dotlink-only bounded-turn refinement is now **implemented locally on dotlink
main against the existing published AW pin**, still as projection over shared Job facts rather than
another Jobs/Process owner:

- one local `RuntimeSurfacePolicy` derived from `ActiveTransports.constraints()`;
- bounded turn = 35m, with 10m reserved for reporting/hand-back, so new command cap = 25m/1500s;
- `sync_hold.is_some()` enables the bounded surface; local-only stdio/HTTP stays uncapped unless a
  transport itself declares a bound;
- one policy source derives server instructions, effective shell ceiling/schema/validation, startup
  warning and near-deadline `jobs_get.next`;
- effective new-command ceiling/transport constraint participates in execution-authority
  fingerprinting; current advisory capability is rebound on every machine construction and never
  rewrites an existing Job's admitted timeout;
- v0.17.9 fixes the advisory clock: `created` remains Job-age/`elapsed_ms` (including product
  queueing), but the runner publishes one absolute `execution_deadline` when it enters Running;
  Queued has no deadline advice and `jobs_get` counts down from `execution_deadline - now`, never
  `timeout_secs - elapsed_ms`. Advice still appears only inside `min(timeout/5, 5m)` on the bounded
  surface and is absent from list/read/ack/cancel and terminal/unknown/cancelling states;
- AW `ReceiptProfile` supplies factual `ts` only for dotlink (`checkpoint_minutes=None`) so generic
  AW checkpoint prose cannot conflict with exact 25/35/10 product guidance;
- if the future shared Jobs adapter blocks this composition, factor only a narrow typed factual
  Job-status projection seam. No rendered receipt parsing, arbitrary callback/plugin, AW clock/turn
  policy, or duplicate dotlink lifecycle state.

The post-release `81ef891` delta plus v0.17.9 refinement is **shared Occupancy correctness**,
not dotlink presentation. Windows `CREATE_NO_WINDOW` can leave transient console/job infrastructure
counted briefly after the main process exits. AW therefore performs bounded post-terminal Job-
accounting settling before publishing pre-cleanup Occupancy: zero at any sample => Empty; query
failure => Unknown; nonzero for the full **uninterrupted** reference bound (<=2s, ~10ms cadence) =>
Live. Cancellation or execution deadline preempts the grace and sweeps immediately; unless Empty
was already proven, the interrupted observation is Unknown and the main physical exit remains
unchanged. `aw-platform-win` owns only the cheap synchronous exact accounting query; `aw-process`
owns the async settle/cancel/deadline race. No conhost/image-name exception and no blocking Tokio
sleep. Dotlink later consumes that shared fact and retains only its `tree`/`success` compatibility
projection.

v0.17.9 (`576d0a1`) also tightens **shared Spawn Facility admission**: when a runner enters
Running it establishes one absolute execution deadline before Spawn Facility admission. A request
queued behind another lane may become stale; cancellation/expiry is rechecked on the stable lane
immediately before fork through a private linearizable launch gate. Stale-before-`Launching` means
no child and therefore no ProcessTerminal/StopRecord/kill claim. Once `Launching` wins, any real
child follows ordinary RAII/Process settlement. Spawn-Lane queueing consumes the same deadline;
pre-Running product queue age does not.

**Local qualification:** `aw-process` 7/7; `aw-platform-unix` 18 executed + 1 explicit cross-UID
ignore; `aw-tools` 126/126; adapter Process 27/27 using the real Nix Bash; focused foreground shell,
shell cancellation, Graph ShellNode cancellation and Graph nonzero-exit smokes all green; root
`aw-cli --tests` type-check green; strict affected all-target/all-feature Clippy, rustfmt and
diff-check green. No Windows target is installed on L5, so no native/cross Windows pass is claimed.

**Dotlink product-policy source checkpoint:** against the current old AW pin, Jobs 121/121,
shared-native 15/15 and Graph 13/13 are green; focused shell queue/timeout/schema tests and bounded-
turn/deadline tests are green; full test graph type-checks; strict all-target/all-feature Clippy,
fmt and diff-check are green. The exact Running/Start checkpoint now establishes the local absolute
execution deadline; local surfaces remain uncapped/quiet and hand-back surfaces derive one
25m/35m/10m policy source.

### Phase-0B compact replay/lifetime checkpoint — complete locally

AW's indexed `policy_admission_current` remains the sole hot replay owner: the immutable replay key,
actor, canonical target, original request digest and revision stay in indexed columns; after a
consumed effect crosses a durable recovery boundary, the full current-row detail compacts best-
effort to a versioned <256-byte never-retry tombstone. Immutable audit retains the full evidence.
A failed compaction leaves the full row and never changes the effect result. Legacy
Consumed-without-Start remains full until Remote Start commits, preserving exact recovery.

Grouped qualification extends the existing policy-admission owner test: the first consume returns
full evidence, explicit post-boundary compaction retains exact identity/target/route, replay cannot
become executable under changed policy/authority, and a fresh admission still succeeds after 8,205
unrelated Session audit rows. AW Remote Start owner tests are 5/5; policy-runtime replay tests 4/4;
root strict all-target/all-feature Clippy, test type-check, fmt and diff-check are green.

Dotlink's existing in-memory `remote_starts` owner now follows the same rule without a new ledger:
full `Mcp`/`JobMcp` receipts retain ordinary TTL, then compact to `RemoteStartReceipt::Compacted`
instead of deleting the authorization-id claim; Unknown is already a payload-free tombstone. Claim
lookup remains HashMap O(1), full-receipt retention compaction is amortized through a FIFO, and
payload-free tombstones do not consume the 4,096 active/full receipt budget. Current disclosure
still gates replay projection: filesystem/shell may return a generic compacted acknowledgement only
under current source/tool disclosure; Job/Graph controls without a retained exact Job ref return the
opaque withheld acknowledgement. Consolidated dotlink `remote_start` tests are 16/16 and strict
all-target/all-feature Clippy/test-graph/fmt/diff are green.

R6 Event/Job capacities remaining in source are live-resource bounds, not lifetime-history limits:
Agent Job Events caps current subscription count; finite Job Exposure counts only current
reserved/published rows in one generation/project; policy/Remote Start limits are per active Turn.
They are intentionally retained.

### Dotlink shared-Process consumer cut — implemented locally against local AW

Dotlink's sole shared Process launch now awaits `aw_process::Process::spawn` and passes the exact
Running/Start deadline plus a level-triggered cloned `JobCancel` through `SpawnControl`. Lane-side
`CancelledBeforeSpawn` and `TimedOutBeforeSpawn` project directly to product cancelled/timed-out
results with `process_not_started=true`; they carry no `ProcessTerminal`, stop evidence, signal or
synthetic Process fact. Definitive OS spawn refusal remains no-Process Failed. Other post-launch/
setup uncertainty remains Unknown as before. The internal old `spawn_failed` boolean is generalized
to `process_not_started`, and shared settlement uses ordinary status settlement when no Process
existed instead of calling `settle_process`.

Focused consumer qualification uses the real process-global Spawn Lane: one test briefly holds the
lane, lets a second Job reach Running and queue, cancels it, and proves no marker command executes,
product status is Cancelled, caller origin is preserved and no ProcessTerminal exists. The timeout
sibling uses the same projection helper without monopolizing the global lane. Against the local AW
checkout, full shell tests are **86/86**, Graph **13/13**, canonical spawn-refusal settlement is
green, full test-graph check is green, and strict all-target/all-feature Clippy/fmt/diff are green.
Validation temporarily substituted local AW paths and restored Cargo.toml/Cargo.lock exactly; the
manifest still pins published `7c16708`, so the current combined source intentionally requires the
future AW publication/repin before an ordinary dotlink build can consume this API.

A real Bubblewrap smoke was explicitly attempted (`DOTLINK_TEST_BWRAP=1`) and reached the installed
`bwrap`, but L5's outer tool sandbox rejects mounting `/proc` with `Operation not permitted`. This is
an unavailable host capability, not a passing/failing Bubblewrap product qualification. AW's generic
`PDEATHSIG` parent-thread regression is green; real-Bubblewrap worker-retirement remains to run on a
host that permits Bubblewrap mounts. Windows-native qualification likewise remains unavailable on
L5.

**Next coherent sequence:** when explicitly authorized, publish AW and repin dotlink to this exact
shared source; re-diff release/0.17 from exact v0.17.9; rerun the already-green consumer gate under
the real pin, plus real Bubblewrap worker-retirement on a suitable host and Windows-native
qualification. After that review, Phase 1 is the next implementation phase.

Author: coordinating design/implementation agent
Owner: @pvl
Updated: 2026-11-21
Source authority host: **L5 only**
Canonical checkout: `/home/pvl/spaces/abird/src/aw`

> Authority order: live source/tests/executed results > direct user instruction >
> active plan/TODO/spec > dated comprehensive handoffs.
>
> The November 15 comprehensive handoff is historical context for the Workspace
> continuation. It is not the current Git-state record.

## LIVE DL-200 checkpoint — original policy ToolCall replay, 2026-11-21

Read `IMPLEMENTATION.md` Phase 0B for the owning contract and unresolved gates.
The existing indexed `policy_admission_current` owner now finds accepted ToolCalls by
immutable `(session, branch, Turn, call ID)` **before** resolving contextual refs or
selecting current policy. The original operation+arguments fingerprint detects changed
request identity, while mutable writer/policy/participation epochs remain admitted
factual evidence. Exact replays retain the original canonical target after reparenting.
A current participant `view` check gates replay lookup; fresh calls still require
`submit` plus the real writer/tool/provider/payer authority. Reusing the same call ID
for ambiguous legacy targets fails closed, and no additional replay ledger was added.

Validation on L5: `aw-store --test store policy_admission` **2/2**; the filtered
production `aw-cli` policy-operation router integration **1/1**; `cargo check`
for AW Store/CLI green; strict affected all-target Clippy **green** after two
behavior-preserving `needless_return` cleanups in the in-flight Unix Process
observation code; policy-owner rustfmt and `git diff --check` green.

### Phase-0B replay payload disclosure checkpoint — cross-host owners composed, uncommitted

Replay identity and payload disclosure are now separate at the correct owners. AW revalidates
current **Session View audience** plus exact **ToolBinding + Actor Envelope source/effect/site**
before asking for a retained remote receipt. Dotlink then independently revalidates the concrete
target-side disclosure before releasing cached content: current issuer trust, current prepared
filesystem mutation capability for canonical native FS requests, current shell ToolPolicy, or
current Job-control ToolPolicy plus exact retained Job availability. Lease expiry and old runtime
generation still block only new effects; they do not erase a factual replay. If either host loses
current disclosure, the caller receives only one generic non-retryable prior-invocation
acknowledgement—no target/ref, Job ID, output size, timestamp, provider/site metadata or protected
payload—and the replay claim remains consumed.

No cross-host preflight protocol, Tool trait hook, replay service or table was added. AW adds only
zero-cost `SourceKind::as_str()` reuse plus its existing source/audience projector. Dotlink reuses
its existing `ValidatedRemoteStart`, prepared `CapabilitySet`, canonical request `access_paths()`,
`SecuredFilesystem::preflight_mutation`, `has_tool_for_policy`, and `JobManager::standalone_status`.
The filesystem replay path performs only authority resolution/preflight: no payload read, lock,
stage or commit. This keeps concrete authority on the machine that actually owns the resource.

Validation on L5: AW production bin check is green; prior policy owner **2/2** and AW Remote Start
owner **5/5** remain green. Dotlink compiles, grouped Remote Start/replay is **16/16**, strict
all-target/all-feature Clippy `-D warnings` is green, and rustfmt/`git diff --check` are green. The
existing routed AW integration binary remains subject to the unresolved L5 SIGKILL during linking;
after the v0.17.7 audit this is treated as an unattributed/infrastructure interruption, not a
proven external-host kill, and the test was not fragmented into replacement fixtures.

### Phase-0B participant Provider/payer checkpoint — complete, uncommitted

Fresh policy admission now reselects the authenticated participant through one shared
`ProviderAccounts::select_exact(actor, identity)` primitive. Full `InvocationIdentity` equality
covers account owner, **payer**, custody/version, executor, model and grant revision; there is no
Session-owner account/payer fallback. Replay lookup still happens first, so a factual consumed
operation never becomes fresh merely because provider authority changed. Guardian/delegate paths
reuse the same exact selection, and the service-less remote router receives the same
`ProviderAccounts` owner directly.

Policy delegation now separates the two principals that were previously conflated: the parent
Session owner owns the delegation root/child Session/journal/lifecycle, while the initiating
participant owns provider/payer/model/tool identity. The participant path is deliberately narrow:
current `Submit` is rechecked transactionally, depth is exactly 1, child routing/constructor replay
are forbidden, and recursive widening remains unchanged/denied. Store model-attempt budgeting
binds the paid identity to the child relation's stored `InvocationIdentity` rather than assuming it
matches the Session owner. Lifecycle mutations authorize the resource owner only. The child Actor
attach path independently rechecks participant `Submit` and the exact ProviderSelection.

Focused evidence without new fixtures: account publication/revoke/regrant **1/1**; Store
participant child creation → running → Bob-identity model-attempt → settlement + revoke denial
**1/1**; policy admission replay **2/2**; `cargo check --offline --locked -j1 -p aw-cli --tests`
green; strict affected all-target Clippy `-D warnings` green; fmt/diff green. The existing
end-to-end policy-router test includes Bob's own payer/provider, revoke/no-fallback and paid
delegate assertions and type-checks, but its root test executable still receives an unattributed
SIGKILL while linking on L5 before assertions. The v0.17.7 parent-thread bug is now a plausible
infrastructure cause, so this is not classified as an outside-host kill and the test was not split
into replacement fixtures.

**Remaining Phase 0B:** compact replay tombstones + removal/qualification of former lifetime audit
ceilings. No commit/push/release.

## LIVE DL-200 implementation checkpoint — Phase 0A, 2026-11-21

Current AW source checkpoint: `c7cc948` (`Checkpoint DL-200 Phase 0 owner hardening`).
It preserves both the pre-R6 Phase-0A Process slice and the independently developed
Phase-0B owner-hardening slice. Phase-0A Process state:

- added shared `Tree::{Empty,Live,Unknown}`;
- `ProcessOutputMeta` carries per-stream completeness; aggregate completeness is derived;
- `ProcessTerminal` now retains `exit + Option<StopRecord> + tree + output`;
- `ProcessReceipt` now stores one ProcessTerminal directly; Outcome/completeness/status/stop
  helpers are derived projections;
- `PipeProcess` owns one private `Scope`; it no longer owns Child + PID + guard separately;
- direct-host fallback is explicitly `aw_platform::DirectScope`: Linux prefers pidfd and falls back safely to the still-unreaped child PID plus process group; Windows owns duplicated leader HANDLE + Job Object; macOS owns child/group identity;
- no public/free readiness API, no higher-layer disarm/release, and the bounded final retry is owned by DirectScope; Darwin EPERM is accepted as NoTarget only when the leader is terminal and remaining scope is proven empty;
- Linux direct observation remains a fallback proof path (same PID namespace + unfiltered
  procfs, zombies excluded, live threads counted), not the target architecture;
- strong Linux target is a tiny AW PID-1 supervisor in a per-command PID namespace with
  Bash/main command as PID 2, instantiated by a real host/Sandbox launcher when promoted;
- Sandbox and Scope are explicitly orthogonal: Sandbox is the durable logical execution
  environment; a fresh private Scope owns one Process lifetime. Unsandboxed/direct remains
  first-class; a Sandbox may enable stronger containment without changing ProcessTerminal;
- sandboxed does not imply PID namespace, Scope backend choice is private/host-owned, and
  launch prefers prepare environment -> prepare Scope -> launch into Scope -> publish;
- `Tree` is immutable pre-cleanup occupancy; cleanup never rewrites it. `Tree::Empty` is
  backend-independent and means the full semantic Process scope is proven empty; direct
  Unix negative group/procfs observation becomes `Unknown` whenever escaped descendants
  cannot be excluded;
- tree facts are not yet projected into compact result strings;
- direct fallback settlement remains non-reaping where needed and settles scope before drain;
- redirected background children are factually `Tree::Live` and are killed before drain;
  inherited-pipe output now quiesces instead of timing out incomplete;
- a launch requirement for closed descendant containment must reject before effects if no
  closed backend is available; never silently downgrade to DirectScope;
- PID-1 supervisor/report loss before the main-command exit fact yields
  `ProcessExit::Unknown`; namespace cleanup never masquerades as the main exit or rewrites
  Tree;
- dotlink Bash/runtime-horizon/result projection is **next and not started**; dotlink must
  consume the same shared Process lifetime owner/settlement semantics, not keep a parallel
  guard/wait/reap runtime and merely copy fields.

Focused green evidence below predates the latest stronger Tree qualification and is **not**
Phase-0 acceptance. Two explicit implementation gates remain: current direct-Unix
`Ok(false) -> Tree::Empty` semantic mapping must become Unknown for weak negative evidence,
and the real dotlink consumer must compile against the shared Process owner without pulling
the full Agent adapter solely for lifetime logic.

Focused evidence:

- affected shared crates compile, including `aw-platform-win`;
- Unix managed-process tests 4/4;
- aw-tools runner tests 4/4 with `full`;
- strict affected all-target Clippy is green;
- DirectScope tests 4/4 and adapter regressions green for redirected-background cleanup, inherited-pipe cleanup, external exit vs connector stop, external signal identity, cancellation, deadline and terminate-once/SIGTERM;
- `git diff --check` clean.

Workspace check note: low-parallel `cargo check --workspace` reached root `aw-cli` and hit
two pre-existing Rust-2024 let-chain expressions in `src/subagents.rs`; those unrelated
files were restored/left untouched and are outside this Phase-0A diff.

Concurrent-work guard: `crates/aw-store/src/job_exposure.rs` became independently dirty
during verification with substantial Job-exposure changes unrelated to Phase 0. It was
not edited/reverted by this work and must be preserved.

## LATEST DL-200 dependency/owner review refinement — 2026-11-21

The external review was accepted with one stronger Process rule:

- Phase 3 now lands the one private replay-safe prepare/publish/reconcile construction
  primitive before `checkpoint.restore`; Phase 4 exposes/reuses it for host/model Agent
  constructors rather than creating another path;
- `Tree::Empty` retains its strong semantic Process-scope meaning. Weak direct-Unix negative
  membership evidence is `Unknown` when escaped descendants cannot be excluded;
- final typed leaf registries are introduced at first semantic use (`EventType` in Phase 2;
  SourceDriver/SandboxDriver/VaultProvider in Phase 3) and Phase 5 composes those same
  owners into ExtensionRegistry without migration/shadow registries;
- dotlink must reuse the same shared Process lifetime owner and is behaviorally qualified,
  not merely field-compatible;
- nested Agents remain allowed within existing bounds, but direct/Agent-mediated/descendant
  nested `batch` stays denied transitively while active Graph-node ownership exists. A later
  independently admitted Turn after release regains ordinary top-level batch eligibility;
- `agent_state_current` is authoritative durable Agent state after one-time **exclusive
  store migration**: complete Agent+ACL seed and new owner/schema epoch activate atomically
  under write exclusion; v1 has no online dual-write/catch-up. SessionStore remains
  Turn/history owner and only Agent lookup/exposure indexes rebuild;
- AW transaction commit is the sole constructor visibility point; post-commit wakes/caches
  are reconstructible accelerators only;
- replay owners compact full detail to minimal never-retry tombstones; replay without current
  disclosure returns only minimal/opaque non-retry acknowledgement and never fresh effects;
- Event cursors/subscriptions/Handlers bind StoreId + **authority-scoped** placement
  generation: hidden Domains cannot perturb observable staleness; authorized cross-store
  scope stales/lags explicitly and progress is revalidated at commit;
- Event projection progress is fenced by one contiguous indexed-through commit position;
  consumers never advance across a missing committed Event projection;
- EventInput acceptance is the one delivery linearization point: subscription generation,
  store affinity, current source/recipient/disclosure authority, dedupe and causal charge are
  checked with input admission; unsubscribe/close/revocation races use transaction-winner
  semantics. V1 Agent delivery requires Event/subscription/causal owner + recipient Input
  owner in one transactional store; cross-store Agent delivery is rejected. Authorization is
  source-first: proven hidden source skips privately without deeper manifest/provider work.
  For an already-visible source, externally mutable disclosure authority must be immutable or
  provider-fenced through acceptance; otherwise `events.read` returns candidate-oblivious
  retryable `authorization_unavailable` with unchanged cursor and subscription/Handler holds
  position with bounded backoff;
- repeated `events.subscribe(..., enabled=true)` on an active same-subject row preserves
  generation/baseline/backlog; lagged re-enable explicitly creates a fresh baseline;
- v36 delivery migration sets `delivery_cutover_rev = current next_event_rev` inside the
  serialized source-owner cutover; entire revisions are owned by one path. Already accepted
  legacy Inputs retain identity and gain immutable compatibility disclosure/provenance
  sidecars; incomplete provenance is conservative `legacy_protected`, while only pending
  pre-cutover deliveries bridge once;
- early leaf registries use the same private staged mutation primitive from first use, so
  Phase 5 composes them atomically without wrappers/migration;
- multi-resource constructors durably claim the existing operation receipt as Prepared
  before the first external/materializing effect; **capture and restore share the same
  prepared SourceDriver operation invariant**, with descriptor pinned to exact driver
  registration + recovery schema. Named sources are bounded (default 128, hard 1024) and
  durable descriptors are non-secret opaque IDs/digests/proofs/handles only. Crash/replay
  inspects/reconciles that candidate instead of blindly repeating effects; live bindings/
  checkpoints/unresolved descriptors block incompatible driver removal/reload, and store-open
  validates persisted dependencies before source effects;
- final publication revalidates all AW-store-owned source/destination/grant/ACL/lifecycle
  authority **inside** the publication transaction and holds AW-owned source fences through
  commit. External evidence must be immutable/non-revocable for the commit interval or a
  provider-enforced fence/lease held valid through the entire AW commit; a pre-commit
  `valid()` check is insufficient. Otherwise the operation stays Prepared/Unknown.
  Revoke/replace-first denies; publish-first remains factual;
- replay guarantees stable effect/created-resource identity rather than indefinite payload;
  ToolCall tombstones follow retained Agent/history lifetime and host-RPC tombstones require
  irreversible request-namespace retirement before deletion;
- `jobs_list` is exposure-store-local and gap-safe. Page one captures publication high-water;
  exact order/cursor key is `(publication_position DESC, canonical_ref ASC)`. A private
  rebuild captures H and, under the final write/swap fence, must verify current head == H or
  catch up `(H,current_head]` before activation; a stale replacement cannot erase newer
  rows. A resumed tree
  frontier is navigation only: every Agent ancestor gate is reauthorized and a now-hidden
  parent discards its entire queued subtree even if a descendant is directly viewable;
- DisclosureManifest is a persistent bounded-fanout content-addressed set/DAG: exact
  provenance facts remain authoritative, while ordinary disclosure checks deduplicate over
  structurally shared stable authority roots with bounded per-request work. An authority
  root is dependency/maximum-ceiling identity only; it identifies the live policy/ACL/group/
  provider owners to recheck and never caches current permission except for an explicitly
  immutable export grant contract;
- the future PID-1 report channel is unidirectional supervisor→host, command descendants
  inherit no writable endpoint, and the host validates framing/launch identity/exactly one
  terminal report so user code cannot forge ProcessTerminal facts;
- Event read privacy promises no direct hidden identity/subject/exact-count/private-position
  disclosure, not constant-time/constant-volume behavior; bounded scans may still permit
  coarse aggregate traffic/timing inference.

## LATEST DL-200 design refinement — 2026-11-20

This section supersedes older DL-200 design summaries below where they conflict.

- one public host constructor: `agent.create(from=Agent|settled Turn)`; no `agent.fork`;
  trusted in-process library code may use typed HistoryBaseRef as a lower primitive;
- model ancestry selector is `agent(..., from=...)`;
- `checkpoint.restore` remains first-class because it restores exact history **plus**
  exact SourceSet into a new Agent + new Sandbox;
- checkpoints capture a plural named typed `SourceSet`; Git is the mandatory first
  SourceDriver and versioned/content-addressed object/bucket manifests fit the same
  fail-closed driver contract;
- Sandbox is canonical at `/d:/i:/p:/s:<sandbox>` as a Project-level sibling of the
  Agent execution branch; it never enters Jobs/control `ExecutionRef` grammar;
- Sandbox is logical mutable execution state. An Agent chooses one Sandbox at creation and
  does not live-rebind in v1; Agents can share one, branch one, or start fresh by creating
  a new Agent; one fresh private Process Scope remains per dispatch;
- named source attachment is only `sandbox.sources.list/set`; SourceRef never carries
  credentials, which remain Vault authority;
- same Sandbox does not imply same authority;
- Agent-scoped VaultGrants project selected provider-backed secrets/services as
  broker/socket/ephemeral-file/env capabilities; Vault authority is never inherited from
  history/Sandbox/checkpoint/ACL state;
- system Event names are bare; custom Events are structurally `#name`; Event routing
  reversibly encodes literal `.` in canonical handles as `%2E`, so resource refs need no
  grammar weakening;
- Events expose stable opaque `Event.id`, durable causal root/hop budgets and internal
  immutable DisclosureManifest refs; read, Agent delivery and durable Handlers all enforce
  exact source + disclosure closure; host also has bounded `events.subscriptions.list`;
- one plain `Extension` registers through one typed `ExtensionRegistry` composition root;
- typed subregistries include Tool/Hook/Action/Resource/View/EventType/EventHandler/
  Provider/ProviderHook/ProcessHook/UI/Setting/State/Compactor plus native SourceDriver/
  SandboxDriver/VaultProvider registries;
- native Rust registration preserves concrete types and may return typed handles; stable
  Extension/local IDs + schema versions own durable/wire identity, never Rust TypeId;
- all contributions from one Extension registration publish atomically across registries,
  but registered/installed definitions are inert until explicitly activated/bound by host
  authority; typed handles stale on their own registration/schema, not unrelated reloads;
- runtime callbacks still use scoped `ctx.tools`, `ctx.state`, `ctx.ui`, `ctx.models`, etc.;
- Context customization is Hook(Context), dynamic Tool visibility/calls use `ctx.tools`,
  Views are pull/read facets, EventHandlers are durable push/background reactions;
- Histate can register fully typed domain Actions/Views/Handlers/UI/Tools/Hooks without
  converting its Rust domain model to arbitrary JSON;
- dynamic Extensions reuse/generalize the existing `aw-extensions` child-process path: one bounded full-duplex stdio protocol projects the same registries and scoped `ctx.*` capabilities;
- Node/Bun/Deno/Python/Rust/etc. participate as ordinary executables; AW owns no npm/pip/cargo marketplace/resolver in v1;
- do not embed a JS runtime or Wasmtime for v1. A WASI Extension may already run through an external `wasmtime run ...` process speaking the same protocol; embedded WASM remains optional later;
- Histate may own a product Session referencing many AW Agents + zero/many Sandboxes and a
  product checkpoint referencing one/more AW Checkpoints; cross-store writes use a
  product-owned idempotent saga/outbox, not AW distributed transactions;
- shared Process settlement is sealed as one small AW delta: `Tree::{Empty,Live,Unknown}`,
  per-stream output completeness and one `ProcessTerminal { exit, stop, tree, output }`
  truth; platform observation stays `io::Result<bool>` and settlement happens before final
  reap/drain;
- dotlink alone owns result/log projection, non-login Bash and the runtime horizon:
  ~30-minute coherent-checkpoint guidance + 35-minute hard conversational command bound;
- one authenticated mutation replay identity/fingerprint protects constructors/additive
  mutations without per-method request-id clutter; multi-resource construction uses private
  prepare -> atomic AW publication -> reconciled cleanup, so half-created resources are
  never public;
- clean and dirty Git checkpoint anchors are both privately retained; submodule contents
  require their own SourceBinding when they must be retained/reproducible;
- Vault `file/env/socket` projections are provider/host-defined safe profiles, never
  arbitrary caller mount/env injection; grant removal never pretends to erase already-
  materialized capability from a running Process;
- the frozen 017 sandbox/mount/config-v13 implementation plan remains frozen.

The canonical detailed authority is the DL-200 SPEC/TODO, not older summaries in this
handoff. The fresh full pre-seal audit and earlier naming/typed-registry/dynamic-host
reviews are clean: no duplicate owner/catalog/database/replay ledger, partial registry
publication, authority/disclosure leak, Event-log duplication, stringly native Histate
path, second transport/runtime stack or Histate/AW ownership conflict remains.

## LIVE published AW + dotlink repin/trust checkpoint — 2026-11-20

This section supersedes older current-state/pending-order text below where it
conflicts.

Current published boundary:

- AW shared source implementation is signed/published at
  `b8a46a942a56a5ae5cb7e2a8fad47010fde4d65b`
  (`Complete v36 runtime admission and process semantics`);
- dotlink consumes that exact AW source commit in all seven direct/transitive AW
  Git packages;
- dotlink signed source checkpoint is
  `648519add3d174b9e13de81aeca62431591dac6d`
  (`Repin AW and complete process-control hardening`);
- dotlink `main` is published through
  `bbd29c13904b7255eb28e2ed6c776c3590ac122a`
  (`Record AW repin and Remote Start trust cutover`);
- the read-only release worktree is currently local
  `release/0.17 = v0.17.6 = 6cd056672d9390b17ac591786253af5f04bfeef1`,
  3 commits ahead of the still-published
  `origin/release/0.17 = v0.17.5 = 3995448d7bf33794ef84c7bb51150d77cc260ada`;
- D4-D7/v0.17.5 semantic equivalence remains closed; the later v0.17.6 forensic delta is
  now captured by sealed DL-200 section 25.11 / implementation Phase 0;
- sandbox/mount/config-v13 remains frozen.

AW v36 completion published in `b8a46a94...` includes:

- explicit v35 -> v36 migration/open coverage and projection backfill;
- exact replay of historical issuer-less v35 Remote Start facts without
  manufacturing v36 issuer evidence;
- `RemoteStartIssuer { client_id, trust_epoch }` and
  `RemoteStartLease { target, issuer }`;
- issuer-bound v2 authorization digest for new Starts, with issuer-less v1 facts
  retained only for exact historical replay;
- issuer/lease-independent recovery of an exact committed Start before new
  mutable authority is consulted;
- indexed canonical Agent lookup with current Session authorization recheck;
- query-only `job_exposure_current` plus bounded keyset pagination for mixed
  Agent/finite Job listing;
- durable Agent/Turn source events, wake admissions and restart recovery;
- shared `ProcessTerminal::status()` projection where physical/semantic process
  outcome and output completeness are orthogonal:
  `Primary` derives from exit/stop evidence and
  `Status.incomplete = !output.complete`.

The dotlink consumer cutover is also complete:

- ordinary child exit immediately performs reasonless managed-descendant
  housekeeping;
- inherited stdout/stderr use a separate 250 ms post-exit drain grace;
- an escaped `setsid` writer cannot erase a known command exit merely by
  retaining an inherited FD;
- the temporary dotlink `shared_process_status` compatibility shim is gone;
  dotlink consumes AW `ProcessTerminal::status()` directly;
- revision-fenced public cancel requires canonical shared Job ownership;
- stdin authority and revision acceptance are linearized before bytes are
  enqueued;
- duplicate static MCP instruction ownership is removed.

Trusted AW Remote Start issuer enforcement is complete target-side:

- ordinary OAuth authentication remains distinct from AW-bridge issuer trust;
- private dotlink OAuth state persists one monotonic trust epoch per approved
  DCR client;
- owner controls are `oauth trust-aw-bridge` and `oauth untrust-aw-bridge`;
- lease issuance is 401 when unauthenticated, 403 for authenticated but
  untrusted clients, and returns issuer-bearing `RemoteStartLease` only for a
  currently trusted bridge client;
- the authorization issuer client ID must match the authenticated OAuth client;
- exact committed replay resolves before current issuer trust;
- revocation blocks a genuinely new Start but does not erase the original
  committed replay;
- re-trust advances the epoch, so an older lease cannot become current again.

Final repinned dotlink qualification on L5:

- main binary: **706 passed / 0 failed / 1 intentional ignored**;
- integrations: **42/42**;
- strict all-target/all-feature Clippy `-D warnings`: green;
- fmt/diff, Alejandra and Markdown (**0 issues / 86 files**): green.

Two combined all-feature test invocations received unattributed SIGKILL before
producing test output or Rust diagnostics. The complete main binary and all six
integration targets were then run serially and are green. At the time these were
treated as infrastructure noise; dotlink v0.17.7 later identified the retireable-
Tokio-worker parent-death bug as a plausible mechanism. They remain non-source
failures, but are no longer attributed to an external host.

Remaining boundaries are external/deferred only:

- native Windows Job Object runtime validation;
- native macOS runtime/deployment validation;
- trusted-selector Bubblewrap runtime smoke on a suitable host;
- frozen sandbox/config-v13 unless explicitly promoted.

No release/tag/deploy action was performed by this source publication.

## LIVE 0.17 fresh-review convergence checkpoint — 2026-11-19

This section supersedes older state summaries below where they conflict.

Current Git boundary:

- authoritative fresh-review AW implementation pin:
  `1a5d0f65208e4a05b737c804a55a3aaad3e2fa59`
  (`Relax shared MCP protocol dependency ranges`), signed and selected as
  `AW_IMPL_SHA`; it follows source checkpoint `7c7de6c9...` and prevents the
  protocol crate from downgrading compatible downstream Tokio/serde_json;
- previous source checkpoint `eb325b31...` is historical and must not be used
  for the dotlink repin;
- AW `HEAD == origin/main == 4234447612421069a746c925c76a25e3ba27774a`
  (`Record final shared 0.17 implementation pin`); final reusable implementation
  remains `AW_IMPL_SHA = 1a5d0f65208e4a05b737c804a55a3aaad3e2fa59`;
- dotlink `HEAD == origin/main == 4c3b96fd5ba9e6772bc159f10a3ad893c9729887`
  (`Record repinned D1-D3 convergence checkpoint`);
- dotlink D0-D3 are published/complete; D4-D7 are the active consumer queue;
- AW and dotlink working trees are clean at this handoff refresh;
- the read-only release reference is now fully published at
  `release/0.17 = origin/release/0.17 = v0.17.5 =`
  `3995448d7bf33794ef84c7bb51150d77cc260ada`. This worktree was not modified
  by this agent.

Fresh-review shared corrections:

- `StopReason::as_str()` is the single shared
  `timeout|cancelled|runtime_error` spelling;
- zero-cost `StopTracker` owns Unknown -> retry reason preservation;
- `ProcessReceipt::stop_record()` exposes the final factual logical stop while
  `ExecutionLifecycleEvent::ProcessStop` remains attempt-level diagnostics;
- v0.17.5 outside-signal projection remains product-only: physical
  `ProcessExit::Signal(n)` plus no reasoned final stop record may become one
  `LOG job exit ... from=outside`; no new AW outside/provenance enum is needed;
- `ProcessTerminal` remains minimal `{exit, stop, output}` terminal truth;
- typed Job/Node receipts now preserve `ProcessTerminal.stop`;
- Job receipt lifecycle distinguishes `revoking`, terminal `revoked`,
  `cancelling`, terminal `cancelled`, and ordinary `settled`;
- physical exit remains
  `ProcessExit::{Code(i32),Signal(i32),Unknown}`, with shared
  `aw_platform::exit_signal(&ExitStatus)` and `signal_name(...)`;
- `JobRef` remains flat/bounded with private fields and typed accessors;
- `RawPageBytes` and `SerializedResultBytes` remain orthogonal;
- bounded diagnostics and typed Job receipt grammar remain shared.

MCP projection is now split cleanly:

- **`aw-mcp-protocol`** owns generic MCP model shaping:
  result validation, 16 KiB result/block metadata bounds, replay-stable receipt
  timestamps, text-only serialized fitting, receipt sidecar reservation,
  heartbeat/progress projection and observer summaries;
- it depends on `aw-tools` with **no aw-tools features**, rmcp model types,
  `serde_json` and Tokio timing/macros — no Agent runtime, reqwest, jsonschema,
  clap or MCP transport stack; receipt decoration takes a feature-neutral
  copyable `ReceiptProfile` and no longer clones whole results just to reserve
  a visible marker;
- the full **`aw-mcp`** client adapter depends on and re-exports that surface;
  it no longer carries a second implementation;
- fitting never silently truncates: the generic marker is mandatory, and
  immutable typed/structured facts fail explicitly when they cannot fit;
- decoration first fits against the full envelope, so a visible stored `ts=`
  that survives fitting never pays an unnecessary sidecar reserve;
- receipt timestamp mutation preserves existing fields under
  `io.abird/receipt` and fails transactionally if the reserved value is not an
  object;
- at the 32-block MCP limit, an existing text block can carry the timestamp
  marker; an all-typed full block set fails explicitly.

Fresh AW validation:

- `aw-tools --all-features --lib`: **357/357**;
- `aw-tools-adapter --all-features --lib`: **255/255**;
- `aw-mcp-protocol`: **12/12**;
- `aw-mcp --all-features --lib`: **20/20**;
- `aw-platform-unix`: **13/13**;
- Unix live PTY integrations: **4/4**;
- cross-UID fixture: **1 intentionally ignored**;
- `aw-terminal --lib`: **20/20**;
- all relevant no-default/minimal checks: green;
- host-side `aw-platform-win` check: green;
- downstream `aw-host` / `aw-cli` and full workspace all-feature check:
  green;
- strict Clippy for core/adapter/protocol/client/platform/terminal: green;
- rustfmt and `git diff --check`: green.

The native Windows Job Object / exit-signal assertions remain authored but
unexecuted on L5 because its Windows Rust std target is unavailable.

Publish and atomic repin are complete. `AW_IMPL_SHA` is fixed at
`1a5d0f65208e4a05b737c804a55a3aaad3e2fa59`. The direct dotlink AW set now
pinned atomically is:

- `aw-tools`;
- `aw-tools-fs-unix`;
- `aw-tools-fs-win`;
- `aw-platform`;
- **`aw-mcp-protocol`** (not the full `aw-mcp` client adapter).

All direct/transitive AW Git packages in dotlink resolve to that exact SHA,
with no registry package/version churn and no path override. Frozen
sandbox/config-v13 work remains untouched and deferred.

## Latest AW-tools published checkpoint — 2026-11-18

- Shared-runtime implementation commit:
  `be6d48db552fd44b9dd85364cd8734454109ce68`
  — `Harden shared Job output and journal uncertainty`; strict-ACK follow-up
  `26a8c788d6dc7e0a7fe16995f8057fb332440978` remains the D1 ACK-compaction
  checkpoint. Writer-quiescence landed in
  `beef1bc2d2f37b5660ebe928d368bca37368e608`; the current shared metadata pin
  is signed/published `7e6fe20a95640e32610819ce94b61a121e165dea`
  — `Align shared Job list metadata`.
- Local `main` contains signed typed-media implementation
  `d6009deaea31abe0cdfbd87b4de48b0eaf9a0ba0` plus its owning-plan follow-up;
  `origin/main` remains published `7e6fe20a95640e32610819ce94b61a121e165dea`
  until a separately authorized push.
  Dotlink `main` is published at
  `5381729cd469eda54691e540deb3fee13e9f3e3e`
  (`Complete shared Toolset metadata adoption`) and consumes `7e6fe20` in all
  three shared dependencies/lock records.
- Git reports **Good ED25519 signatures** for the published AW commits using key
  fingerprint `SHA256:4aB+NvRb1shCx4xjtZDr2Y7FzfHGph80eFtpF+pHuCI`; L5 has
  no `allowed_signers` principal file, so local trust status is `U` rather than
  a principal-mapped `G`.
- Current implementation qualification: `aw-tools` **341/341**,
  `aw-tools-adapter` **251/251**, read-only shared **110/110**, strict
  all-target/all-feature Clippy, formatting/diff checks, minimal shared build,
  MCP, minimal CLI and full CLI all green.

## Typed MCP wire/media checkpoint — 2026-11-18

- Signed local AW checkpoint `d6009de` adds bounded `aw_mcp::McpToolResult` as
  the raw typed MCP result boundary. It preserves current RMCP Text/Image/Audio/
  Resource/ResourceLink blocks plus structured content, with 32-block and
  existing 128 KiB semantic-result bounds plus bounded metadata overhead.
- Existing `ToolOutput` and the Agent/provider conversation path stay text-only.
  The ordinary MCP `RemoteTool` path still rejects non-text content; this work
  does not implicitly grant provider media delivery, device/media consent or a
  new durable media transcript format.
- Real cross-repo qualification uses exact published dotlink `5381729` over
  stdio with a private temporary profile: `bread(format=mcp)` preserves a PNG
  image block (`image/png`), WAV audio block (`audio/wav`) and PDF embedded
  resource (`application/pdf`) through AW's MCP client. Synthetic regressions
  also prove non-text text-path denial and block/payload bounds.
- Current gates: `aw-mcp` **18/18**, strict all-target Clippy `-D warnings`,
  downstream `cargo check -p aw-cli --lib`, rustfmt and diff checks green.
- Remaining media work is explicitly **provider/model delivery**, durable media
  receipt/history semantics and consent—not MCP wire interoperability.

## Current dotlink D1–D4 security checkpoint — 2026-11-17

- Signed implementation commit `26a8c788d6dc7e0a7fe16995f8057fb332440978`
  — `Preserve strict output ACK replay across compaction` — contains the shared
  fix discovered by dotlink D1.
- Dotlink's first D1 delegation removed its duplicate shared delivery watermark
  and exposed one shared bug: strict `acknowledge_delivered_output` rejected a
  lost-response replay of an already-ACKed owner watermark after another owner
  legitimately compacted the physical retained prefix below it.
- The fix validates exact owner + delivery history first, accepts only an
  already-committed `through <= owner.through` replay, and still requires current
  retained bytes for every **new** ACK advance. Foreign owners and new ACKs into
  compacted-away bytes remain rejected.
- The owner-aware compaction regression exercises the strict API for both the
  replaying Job owner and independent Task owner. Full shared **339/339** and
  minimal Job-runtime **200/200** tests pass; strict all-target/all-feature
  `aw-tools` Clippy, format and diff checks pass.
- `beef1bc` is published and the current uncommitted dotlink D1–D4 worktree
  pins all three AW dependencies/lock records to it. D1 remains complete there: shared
  delivery proof + Through-ACK + omitted-offset Finish + aggregate
  `jobs_get.tail` all delegate to AW. Tail uses the shared 512-default / 1024-max
  combined stdout+stderr split/redistribution and remains a pure peek with no
  delivery/ACK credit.
- D2 is also complete without a production grep fork. Dotlink direct MCP and
  Graph-native grep already use AW's exact schema, `request::parse`,
  `direct::execute`, and `search::grep`. A real stdio regression qualifies exact
  files/directories, runtime context 0/8/128, rejection of 129 and unknown
  fields, asymmetric side overrides, pagination/resume, compact incomplete
  projection, and native `batch` parity.
- Dotlink's first D3 owner-policy review found one concrete host/shared mismatch:
  standalone shell Jobs could become native MCP Tasks while the immutable shared
  plan said `task_result_eligible=false`; dotlink then attached a private
  hash-derived Task owner outside AW's plan policy. The current dotlink tree now
  declares standalone Process Task-result eligibility in the sealed plan and
  derives the exact lease with shared `task_result_owner(id, 1)`. Native and
  shell Graph plans remain Task-ineligible; non-Task dispositions release the
  prospective lease only after terminal metadata fixes the retained prefix.
- Hosted physical-delivery semantics still require positive POST delivery plus
  replay commit; 404/expiry/ambiguous delivery does not ACK the inner request
  key. A second D3 review found a real old-owner leak on namespace/execution-
  authority rotation: outer replay state could become retired/uncertain while a
  hidden durable shell request key still pinned the inner Job/spool. Dotlink now
  records bounded shell-owner dependencies before dispatch and routes revoked
  generations through its existing `RetireShell → retire_job_request_delivery`
  queue. A real replacement-client regression proves the same execution is
  exposed/released without rerun, while ordinary mutations remain `Uncertain`.
- Optional `consumer_output_owner` leases remain intentionally unadopted because
  dotlink currently has no distinct authenticated output-consumer audience beyond
  canonical Job/model and native Task owners.
- Final D3 audit adds direct proof that Busy old-owner retirement stays queued
  until the active response attempt releases, then retires/exposes the same Job;
  positive `Delivered` tombstones remain final under later authority revocation
  even with a registered shell-owner dependency. Runtime replacement sequencing
  was re-read end-to-end: old transports are cancelled/drained before a new
  runtime/client is constructed, and cleanup failure blocks restart.
- D4 independent review found one additional dotlink-host bug outside AW shared
  semantics: a cached shell response whose live sandbox guard was revoked could
  become generic uncertainty while hidden discard was Busy, allowing later
  expiry to lose cleanup while the revoked request key remained. Dotlink now
  carries guard revocation through the same bounded shell cleanup queue in an
  explicit **discard** mode; Busy cleanup stays queued, revoked hidden work is
  cancelled/discarded rather than exposed, and ordinary old-owner retirement
  still exposes the same execution. A red regression fails before and passes
  after the fix; no AW source change was required.
- D4 all-feature host qualification is **684 passed / 1 ignored** in the binary
  plus **36/36 integrations**; strict all-feature Clippy/fmt/diff/Alejandra pass.
  Repo-wide dotlink Markdown is **0 issues across 76 files**. All Nix check
  derivations are green using command-local verified SSH + an immutable Nix-store
  CA bundle. The hermetic Nix test fixture now supplies pinned Bash explicitly and
  passes **684/0/1 + 36/36 integrations** with a verified store output. Native full
  Bubblewrap qualification is also **684/0/1 + 36/36**, and all seven targeted
  GPU/KVM/device-security cases pass on a capable host. D1–D4 are complete.
  Fresh post-repin Rust qualification on `beef1bc` is **684/0/1 + 36/36**,
  strict all-feature Clippy/fmt/diff green.

## Repository state before this review

- Branch: `main`
- HEAD / `origin/main`:
  `7d09d5f3c882894cd46b427a89fa75280223115a`
  — `Advance workspace views, remote resources and handoff`
- Previous AW-tools merge:
  `0d2b4e3`
  — `Build configurable toolset and harden shared Job control`
- The worktree was clean, nothing staged, with one worktree.
- `.agents/plans/ux/**` had no worktree or staged diff.
- Use `dotlink---l5` only. Do not use A1.
- Do not create another worktree.
- Do not commit/push/tag/release/deploy without explicit authorization.
- Keep implementation turns near 30 minutes and checkpoint coherently.

## AW tools merge review — 2026-11-16

The merged boundary remains sound:

- `aw-tools` owns canonical transport-neutral filesystem, Graph, finite-Job,
  Toolset metadata/limits and typed receipt semantics.
- `aw-tools-adapter` remains AW projection/compatibility glue. It does not own a
  second filesystem engine, JobManager, scheduler or authority source.
- `Toolset::only` constrains both direct filesystem registration and Graph
  native leaves.
- Tool metadata overrides are presentation-only: the thin described-tool wrapper
  delegates availability, validation, parallelism and execution to the original
  Tool and cannot change name/effect/schema shape/grants.
- AW `ToolMode`, selected tool names, ToolPolicy, execution site and live Actor
  Envelope remain independent positive authority gates.
- finite `jobs_get` output disclosure independently rechecks live
  `jobs_read` authority before retained bytes are appended.
- finite `jobs_ack` composes current `ToolBindingAuthority` and live
  `AgentStepGate` commit guards around exactly one synchronous JobManager
  ACK/Finish mutation. The established lock order remains
  ToolBindingAuthority → Actor runtime → JobManager.
- shared delivered-output proof credits only raw bytes actually represented by a
  successfully validated finite projection.
- Graph `unknown` remains dominant over mixed known outcomes; explicit Finish
  refuses unresolved effects.
- canonical grep parser/schema/engine remain shared by direct and Graph
  execution.

### Review fixes currently uncommitted

Only narrow merge-quality corrections were required:

- update four hex/digest parsing loops to the Rust 1.98 strict-Clippy-preferred
  `slice::as_chunks::<2>()`; behavior is unchanged and the repository MSRV
  remains Rust 1.88, where this API is available;
- replace stale hard-coded `jobs_get tail 0..1024` / `wait_ms 30 seconds`
  validation messages with truthful configured-ceiling messages. Enforcement
  already used the configured `JobProfile`; only the diagnostics were stale;
- reconcile the toolset-builder and Job-output/grep TODOs with the actual merged
  commit `0d2b4e3`;
- clarify in the dotlink-tool-port plan that its November 9 AW revisions are
  historical consumer pins, not evidence that dotlink already consumes the
  newer Toolset/Job-control merge.

### Still open — do not claim complete

The approved Job-output contract still has two intentionally unfinished
observation semantics:

1. finite `jobs_get.wait_ms` must become lost-wakeup-safe and event-driven,
   waiting **only for settlement or deadline**. Current code still polls at
   10 ms and can return on any revision change.
2. finite `jobs_read.wait_ms > 0` must wait for new bytes or EOF/settlement at
   a nonnegative absolute stream offset. Current finite-Job path still rejects a
   nonzero wait; negative offsets remain nonblocking snapshots.

These belong to
`.agents/plans/ai-native-tool-output-grep-2026-11-15/` and are the next shared
Job semantics work. Do not hide them by marking the whole plan complete.

Before dotlink adopts configurable grep work profiles above AW defaults, B6
should also decide/document explicit product-neutral shared grep safety maxima
rather than inventing host-independent caps ad hoc. AW itself already prevents
widening beyond its configured/default product ceilings.

## Current uncommitted review paths

Preserve exactly these review changes unless the user asks otherwise:

- `.agents/docs/CURRENT_HANDOFF.md`
- `.agents/plans/ai-native-toolset-builder-2026-11-15/TODO.md`
- `.agents/plans/ai-native-tool-output-grep-2026-11-15/TODO.md`
- `.agents/plans/dotlink-tool-port/TODO.md`
- `crates/aw-tools/src/filesystem/direct.rs`
- `crates/aw-tools/src/filesystem/request.rs`
- `crates/aw-tools-adapter/src/binary.rs`
- `crates/aw-tools-adapter/src/jobs_tools.rs`

Nothing should be staged or committed without explicit authorization.

## Validation at this checkpoint

Fresh review gate after the code fixes:

- `aw-tools --all-features --lib`: **327/327**;
- `aw-tools-adapter --all-features --lib`: **242/242**;
- `aw-agent-core --lib`: **90 passed, 1 ignored**;
- read-only Toolset tests: **9/9**;
- `aw-tools --no-default-features`: green;
- strict affected-crate all-target/all-feature Clippy with `-D warnings`: green;
- formatting: green.
- downstream `aw-mcp`, `aw-host`, and `aw-cli --lib` checks: green.
- `git verify-commit 0d2b4e3` reported a good ED25519 signature, but L5 lacks
  the configured `allowed_signers` file, so principal trust resolution is
  environment-incomplete rather than fully verified here.

Run the final stable-tree diff/staging/UX audit before handing off.

## Other active work

Workspace/remote-view state moved in `7d09d5f`; use the live
`.agents/plans/workspace-views/TODO.md`,
`REMOTE-DEVICE-TRUST.md`, and `NEXT-AGENT.md` for that continuation. Do not
infer its current gate from the older November 15 snapshot prose.

Release-readiness and platform-native external gates remain separate from this
AW-tools review.

## 2026-11-17 AW–dotlink tools handoff update

The newer, comprehensive **shared-toolset/Job-output** handoff is [AW-DOTLINK-TOOLS-FULL-COMPREHENSIVE-HANDOFF-2026-11-17.md](AW-DOTLINK-TOOLS-FULL-COMPREHENSIVE-HANDOFF-2026-11-17.md). It supersedes this dated file for the post-merge Job wait, owner/ACK, journal and dotlink migration state. Keep this file for its original AW merge review and broader Workspace context; live code/tests and the active owning plan take precedence.

## 2026-11-18 current AW-tools revalidation

- Revalidated current `HEAD == origin/main == beef1bc` and reviewed the
  post-`0d2b4e3` shared changes through `be6d48d` and `26a8c78`, plus the
  current dotlink D1–D4 consumer paths. The older 2026-11-16 “Still open” wait
  bullets above are historical: settle-only `jobs_get` and byte/EOF-aware
  finite `jobs_read` are implemented and qualified in the active Job-output
  plan.
- Two adjacent shared removal defects were found red/green: a locally terminal
  Job with a still-live retained `OutputWriter` could be erased by
  `discard_unexposed_terminal`, and automatic exposed-capacity pruning could
  erase the same state when that live stream retained zero bytes. The published
  `beef1bc` fix applies the same writer-quiescence invariant already used by
  `expire_terminal` to both paths; after `OutputWriter::finish`, ordinary
  hidden retirement/capacity reclamation proceeds.
- `has_nonterminal_jobs()` now treats unfinished retained writers as live
  runtime work because AW uses it to fence parallel-policy changes and Session
  scope replacement. Execution terminality remains a separate factual state;
  this does not alter `Snapshot::is_known_terminal()`.
- Source change is signed/published in
  `beef1bc2d2f37b5660ebe928d368bca37368e608`
  (`Harden Job writer quiescence`). Preserve the existing dirty documentation
  changes in this file and the Job-output TODO; nothing else is staged.
- Final post-fix gate: `aw-tools` **341/341**, adapter **251/251**, Agent core
  **90 passed / 1 ignored**, read-only shared **110/110**, minimal shared build,
  strict affected-crate Clippy, `aw-mcp`, `aw-host`, `aw-cli --lib`, and
  rustfmt all green.
- Dotlink repin qualification on the exact Git dependency is also green:
  all-feature binary **684 passed / 1 ignored**, integrations **36/36**,
  locked all-target/all-feature check, strict Clippy, fmt and diff checks.

## 2026-11-18 B6 Toolset cross-product checkpoint

- Published dotlink `main` is now
  `4f343c3183d427e036a6cb1a9cc42c8a4f2c2cb4`
  (`Complete AW shared-tools convergence`). The current uncommitted B6 slice
  is repinned to published AW
  `7e6fe20a95640e32610819ce94b61a121e165dea`.
- The current dotlink B6 slice consumes one shared immutable `Toolset` for
  canonical filesystem, shell, batch and finite Job MCP descriptors.
  Runtime Job defaults now come from one `DOTLINK_JOB_PROFILE` matching the
  shared contract: tail 512/1024, waits 30 s, list max 256. Toolset does not
  replace dotlink policy/sandbox/replay/transport authority.
- Dotlink deliberately leaves `ReceiptProfile` disabled: no `ts` or
  checkpoint metadata is added until a typed shared receipt can carry factual
  status. Existing free-form MCP strings are not reparsed for presentation.
- B6 review found one shared descriptor gap: `JobsList.limit` had the
  configured maximum but not the runtime-universal `minimum=1`. Published
  AW `7e6fe20` now projects that lower bound while preserving stricter host
  minima; focused Toolset **12/12** and full
  `aw-tools` **341/341** plus strict Clippy/fmt/diff are green.
- Dotlink is repinned to `7e6fe20`; the local lower-bound shim is removed.
  Shell/PowerShell/Batch and finite Job provider metadata use one shared
  descriptor projection after host dynamic limits. Inspection, `list_tools`
  and `get_tool` report the same shared canonical titles while product
  descriptions/schema shapes remain unchanged.
- Current dotlink validation is **685 passed / 1 ignored + 36/36 integrations**,
  all-target/all-feature check, strict Clippy, fmt and diff green.
- B6 is complete for the current product surfaces. Release/version/tag/deploy
  remains separate.
