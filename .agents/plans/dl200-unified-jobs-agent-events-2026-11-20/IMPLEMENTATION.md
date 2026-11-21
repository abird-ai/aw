# DL-200 — phased implementation plan

Base design: [SPEC.md](SPEC.md)

Status: **R6 + SHARED PROCESS/RELEASE-CORRECTION AMENDMENT SEALED / IMPLEMENTATION PROMOTED / PHASE-0 CORRECTIVE IMPLEMENTATION GATE PENDING**

Original R1 seal: explicit user approval on 2026-11-20.
R2 amendment/reseal: 2026-11-21 after the Process/Event/replay/registry/owner-boundary reviews.
R3 amendment/reseal: 2026-11-21 after construction-fence, Event-auth/index, Jobs traversal and DisclosureManifest scalability reviews.
R4 amendment/reseal: 2026-11-21 after Event/Input store-coherence, Event migration cutover, symmetric driver recovery, Agent activation and rebuild-swap reviews.
R5 amendment/reseal: 2026-11-21 after external Event authority fencing, supervisor report-channel hardening, legacy-input provenance, driver startup compatibility, Event privacy ordering and source-bounds/descriptor review.
R6 **sealed**, 2026-11-21: collapse repeated internal mechanics; public model unchanged. Explicitly approved after final live-owner review.

Implementation was explicitly promoted by the user on **2026-11-21**. Work proceeds in
coherent Phase checkpoints under the constraints below.

## R6 simplification integration — existing phases, no separate runtime

The normative shared contracts are in `SPEC.md` sections 0.1–0.3. They define
**global identity / store-local consistency** and six descriptive roles:
Owner / Fact / Operation / Projection / Cursor / Scope. No six-trait framework is
created. Use one owner, one factual commit boundary and projections as accelerators.
R6 is a review-ready design, not evidence of code already migrated.

| Phase | Coherent code cut | Shared behavior to qualify together |
|---|---|---|
| **0** | Rename shared Process `Tree` → `Occupancy`; create the **required `aw-process` crate**, move the sole async Scope/pipe/stop/wait/drain owner into it and cut AW/dotlink over | Same exit/stop/occupancy/output semantics; bounded sinks; no parallel guards; dotlink `tree` wire unchanged |
| **0B** | Reuse stable replay identity, effect-outcome and ExternalEvidence checks inside current policy/Start owners | Prepared before an effectful external lease/reservation; exact replay; Unknown/no-repeat; current disclosure; held vs immutable proof |
| **1** | Introduce StoreScope (`store_id` + existing `restore_generation`), local `StorePos` and cursor/projection helpers in the existing Store; Agent/history/DisclosureManifest owners remain distinct | Restore-stale cursors/heads, cross-store mismatch, exclusive Agent activation, no audit-derived Agent state or DAG policy expansion |
| **2** | One EventScan core for read/subscription/Handler; one cursor envelope and projection-safe prefix; EventType registry staged with shared core | Source-first allow/deny/unknown; no advancement on uncertainty; Input transaction recheck, Handler at-least-once, hidden-row privacy, rebuild/cutover |
| **3** | `DriverKey`, Prepared/external-evidence reuse and typed Source/Sandbox/Vault leaf staging | Durable driver schema pin + restart inspect, external held fence through publication, one atomic constructor |
| **4** | Jobs consume `StorePos`, cursor envelope and projection activation helpers; owner-specific tree traversal remains | Stable tuple ordering, ancestor reauthorization, concurrent creation and rebuild catch-up |
| **5–6** | Compose existing typed leaf registrations, then dynamic Extensions consume the **same** Process owner | Atomic multi-leaf registration, typed-handle stability, no activation by installation, bounded process ABI |
| **7** | Retire obsolete compatibility state/legacy adapters and run final status/handoff + cross-repo convergence | One truth source per semantic fact, no duplicate scanner/guard/registry/ledger, migration+cross-repo gates |

No R6-wide rewrite: each primitive is implemented only when its first actual owner
uses it. Every extraction must remove duplication rather than add a parallel layer.
Store-only mutations do not become Prepared operations. An effectful external lease or
reservation happens only **after** durable Prepared/recovery identity; pure immutable
reads may happen earlier. External provider calls do not run unbounded inside SQLite
write locks. `StoreScope` is existing StoreId + restore generation, and scoped `StorePos`
is not a global revision. EventScan never owns EventInput or Handler effects. A shared cursor
envelope does not replace distinct Event/Jobs scanning algorithms.

The first R6 code slice remains **Phase 0**, before Phase 1. Preserve the earlier
R5 Phase-0A test evidence as **historical** evidence for code still using `Tree`;
run one focused AW+dotlink convergence gate after the planned `Occupancy` rename.
All other existing R5 qualifications remain required, but do not repeatedly
requalify unchanged primitives at every step.

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

## Phase 0 — shared Process settlement + dotlink 0.17.7 convergence

Status: **SHARED CUT COMPLETE / AW + DOTLINK PROCESS CONSUMER SOURCE IMPLEMENTED LOCALLY / PHASE 0B COMPLETE LOCALLY / PUBLICATION + REPIN + REAL-BUBBLEWRAP + WINDOWS-NATIVE QUALIFICATION PENDING**.

The earlier Phase-0 acceptance remains valid historical evidence for the v0.17.6 convergence,
but dotlink **v0.17.7** (`aefe5e7`, fix `64e3bd8`) exposed one shared Linux launch-lifetime bug
that the accepted `aw-process` source still contains: `Process::spawn` directly calls Tokio
`Command::spawn()` on the requesting worker. A launcher using Linux `PR_SET_PDEATHSIG` (Bubblewrap
`--die-with-parent`) therefore follows that retireable worker thread, not the host process. This is
a focused post-acceptance correction, not a re-run of all Phase 0 and not a new public phase. It
must be resolved before Phase 1; do not explain later SIGKILLs as external host noise until this
shared path is fixed/qualified.

### R6 shared-Process cross-repo checkpoint — published source (2026-11-21)

The R6 shared-runtime source cut is complete across AW and dotlink:

- AW runtime `7c16708` owns `Occupancy`, containment/spawn/stdin/stdout/stderr/wait/stop/reap/
  drain and canonical `ProcessTerminal`; AW main `59d67ba` adds the focused drain-flush test;
- hosts transfer authorized prepared commands by value; AW and dotlink retain policy,
  product storage/paging/ACK/result projection at their edges;
- weak direct-Unix negative membership => `Occupancy::Unknown`; Windows Job negative => Empty;
- `aw-process` has one global output-drain bound, cancellation/deadline-preemptible interactive
  writes, and bounded reader quiescence that flushes already-captured partial sink batches;
- AW adapter and dotlink contain no parallel Scope/Child/Reader/wait/stop/drain owner;
- dotlink `16475e4` deletes `src/shell_process.rs`, stores canonical optional ProcessTerminal in
  JobCompletion, retains exact `tree=leftovers|unknown` compatibility projection and uses non-login
  Bash. Its historical 30/35 turn projection is superseded by the pending audited v0.17.8
  dotlink-only 25/35/10 convergence below;
- PTY Terminal custody remains in `aw-terminal` and is not folded into Process Scope.

Grouped evidence remains intentionally compact: AW `aw-process` 3/3, runner 4/4 and adapter
Process 27/27 are green; dotlink final all-target/all-feature source graph and strict Clippy
`-D warnings` are green; rustfmt/diff checks are green. The final L5 executable rerun is also
green: shell **134/134** plus child-env **1/1**, Process **18/18** plus two process integration
tests, and Remote Start/replay **16/16**. Those successful reruns remain valid for unchanged
semantics. Earlier cold codegen/link SIGKILLs are no longer attributed to the external host after
the v0.17.7 parent-thread discovery; they are inconclusive infrastructure interruptions that may
have been caused by the retireable-worker spawn bug. No fixture fragmentation was needed.

The original v0.17.6 convergence gate was accepted. **That acceptance is now amended only for the
post-v0.17.7 Process-launch correctness items below.** The stronger PID-1 launcher/backend remains
separately promoted future work and is not part of this corrective gate.

### Shared Process/release-correction amendment — **SEALED 2026-11-21**, origin v0.17.7 / required before further DL-200 implementation

Port the release semantics **down into the existing shared owners**, never by resurrecting
`dotlink::shell_process` on main. The sealed high-concurrency refinement is one logical private
Linux Spawn Facility with stable process-lifetime lanes—not one thread per Process and not one
Process pinned to one lane:

1. **Linux Spawn Facility, O(1) lanes / O(n) Processes.** `aw-process` owns one private logical
   facility with a small fixed/add-only set of process-lifetime OS-thread Spawn Lanes. Start with
   **N=1**. Every Linux non-PTY spawn crosses this facility; only the short `Command::spawn` + exact
   `DirectScope` attachment executes on a lane. The resulting `Process` remains `Send` and all
   stdin/wait/cancel/output/settlement work remains ordinary Tokio work that may migrate between
   workers. A long-running command never occupies its Spawn Lane after custody transfer.
2. **Async bounded admission + linearizable stale-launch rejection.** Change/use the shared spawn
   handoff as an async operation. Hosts transfer the authorized `tokio::process::Command` **by
   value** through bounded async admission/backpressure. Do not use `block_in_place`,
   `spawn_blocking` or a synchronous caller rendezvous; runtime-managed blocking workers can retire
   and do not satisfy the parent-lifetime contract. Each request carries the originating Tokio
   `Handle` only so the lane can enter that runtime for child registration.

   Import the **v0.17.9 (`576d0a1`)** rule: cancellation/expiry is rechecked on the Spawn Lane
   immediately before fork because a request may become stale while queued behind another launch.
   Keep `aw-process` Agent-neutral by using a private linearizable launch gate rather than importing
   a product cancellation-token type. Conceptually the async side and lane race on
   `Pending | Cancelled | TimedOut | Launching`; the lane CASes `Pending -> Launching` immediately
   before `Command::spawn`. `Cancelled`/`TimedOut` wins => **no child is created** and the shared
   spawn result reports a pre-spawn outcome, never a Process stop. `Launching` wins => any later
   cancellation/deadline is handled by normal Process settlement.

   The runner establishes one **absolute execution deadline** when it enters Running, before Spawn
   Facility admission. The same deadline fences the lane and the later Process wait: Spawn-Lane
   queueing consumes execution budget; pre-Running product queue/preparation time does not. Do not
   restart a fresh full timeout after spawn.
3. **RAII custody before publication.** The lane spawns exactly once only after its final launch
   gate succeeds, obtains child identity, attaches the exact `DirectScope`, then constructs private
   `Spawned { Child, DirectScope, pid }`. Only this complete package may cross back to the caller.
   If the reply receiver vanished, failed transfer returns/retains `Spawned` and RAII settles the
   just-created scope. If cancellation/expiry wins before `Launching`, discard the unexecuted
   Command and return `CancelledBeforeSpawn`/`TimedOutBeforeSpawn` (names descriptive): no
   `ProcessTerminal`, `StopRecord`, occupancy, signal or "killed" claim exists because no Process
   existed. If `Launching` won and the caller disappears, RAII cleans the real child; never orphan
   or retry it. `kill_on_drop` remains only the final child fail-safe; `DirectScope` owns
   containment.
4. **Lane lifetime/failure semantics.** Spawn Lanes never retire, shrink or recycle while the host
   process lives; otherwise their still-running `PDEATHSIG` children may be killed. Future scaling
   may use a fixed `N>1` or monotonic add-only lanes, never shrink. Per-request launch code is
   panic-contained: RAII cleans partial state, the facility becomes poisoned/fails closed for new
   launches, but the lane thread stays alive/parked so existing children survive. Failure to create
   a lane before any accepted request may retry. Established-lane failure, facility poison, or a
   lost reply after effectful submission is `SetupUncertain`/restart-required and is never repaired
   by silently replacing the lane and replaying a Command.
5. **No public pool/tuning surface.** Lane count is an internal performance property, never a
   user/model/profile/API knob. Implement `N=1`; add lanes only if the focused spawn benchmark shows
   material admission queueing. Benchmark concurrency `1/8/32/128` against lanes `1/2/4`, measuring
   spawn throughput, queue-wait p50/p99, spawn-handshake p50/p99 and CPU. Prefer `N=1` unless those
   measurements justify more. Do not build a generic pool scheduler in advance.
6. **Wake != terminal fact.** Keep SIGCHLD/pidfd readiness only as wakeups. Tighten Linux
   `DirectScope` so cleanup eligibility requires strict parent-owned non-reaping
   `waitid(WEXITED|WNOHANG|WNOWAIT)` for the main child. Do not use the generic nonchild
   pidfd-readiness fallback (`ECHILD -> poll`) to authorize a group sweep. Retain the leader
   unreaped through occupancy observation + final group signal, then reap.
7. **Exit and stop remain independent facts.** `StopRecord` records a containment stop operation;
   delivered stop evidence alone does not prove it caused `ProcessExit`. A status published first
   or physically incompatible with the delivered termination wins. Leftover cleanup never
   relabels the main ending. Replace/avoid projections that infer `stopped_by` from
   `StopRecord::terminal_reason()` alone; derive causal wording from the complete
   `ProcessTerminal`. A signal without reconciled shared stop is *unattributed*, not proven
   external. Keep first-requester caller/connector attribution at the dotlink host edge, recorded
   before firing its cancellation token; shared AW still carries only semantic `Cancelled`.
8. **Windows noninteractive launch + interruptible occupancy settling.**
   `aw-platform-win::configure_managed_process` uses `CREATE_SUSPENDED | CREATE_NO_WINDOW`, then
   existing Job Object attach + resume. Do not use `DETACHED_PROCESS`; PTY launch remains separate
   in `aw-terminal`. Port the post-v0.17.8 `81ef891` rule plus the v0.17.9 refinement into the
   shared owner: after the exact main process is terminal, do not publish `Occupancy::Live` from the
   first nonzero Job accounting sample because transient no-window console/job infrastructure may
   still be leaving. `aw-platform-win` supplies instantaneous exact active-member accounting;
   `aw-process` performs a bounded async settle (initial reference: <=2s, ~10ms cadence): zero at
   any query => Empty, nonzero for the whole **uninterrupted** bound => Live, query failure =>
   Unknown. Cancellation or the execution deadline preempts this grace and sweeps immediately;
   unless Empty was already proven, interrupted settling publishes Occupancy Unknown while
   preserving the main command's physical exit. A nonblocking/try-wait path may accept immediate
   zero as Empty but must treat immediate nonzero as Unknown unless it performs the full settle.
   No image-name/conhost exemptions, and never sleep the settle window directly on a Tokio worker.
9. **One focused consumer gate, not fixture proliferation.** AW: force the requesting Tokio worker
   to retire and prove a `PDEATHSIG` child survives while its async Process task can continue on
   another worker; queue a launch behind the lane, cancel it and separately expire its absolute
   deadline before the lane claims it, and prove no exec occurs and no Process stop fact is
   fabricated; race cancellation after `Launching` and prove any real child follows normal RAII/
   Process settlement; prove a poisoned launch request does not terminate the lane/existing
   children; prove Spawn-Lane delay consumes the same absolute execution deadline rather than
   granting a fresh timeout; prove spurious pidfd readiness cannot authorize cleanup; prove an
   earlier/incompatible physical exit is not overwritten by a racing stop; cross-target/type-check
   Windows launch flags; on Windows prove a clean `CREATE_NO_WINDOW` command with transient
   console/job infrastructure settles to Empty, a real leftover remains Live through the settle
   bound, and cancellation/deadline interrupts the grace immediately with Unknown rather than false
   Live. Dotlink main then repins the fixed AW runtime and reuses the real v0.17.7 Bubblewrap,
   post-v0.17.8 Windows and v0.17.9 stale-launch behavior gates. Re-run the existing grouped
   Process/shell gate; do not copy a second launcher/observation state machine into dotlink.

A dedicated launcher process is deliberately rejected for this direct fallback: making another
process the kernel parent would move wait/reap/pipes/pidfds across IPC/SCM_RIGHTS and duplicate much
of the separately promoted PID-1 supervisor. This correction does not thaw
`017-sandbox-mount-convergence`, change `ProcessTerminal`'s public shape, add a generic
`SpawnBroker`/pool API, or promote that future supervisor. It is a private sealed implementation
correction inside the existing `aw-process`/`aw-platform` boundaries.

### Phase 0A checkpoint — shared facts + direct observation complete (2026-11-21)

Implemented in the current uncommitted AW working tree:

- `Tree::{Empty,Live,Unknown}` in `aw-tools`;
- `ProcessOutputMeta` now carries per-stream completeness and derives aggregate complete;
- `ProcessTerminal` now carries `Option<StopRecord>` + `Tree` + output as the one factual
  terminal model;
- `ProcessReceipt` derives aggregate completeness and preserves the full StopRecord;
- `job_host` passes one ProcessTerminal instead of reassembling exit/stop facts piecemeal;
- historical checkpoint: direct platform observation was first introduced through a Guard-shaped API; this was subsequently superseded by the private `Scope` + `DirectScope` owner below;
- Linux direct observation requires the correct PID namespace + unfiltered procfs for a
  trustworthy negative, excludes zombies and counts live threads;
- macOS direct group observation and Windows Job active-member observation are implemented;
- no tree product/wire projection has been enabled yet, so existing AW compact result
  wording is not prematurely changed.

Focused evidence is green:

- affected shared crates compile, including `aw-platform-win`;
- Unix managed-process tests: 4/4;
- `aw-tools` runner tests with `full`: 4/4;
- adapter lifecycle stop/external-exit test: 1/1;
- `git diff --check` clean.

A low-parallel `cargo check --workspace` reached the root `aw-cli` and then failed on two
pre-existing Rust-2024 let-chain expressions in `src/subagents.rs` under the workspace's
current edition; those files are outside this Phase-0A diff and were not changed.

`crates/aw-store/src/job_exposure.rs` also became independently dirty during verification
with unrelated Job-exposure work. Preserve it; Phase 0 did not edit or revert it.

### Phase 0A continuation — shared settlement ordering complete (2026-11-21)

Implemented after the initial Phase-0A facts/observation checkpoint:

- historical checkpoint: non-reaping readiness was first exposed as a free helper and
  adapter-owned wait loop; the supervisor refinement below folded that into private
  `Scope`/`DirectScope` ownership with stable leader identity;
- natural exit now performs `exit-ready -> Tree observe -> cleanup -> immediate retry on
  Unknown -> reap -> drain`;
- graceful Unix semantic stop waits non-reaping, observes/settles the remaining tree, then
  reaps; forced stop keeps Tree `Unknown` when pre-cleanup post-command occupancy cannot be
  proven;
- post-drain descendant cleanup was removed: containment is settled before draining, so a
  successful drain can no longer hide cleanup failure and an inherited pipe can close
  promptly after descendants are killed;
- historical checkpoint: the bounded Unknown retry originally required explicit guard
  disarm before reap; the supervisor refinement below moved retry/finalization entirely
  into `DirectScope::terminate()` so higher layers cannot release/retry containment;
- `termination_available()` now uses non-reaping readiness and cannot accidentally consume
  leader status before settlement;
- physical `ProcessExit` continues to come from the actual child wait status and is never
  overwritten by cleanup attribution.

Focused evidence is green:

- affected shared crates compile, including `aw-platform-win`;
- Unix managed-process tests 4/4;
- aw-tools runner tests 4/4 with `full`;
- adapter regressions green for: redirected background child cleanup/Tree::Live,
  inherited-pipe pre-drain cleanup, external exit vs connector stop, cancellation,
  deadline, and terminate-once/factual SIGTERM;
- `git diff --check` clean.

The unrelated concurrent `crates/aw-store/src/job_exposure.rs` and other current AW store
work remains untouched and must be preserved.

### Phase 0A supervisor refinement — one private Scope owner (2026-11-21)

The Process design was deliberately reopened before dotlink cutover and simplified further:

- AW semantics now define **Process = one supervised execution scope**, not one leader PID;
- `PipeProcess` owns one private async `Scope`; higher layers no longer own `Child + pid +
  guard` separately;
- the platform fallback is explicitly `DirectScope`, not the Process model;
- Linux `DirectScope` uses pidfd when available and the still-unreaped child PID as the safe fallback, plus process-group identity; Windows owns a
  duplicated exact leader HANDLE plus Job Object; macOS owns child/group identity;
- readiness/observation/grace/forced cleanup are synchronous platform facts under that
  scope; async waiting stays in the Process runtime and `aw-platform` gains no Tokio
  dependency;
- free/public `child_exit_ready`, higher-layer `disarm/release` and adapter-owned retry
  choreography are removed; `DirectScope::terminate()` owns the bounded final retry and
  then becomes inert;
- `ProcessReceipt` now stores one `ProcessTerminal`; `Outcome`, aggregate completeness,
  status and stop helpers are derived projections;
- the stronger Linux backend is a tiny AW-owned PID-1 supervisor in a per-command PID
  namespace, with Bash/the main command as PID 2. It is instantiated by a real host/Sandbox
  launcher when that layer is promoted, not by unsafe post-fork Rust or a hidden mode
  imposed on every embedding binary;
- current direct execution remains the private fallback. Swapping it for the PID-namespace
  supervisor must not change `ProcessTerminal`, Jobs or dotlink semantics.

Focused existing Process regressions remain green after the ownership refactor; platform
crates and adapter compile on Unix + Windows.

**Resume boundary:** Phase 0 shared Process convergence and executable acceptance are complete.
Continue the outstanding Phase 0B replay/admission/exposure remediation without disturbing this
owner. Do not start DL-200 Phase 1 until Phase 0B is resolved/reviewed.

This phase is intentionally first. It fixes current reusable Process semantics before
DL-200 builds more Jobs/Agent behavior on them.

### AW

1. Keep one shared terminal model only:

   ```text
   Occupancy = Empty | Live | Unknown
   ProcessTerminal = exit + Option<StopRecord> + occupancy + output
   ProcessOutputMeta = flat per-stream bytes/truncated/complete facts
   ```

2. Model Process as one supervised execution **scope**, not a leader PID. Create
   **`crates/aw-process`** as the required Agent-neutral async Process library. Extract
   private `Scope`, one prepared spawn, stdin/EOF, stdout/stderr pipe readers,
   wait/stop/cleanup/occupancy, bounded drain and `ProcessTerminal` settlement from
   `aw-tools-adapter::process` into that crate. `aw-process` depends downward on
   minimal `aw-tools` semantics, `aw-platform` and async primitives; it does not depend
   on `aw-agent-core`, MCP, SessionStore, ToolPolicy or host policy.
   AW `PipeProcess`/adapter and dotlink shell must consume it, without independent
   Child/PID/process-group/Job/pidfd/wait/stop/pipe-drain state machines. Each host
   prepares its authorized executable/Sandbox/environment and transfers the prepared
   command **by value**; `aw-process` owns containment finalization, stdio ownership,
   spawn, child custody, wait/stop/reap and drain. Keep host launch authority and
   deadlines, output spool storage/retention, transport and logs at the edges. One
   narrow bounded output-sink interface reports actual acceptance; blocked/failed sinks
   cannot hang cleanup and dropped bytes must never be marked fully captured.
   `ProcessTerminal` is the shared Fact; AW `ProcessReceipt` and dotlink JobCompletion
   remain edge envelopes rather than a new generic receipt. `aw-terminal` PTY custody,
   including source release/adoption, remains a distinct Terminal resource and is not
   forced through ordinary Process Scope.

3. Keep direct-host platform mechanics behind `aw_platform::DirectScope`:
   - Linux: pidfd when available, unreaped-child PID fallback + process group + trustworthy procfs fallback observation;
   - macOS: stable owned child identity + process group;
   - Windows: duplicated exact process HANDLE + Job Object.

4. Keep `aw-platform` synchronous and runtime-free. Async wake/wait policy belongs to the
   private Process `Scope`; wake hints are never factual authority.

5. `DirectScope` owns readiness/observation/grace/final termination. Final termination does
   one immediate retry on Unknown while leader identity is safe and then becomes inert;
   there is no higher-layer disarm/release API or post-reap numeric retry.

6. Store `ProcessTerminal` directly in ProcessReceipt. Derive Outcome, aggregate output
   completeness, Process status and stop helpers. Keep independent Job-level Status only
   where a Job result can fail outside the Process itself.

7. Direct fallback settlement converges on one order:

   ```text
   main terminal readiness / stop boundary
   -> observe remaining scope ONCE and freeze Occupancy (pre-cleanup occupancy)
   -> settle scope / record StopRecord
   -> reap main command
   -> settle output
   -> ProcessTerminal
   ```

   Cleanup never rewrites Occupancy. Preserve the main command's independently observed
   physical exit across cleanup races. `aw-platform::DirectScope` may retain a private
   containment-local negative for cleanup, but `aw-process` maps weak direct-Unix negative
   process-group/procfs evidence to `Occupancy::Unknown` whenever escaped descendants cannot
   be excluded. Never promote group-local absence to semantic `Occupancy::Empty`.

8. Strong Linux target: per-command PID namespace with tiny AW supervisor as PID 1 and the
   main command/Bash as PID 2. Supervisor owns spawn/reap/tree/cleanup and reports main-exit
   + pre-cleanup occupancy over one private bounded **unidirectional supervisor→host** channel
   independent of user stdio. Only the supervisor retains a writable report endpoint; close/
   CLOEXEC it out of the command/descendant fd tables before exec. The host validates bounded
   versioned framing + expected launch/supervisor identity and accepts exactly one terminal
   report. Malformed/duplicate/unexpected reports are report failure, never command facts. If
   PID 1/reporting dies before the main command terminal fact is received, physical exit is
   `ProcessExit::Unknown`; Occupancy is the already-frozen pre-cleanup fact if known, otherwise
   Unknown. Namespace destruction may prove final cleanup but never masquerades as the main
   exit or rewrites Occupancy. Do not make Bash PID 1.

9. Because AW is a library, instantiate the strong PID-1 backend only through a real
   host/Sandbox launcher. Do not run arbitrary Rust after fork, assume an external utility,
   or require every embedding binary to implement a hidden supervisor mode. Do not promote
   the frozen 017 mount plan merely to land this Process API.

10. Keep Sandbox and Scope orthogonal:
    - Sandbox prepares where/how execution may run;
    - private Scope owns exactly one Process lifetime;
    - unsandboxed/direct execution remains first-class;
    - sandboxing alone does not imply PID namespace;
    - SandboxDriver may provide an opaque host-only launch capability for a stronger Scope,
      but never owns wait/stop/ProcessTerminal semantics.

11. Select Scope privately from the trusted launch requirement; do not add a model/user-
    facing direct/namespace/job/remote selector. Default policy may choose the best
    authorized/available backend, but a grant/policy requiring **closed descendant
    containment** fails admission if no closed PID-namespace/Job/equivalent backend is
    available. Never silently downgrade or retry through weaker DirectScope after such a
    requirement was admitted. Preserve factual containment strength: weaker allowed
    backends may yield Occupancy::Unknown.

12. Establish environment + lifetime ownership before the main command can run where the
    platform permits: prepare Sandbox -> prepare/select Scope -> launch into Scope -> publish
    Process. Do not make spawn-then-attach the generic contract.

13. Swapping `DirectScope` for the strong Linux supervisor must require no ProcessTerminal,
    Jobs or dotlink semantic API change. Windows Job Object is already a strong containment
    backend; macOS/direct Unix remain weaker fallbacks where Occupancy::Unknown can be factual.

### dotlink

1. Depend directly on the **`aw-process` crate** (after the separately authorized AW
   publication/repin) and consume its shared Process lifetime/pipe owner, not merely
   the `ProcessTerminal` struct. Dotlink must not import the Agent-specific AW adapter
   solely for Process execution and must not retain a parallel guard, pipe reader,
   wait/reap/stop/retry state machine. This is a real dependency and consumer compile gate.

2. Remove/supersede dotlink-local lifetime ownership where shared Process now owns it. Keep
   dotlink-specific launch/configuration, output transport, MCP projection, operator logs and
   timeout policy only. An unresolved cleanup may never be hidden by a successful drain.

3. Converge Bash launch with AW:
   `--noprofile --norc -c` for direct and Bubblewrap execution.

4. Project shared facts in one place:
   - `Occupancy::Live -> tree="leftovers"`;
   - `Occupancy::Unknown -> tree="unknown"`;
   - `Occupancy::Empty -> omit tree`;
   - per-stream drain-incomplete flags;
   - signal names via AW helper;
   - causal `stopped_by` derived from the reconciled full `ProcessTerminal`, never StopRecord alone;
   - one concise lifecycle log line.

5. Product result policy:
   - leftovers may clear dotlink `success`;
   - leftovers do not rewrite the physical exit code;
   - leftovers alone do not turn the MCP call into a generic tool error.

6. Restore concise lifetime guidance:
   - foreground daemon idiom: `server & wait`;
   - Windows equivalent: `Wait-Process`;
   - state process-group/Job containment boundaries honestly.

7. Port audited dotlink **v0.17.8** bounded-turn policy cleanly on top of the AW-backed main
   runtime after the shared Process correction/repin:
   - one dotlink-local `RuntimeSurfacePolicy` derived from `ActiveTransports.constraints()`;
   - bounded turn horizon = **35 minutes**;
   - hand-back/report reserve = **10 minutes**;
   - bounded new-command ceiling = **25 minutes / 1500 s** (`horizon - reserve`), further narrowed
     by the configured shell timeout;
   - from minute 25, server instructions say open no new work: land/park current work, leave tree/
     notes reportable, state done/pending and hand back before minute 35;
   - `sync_hold.is_some()` is the capability that enables this policy; local-only uncapped runtimes
     keep configured Process limits and no turn advice; the tightest active transport constraint
     wins when transports share one machine/schema;
   - effective command ceiling/transport constraint participates in execution-authority fingerprint
     for **new** effects; current advisory capability is rebound at every machine construction and
     does not rewrite/reclassify an already-running Job's immutable timeout;
   - keep `background=true` as observation/handoff only; it never extends the admitted deadline;
   - derive startup warning, server instructions, shell schema/runtime validation and near-deadline
     Job advice from the same local policy values—no duplicated 25/35/10 constants.
8. Keep the bounded-turn policy out of AW Process/Job semantics and use AW extension points by
   composition:
   - `aw-tools::JobManager` remains canonical for Job lifecycle/revision/output/ACK facts and stays
     clock-/transport-policy-free;
   - dotlink keeps only narrow local monotonic Job timing: `created` for observational
     `elapsed_ms`, immutable admitted `timeout_secs`, and `execution_deadline: Option<Instant>` set
     exactly when the runner enters Running; product queue/preparation age does not consume the
     execution timeout, while Spawn-Facility queueing after Running does;
   - use typed `runtime_snapshot`/ProcessTerminal/output facts; never parse a shared rendered receipt
     and never mirror canonical lifecycle state locally merely to produce advice;
   - AW `ReceiptProfile` provides factual `ts`, but dotlink bounded runtime sets
     `checkpoint_minutes=None` so the generic AW ~checkpoint instruction cannot conflict with the
     exact 25/35/10 product policy;
   - v0.17.9 correction: `jobs_get` near-deadline note uses the runner's actual absolute
     `execution_deadline`, never `timeout_secs - elapsed_ms`; Queued has no deadline/advice, Running
     warns only inside `min(timeout/5, 5m)`, reached/cancelled deadlines are quiet, and the wording
     names the execution deadline. Prepend the ordinary `next`; remain absent from list/read/ack/
     cancel and terminal/cancelling/revoked/journal-Unknown views;
   - if direct use of the shared jobs adapter blocks this composition later, factor only the
     smallest typed factual Job-status projection seam at the first real consumer. Shared default
     renderer and dotlink renderer consume the same facts; no arbitrary callback/plugin, generic
     Job-policy trait, host clock in `aw-tools`, or second Job runtime.

### Phase-0 qualification

Use focused semantic tests, not a large fixture expansion:

- clean exit / nonzero exit / explicit exit 137 / external SIGKILL;
- timeout / caller cancel / connector shutdown;
- parent exit with live child whose stdout/stderr are redirected away;
- child holding inherited output pipe past drain grace;
- cleanup Unknown gets one bounded final scope-owned retry and no post-reap retry;
- direct Unix stable leader identity is retained through observation/cleanup/reap ordering;
- Linux direct fallback zombie-only vs live-thread-under-zombie;
- direct Unix positive remaining member => Occupancy::Live, but negative process-group/procfs
  observation => Occupancy::Unknown whenever escaped descendants cannot be excluded; never claim
  Occupancy::Empty from a containment primitive that is not closed over the semantic Process scope;
- current direct-Unix source must be corrected so semantic observation no longer maps a
  weak `Ok(false)` group result to Occupancy::Empty; qualify with a `setsid` escaped-descendant
  case and preserve the main command's physical exit;
- incomplete/filtered/wrong procfs => Unknown on direct fallback;
- policy/grant requiring closed descendant containment rejects before launch when no closed
  backend is available; never silently downgrade to DirectScope;
- the future PID-1 backend contract remains supervisor-authored only: command/descendants
  cannot inherit a writable endpoint or forge terminal facts; malformed/duplicate/unexpected
  reports fail closed and pre-exit report loss yields `ProcessExit::Unknown`. This is **not**
  an executable Phase-0 gate while the launcher/sandbox backend remains frozen; run the
  corresponding live qualification only when that stronger backend is separately promoted;
- Windows exact leader HANDLE + active Job membership where platform is available;
- Sandbox/Scope composition is identical sandboxed or unsandboxed: Sandbox environment is
  orthogonal to one fresh Process Scope; strong PID-1 supervisor remains a launcher/Sandbox
  backend with no semantic API fork;
- `aw-process` builds as its own Agent-neutral crate, with a downward dependency
  graph and no AgentCore/MCP/SessionStore dependency; the real AW adapter and dotlink
  consumer both compile against it without an independent lifetime owner;
- AW and dotlink behavioral parity through **one `aw-process` lifetime owner** for
  clean/nonzero/signal exits, escaped-descendant uncertainty, remaining children,
  timeout/cancel, inherited-pipe output, bounded sink backpressure/failure and incomplete
  drains; never accept false output completeness or stop/drain hangs, and do not qualify
  only serialized-field parity;
- direct + Bubblewrap non-login Bash;
- v0.17.8 dotlink product projection: bounded surface derives 25-minute command cap + 10-minute
  report reserve inside the 35-minute turn from one `RuntimeSurfacePolicy`; stdio/HTTP-only
  uncapped runtime retains configured Process limit and no turn advice;
- effective ceiling participates in new-effect authority while a transport/advisory rebind cannot
  rewrite an existing Job timeout or factual shared lifecycle;
- v0.17.9 stale-launch gate: queue a Process launch, cancel/expire it before the stable lane
  claims `Launching`, and prove no exec/ProcessTerminal/StopRecord/kill attribution occurs; when
  `Launching` wins first, any later stop follows normal Process semantics; the same absolute
  execution deadline spans Spawn-Facility queueing + Process runtime and is never restarted;
- v0.17.9 deadline projection: Queued Job age may grow in `elapsed_ms` with no advice; runner
  publishes one `execution_deadline` at Running, and `jobs_get.next` counts down from that deadline
  (not `timeout_secs - elapsed_ms`) only inside `min(timeout/5, 5m)` on a hand-back-capable surface;
- Windows settle is preemptible: clean transient infrastructure => Empty, persistent full-window
  member => Live, query failure or cancel/deadline-interrupted nonzero settle => Unknown, with main
  physical exit preserved and immediate cleanup on interruption;
- shared AW receipts keep `ts` but no competing generic checkpoint schedule for dotlink;
- port uses canonical `aw-tools` Job facts and dotlink projection only—no local canonical lifecycle
  fork or rendered-text parsing.

Checkpoint when AW shared semantics are green and dotlink is a thin consumer of the same
Process lifetime owner again.

### Phase 0B — current replay/admission/exposure review remediation

Fresh post-seal review identified owner-level seams to qualify **before** the broader
DL-200 Agent/Event cutover. Work already in progress on Phase 0B must be preserved; reuse
its current indexes, atomic policy/Start transition and owner contracts. No second
replay/projection/runtime service and no lifetime audit scans on the execution path:

1. **Process cleanup fail-safe** — superseded by the Phase-0A supervisor refinement above:
   private `Scope` owns lifetime and `DirectScope::terminate()` performs the bounded final
   retry while stable leader identity is still owned. Higher layers never disarm/retry a
   numeric containment target after reap; final Unknown remains factual in ProcessTerminal.
2. **Remote Start replay-first target admission** — immutable authenticated client/semantic
   identity is validated for every delivery, then the existing target replay owner is
   consulted. Lease age, live runtime generation and current issuer trust gate only a
   genuinely new claim. Ready/Unknown target receipts use bounded Job retention rather than
   the short launch lease, so an expired/revoked proof can recover an exact retained receipt
   but can never create a new effect. Runtime restart remains an explicit fail-closed
   boundary while the target replay owner is memory-resident.
3. **Atomic AW policy->Start linearization** — one SQLite transaction reuses the existing
   policy-consume transition and commits `remote_start_current`. A failed Start validation
   rolls policy consumption back; a historical Consumed-without-Start row may finish only
   while its exact original/current evidence still revalidates. Read-only remote calls may
   repeat after a consumed-policy crash under current authority because no remote effect
   receipt exists to recover.
4. **Complete Agent exposure publication** — one shared `branch.created` helper is used by
   normal fork, Context fork, archive extra branches, Workspace replica import/divergence
   and remote-writer fork. Initial Session main branches remain covered by `session.created`.
   Schema v37 repairs existing v36 gaps by enumerating authoritative Session/branch rows,
   never by trusting the incomplete old projection/event set.
5. **Projection-backed operational hot path** — finite exposure current lookup, capacity,
   listing and Agent position use indexed `job_exposure_current`; immutable `commit_log`
   remains ordering/audit/rebuild evidence rather than a lifetime JSON-scan hot path.
6. **Typed finite identity** — finite exposure accepts only a canonical shared
   `ExecutionRef` whose owner exactly matches the authoritative Agent and whose suffix is
   exactly `[Turn, Job]`; string-prefix/`contains("/j:")` heuristics are removed.
7. **Policy replay and real initiating actor** — the existing `policy_admission_current`
   claim owner must find a matching original ToolCall **before** current writer/Turn/Policy
   checks and before resolving a contextual ref again. The claim key is authenticated
   source/operation/original call identity; accepted canonical target, policy/authority
   generations and revised arguments are stored evidence, **not** a fresh replay identity.
   Changed original arguments/ref with the same call ID reject. Alice's Session/writer
   remains the resource owner while Bob's authorized participation, ToolPolicy, provider,
   payer and disclosure are independently checked at **fresh** admission; no owner-rights
   union. A revoked actor can recover only presently authorized disclosure of an old
   receipt, never fresh effects or protected result payloads.
8. **No Session-lifetime audit ceilings / compact replay tombstones** — complete use of the
   **existing** indexed `policy_admission_current` and `remote_start_current` owners for
   claim/replay/active-Turn capacity. Immutable audit remains append-only provenance and
   one-time migration rebuild input, not the normal per-request JSON scan or a 4,096/8,192-
   event lifetime admission limit. Full receipt/recovery detail may compact in place to a
   tiny replay tombstone containing replay key + original semantic fingerprint + terminal
   disposition/Unknown + minimal non-retry evidence. Large payload/current-authority detail
   can expire independently. Replay contracts preserve **effect/outcome identity**, not
   indefinite payload bytes: constructors retain their exact created canonical IDs for the
   replay lifetime; compacted effectful operations return the typed non-retryable compact
   acknowledgement instead of pretending the original receipt still exists. Lifetime is
   concrete per replay namespace: durable Agent/history ToolCall identities retain the
   tombstone with that retained identity; a host RPC namespace may retire it only after the
   authenticated transport/owner irreversibly rejects all old IDs from that namespace. If
   no such presentation boundary exists, retain the tombstone. TTL alone never re-enables a
   consumed identity. Do not introduce another table/service for the same owner.
9. **Replay evidence vs payload disclosure** — an exact committed remote Start, policy Tool
   effect or paid guardian receipt stays factual and cannot rerun after lease expiry/
   revocation. Cached content is returned only through current source/resource/audience +
   immutable dependency-manifest disclosure checks. If disclosure is gone, return only a
   minimal non-retryable replay acknowledgement: no canonical target, Job ID, output size,
   timestamps, provider details or protected payload. If even effect existence is
   protected, return an opaque prior-invocation/non-retryable response. Withholding content
   never erases the replay claim or permits a replacement.

**One grouped Phase 0B qualification:** original ToolCall after writer/policy/parent
change (same target/receipt, no second paid/effectful attempt), changed request under
same ID denied; Alice owner/Bob permitted participant works with Bob's own grants;
revoke-Bob blocks fresh effect and private replay payload while returning only the minimal
non-retryable acknowledgement permitted by current disclosure; long-lived Sessions beyond
old audit thresholds still admit new work; full replay rows compact to tiny tombstones
without making the identity executable again; constructor replay still returns the same
created canonical identity after detail compaction; ToolCall tombstones follow retained
Agent/history lifetime; a host-RPC tombstone disappears only after its request namespace is
irrevocably retired; exact/expired/revoked/old-generation Start returns only the currently
disclosable retained receipt, compact/opaque withheld replay acknowledgement or Unknown. Retain the existing tests for atomic consume rollback, v35→v37 exposure repair,
finite capacity/paging/typed refs and Process cleanup.
Do **not** requalify unchanged primitives or reintroduce lease-scoped replay caches,
commit-log operational scans, list-time fallbacks or duplicate policy/exposure owners.

**R6 consolidation:** share tiny typed replay-key/fingerprint, Prepared/effect disposition,
compact tombstone and ExternalEvidence validation helpers with the real owner; do **not**
replace the existing policy/guardian/remote Start state machines or move their receipts
into a new ledger. For Store-local claims, reuse the existing SQLite transition; an
external provider fence is acquired outside the write lock and must remain held through
the exact commit. Qualification uses the single grouped boundary above, including no
second effect after a restarted Unknown and current disclosure after replay.

**Scoped policy/replay implementation checkpoint (2026-11-21):** AW now looks up
`policy_admission_current` by `(session, branch, Turn, original ToolCall)` before comparing
accepted canonical targets, and the host policy router performs this lookup before new
contextual-ref resolution or grant/provider selection. Same original request recovers the
stored target; changed original fingerprint conflicts. Replaying a factual claim requires
current participant `view`; fresh admission still requires current `submit` and original
writer/grants. There is no replacement replay ledger or new index. The two focused
`aw-store --test store policy_admission` cases pass (2/2), including Bob's
share/revoke/View-only regrant. The production policy-router integration test passes
(1/1) and `cargo check --offline --locked -p aw-cli -p aw-store` is green. Strict
`cargo clippy --offline --locked -p aw-store -p aw-cli --all-targets -- -D warnings`
is green after removing two unnecessary `return` statements from the in-progress Unix
Process observation paths (same behavior). The policy admission owner passes its standalone
`rustfmt --check`, and `git diff --check` is clean. Existing unrelated Phase-0A/source
formatting drift is not widened by this checkpoint.
**Replay-disclosure owner checkpoint (2026-11-21):** immutable replay identity remains with the
existing Store/target replay owners while payload disclosure is composed across the two real
hosts. AW first requires current Session View + ToolBinding + Actor Envelope source/effect/site.
Dotlink then requires current issuer trust and its own concrete resource authority before exposing
the cached receipt: prepared native filesystem mutation capability, shell ToolPolicy, or Job
control ToolPolicy + exact retained Job availability. Expired leases and old runtime generations
remain fresh-effect fences only. Failed disclosure returns only the shared opaque/minimal non-
retryable acknowledgement and can neither erase the replay claim nor dispatch another effect.

The implementation deliberately adds no cross-host preflight interface and no generic Tool hook.
AW reuses `SourceKind::as_str()`. Dotlink reuses the existing canonical filesystem request,
`access_paths()`, prepared `CapabilitySet`, `SecuredFilesystem::preflight_mutation`, ToolPolicy
lookup and `standalone_status`; the FS check resolves authority only and never reads/locks/stages/
commits payload. This keeps filesystem/Job authority at dotlink, rather than falsely projecting
AW-local resources onto a remote target.

AW production compile, policy owner **2/2** and AW Remote Start owner **5/5** remain green.
Dotlink grouped Remote Start/replay is **16/16**, strict all-target/all-feature Clippy is green,
and fmt/diff checks are green. The routed AW root integration linker still receives an
unattributed SIGKILL before assertions; v0.17.7 makes the retireable-worker launcher bug a
plausible infrastructure cause, so do not classify it as an outside-host kill and do not split the
test merely to manufacture evidence.

**Participant Provider/payer checkpoint (2026-11-21):** one shared
`ProviderAccounts::select_exact(actor, captured_identity)` now reselects current authority and
requires full `InvocationIdentity` equality (account owner, payer, custody/version, executor,
model, grant revision). Policy replay lookup remains before this check; every fresh policy
admission uses it, guardian reuses that exact selection, subagent/participant owners use the same
primitive, and the service-less remote router receives the same ProviderAccounts owner. No
Session-owner account or payer can be unioned into a participant.

For paid policy delegation, durable resource ownership and paid identity are explicitly separate:
the parent Session owner owns root/allocation/child Session/journal/lifecycle; the participant's
exact identity owns provider/payer/model/tool work. The Store exposes only a narrow top-level
participant-policy-delegate creation path: transaction-local current `Submit`, non-owner, depth 1,
no child routing, no constructor replay. Ordinary/recursive children retain their existing owner
identity contract. Delegation model-attempt admission now checks the exact identity stored on the
running child relation instead of equating provider actor with resource owner; lifecycle mutations
check the resource owner only. Child Actor attach rechecks both exact ProviderSelection and current
participant `Submit`.

Qualification reuses existing tests: account publication **1/1**, Store subagent/delegation
**1/1**, policy replay **2/2**, all root test code type-checks, strict affected all-target Clippy is
green, fmt/diff clean. The existing Bob local/delegate policy-router test now includes Bob-owned
payer/provider, revoke/no-owner-fallback, explicit regrant/reselection and Bob-paid delegation;
its root executable still receives an unattributed SIGKILL during L5 linking before assertions
and was not split into fixtures. The v0.17.7 shared Process corrective gate now precedes the
remaining compact tombstone + lifetime-threshold Phase-0B work.

---

## Phase 1 — Agent identity, history and access foundation

Land the durable owner model before broad public API cutover.

1. Persist `agent_state_current` as **authoritative durable Agent-envelope state** with
   owner-local `rev`, lifecycle, admission, Project, Sandbox binding and immutable
   parent/delegation lineage. Back up/restore it as owner state; only lookup/list/exposure
   indexes over it are rebuildable.
2. Perform the one-time Agent-state + ACL seed under the store's existing schema/write
   exclusion: no serving/concurrent legacy Agent-envelope writes. In one migration
   transaction backfill the complete rows, freeze handles/revs and publish the internal new
   owner/schema epoch only after the seed is complete. Commit atomically activates
   `agent_state_current`; rollback leaves legacy ownership intact. V1 does not add online
   dual-write/catch-up. After activation, stop deriving every Agent-envelope fact from
   legacy audit; SessionStore remains authoritative only for backing Turn/history state.
3. Add Domain principal registry and user/group kinds under one `i:` handle namespace.
4. Add groups/membership + group rev/security generation.
5. Add Agent ACL + `acl_rev` + derived `access_generation`.
6. Keep Project as the existing Workspace-backed projection; do not create a Project
   service/database.
7. Add immutable `HistorySpan`, `HistoryBase`, `HistoryGrant` and scalable
   `DisclosureManifest` storage/retention. Manifest representation is a persistent
   content-addressed bounded-fanout set/DAG with structural sharing; preserve exact
   source-fact provenance separately from deduplicated stable disclosure-authority roots.
   Ordinary audience/share/fork checks operate over unique authority roots (with an optional
   rebuildable acceleration projection), while exact facts remain authoritative for audit/
   rebuild. Bound node size/fanout and per-request union/authorization work; never flatten a
   long-lived closure into giant JSON or a mutable ACL mirror.
8. Implement lazy canonical HistoryBase sealing with no Agent rev/Event change.
9. Preserve stable immutable history-fact identity and original provenance across forks.
10. Add minimal Domain/Principal/Project/Agent snapshot reads with privacy-aware metadata.
11. Resolve one canonical Agent ref from authoritative `agent_state_current` through a
    separate indexed exact-lookup/exposure projection; expose deterministic cursor-bounded
    listing without scanning all Sessions/branches and sorting each page. That lookup/index
    is rebuildable **from authoritative Agent state**. `agent_state_current` itself is not.
12. Introduce internal `StoreScope { StoreId, RestoreGeneration }` using the existing
    `store_meta.store_id` and `store_meta.restore_generation`, plus zero-cost `StorePos`
    for **commit_log.seq** positions. Ordered cursors, subscriptions/Handler progress
    and projection heads must bind to the Store incarnation as well as any selector,
    authority or projection generation. Same-Store backup restore increments the existing
    generation: old positions cannot silently resume against the restored sequence.
    Keep the private cursor identity/fence and projection head/swap helpers only;
    Phase-2 Events and Phase-4 Jobs reuse them, without global ordering or a universal
    cursor engine.

Qualification:
- one-time exclusive migration from live legacy Session state with a concurrent legacy
  lifecycle/ACL/audit writer attempt blocked by the migration boundary; activation sees
  either the complete legacy owner or the complete new Agent owner, never a half-seeded
  interleaving, followed by backup/restore without legacy-audit reconstruction;
- rebuild Agent lookup/exposure indexes from `agent_state_current` and verify rev/lifecycle/
  lineage/ownership remain unchanged;
- direct ACL/group-generation behavior;
- no Agent rev fanout from group membership;
- exact fork attribution;
- disclosure-closure expansion denial;
- manifest scalability: thousands of exact protected facts sharing the same disclosure
  policy/source authority structurally share manifest nodes and deduplicate authorization
  work to the unique authority root while preserving exact provenance; incremental union
  does not rewrite the whole closure and bounded work fails/defers safely;
- lazy sealing idempotence/reachability;
- exact Agent-ref lookup and mixed Job-list pagination at scale without O(all Sessions)
  per-request scans, with row-level current authorization;
- reject a StorePos/cursor from a different StoreId **or restore generation**; restore
  an earlier same-Store backup and prove old Event/Jobs positions become stale rather
  than replaying a different row at the same number; a projection rebuild never
  manufactures another Agent rev, owner or publication clock.

---

## Phase 2 — generic Events substrate

Build one Event owner before replacing legacy Job-only observer APIs. Introduce **one**
private `EventScan` over the existing ordered Event index, used by read, subscription and
Handler consumers. It checks StoreScope/placement (including restore generation), a
contiguous safe-prefix, source-first
visibility, and bounded authority/manifest evaluation. The scanner returns candidate /
proven-skip / blocked-unknown / stale / end, **not** a committed delivery or grant. The
three consumers retain separate result projection, Input-transaction admission and
Handler invocation/ack. Candidate authority is rechecked at each final owner boundary.

1. Project canonical resource refs to Event subjects, including reversible handle-token
   encoding such as literal `.` -> `%2E`.
2. Reuse immutable `commit_log.event_id` as `Event.id`; keep canonical subject/envelope
   in bounded payload/projection data rather than legacy compact `entity_id`.
3. Add source-local `event_rev` and transaction-coupled Event emission.
4. Add the rebuildable subject projection/index with one contiguous
   `indexed_through_commit_position` safe watermark. Newly authored Events write their
   projection transactionally where practical; recovery/backfill advances the watermark
   only through a gap-free commit-log prefix. No reader/subscription/Handler may advance
   past the safe watermark or a missing Event projection. Reuse the Phase-1 private
   projection-generation, StorePos/prefix and final head/swap helpers; Event subject-index
   row layout and ordered scan remain Event-owned.
5. Add `events.read` cursors, replay floor, hidden-row privacy and wait/wake behavior. Bind
   each opaque cursor to existing StoreScope (StoreId + restore generation) plus the
   **authority-scoped** selector placement generation. Hidden/unauthorized Domains do not perturb that generation.
   Authorization is source-first: proven source denial advances privately without evaluating
   deeper manifest/provider dependencies. Only for an already-visible source may an
   indeterminate dependency return retryable candidate-oblivious `authorization_unavailable`
   with unchanged cursor/progress; include no Event/source/position/dependency identity.
   Revalidate placement before scan and again before cursor-progress commit; mismatch in the
   caller's authorized selector scope returns `cursor_stale`. With wait_ms, wait on relevant
   authority dependencies until deadline before returning the same transient condition.
   Document that AW directly hides identities/counts/positions but does not promise constant-
   time/constant-volume resistance to coarse aggregate traffic inference.
6. Add durable Agent subscriptions with generation/baseline/lag semantics and the same
   StoreScope/authority-scoped placement affinity under creating-principal/tool authority.
   V1 subscription delivery requires the subscription/Event/causal owner and recipient
   Agent Input owner to share one authoritative transactional store; cross-store Agent
   delivery fails explicitly, while cross-Domain same-store delivery remains valid.
   Repeated `enabled=true` on an already-active same-subject row preserves generation,
   baseline and scan position; absent/disabled creates a fresh baseline, lagged re-enable
   creates a fresh generation/baseline, and disable fences later admissions. Placement
   mismatch stops delivery/pinning and reuses `lagged` until explicit rebaseline.
7. Deliver replay-safe EventInput through the existing Agent input owner. Candidate scans
   are advisory only; the input-owner transaction is the admission linearization point.
   Authorization order is source-first: prove current exact source visibility before
   evaluating deeper DisclosureManifest/provider dependencies. Proven source denial skips
   privately without touching those dependencies. For a visible source, atomically recheck
   active subscription generation/store affinity, recipient lifecycle + installer authority,
   store-local source/audience authority, disclosure ceiling, delivery dedupe and causal
   budget. Any externally mutable disclosure dependency must provide immutable/non-revocable
   evidence or a provider-enforced fence held valid through EventInput transaction commit;
   otherwise result is authorization_unavailable/Unknown and neither Input nor scan progress
   commits. Only proven deny/nonmatch advances. Disable/close/revocation committed first
   denies; EventInput admission committed first remains factual.
8. Enforce exact source + recipient audience + DisclosureManifest authorization.
9. Persist causal root/hop/delivery safety projection so loop bounds survive restart/replay.
10. Introduce the final typed `EventTypeRegistry` leaf owner and use it for custom `#name`
    schema identity/validation. From day one it mutates published state only through the
    shared private staged-registry transaction/snapshot primitive; Phase 2 performs a
    one-leaf stage/validate/publish. This is the same registry later composed into
    `ExtensionRegistry`; do not create a temporary Event schema registry.
11. Add model `events_subscribe` / `events_publish`.
12. Enforce **one-store v1 cross-Domain scope** under the host's authoritative Domain
    placement universe. A broad selector is complete only for currently authorized matching
    Domains whose placement the existing owner can prove. Hidden Domains never perturb
    caller-visible affinity; if an authorized selector cannot be proven wholly served by one
    Event store, fail/stale explicitly. Do not fabricate merged cursor positions or silently
    return partial results. Exact source/recipient authorization still applies.
13. Migrate v36 SourceEventBook/pending-delivery state using one source-owner serialized
    cutover transaction with **`delivery_cutover_rev = current next_event_rev`** before
    releasing the source owner. Entire revisions (including multiple names sharing one rev)
    belong to one side: below cutover legacy-owned, at/above cutover DL-200-owned. Install
    post-cutover subscription baseline/state and fence legacy new admission in that same
    transaction. Already accepted legacy Inputs retain identity and gain an immutable
    compatibility disclosure/provenance sidecar keyed to that exact fact; reconstruct the
    narrowest source/dependency/audience evidence available and mark incomplete provenance
    conservatively `legacy_protected` so future audience expansion fails closed. Only
    genuinely pending pre-cutover deliveries bridge once via deterministic migration
    identity. Crash/restart rereads the cutover rev; no revision can be admitted by both
    paths and migration never declassifies accepted historical Inputs.

Qualification:
- wildcard cardinality/terminal semantics;
- encoded literal handles;
- hidden-source cursor privacy;
- replay/dedupe/lag and repeated active `enabled=true` preserves backlog/generation;
- projection gap safety: delete/rebuild the Event projection while publishing/reading and
  prove no consumer advances past an unindexed committed Event, with no silent loss/double
  delivery beyond the documented at-least-once Handler/Input semantics;
- authority-scoped StoreScope/placement affinity (including same-Store restore generation):
  hidden Domain creation/move does not stale/lag an unauthorized caller, while newly
  authorized or migrated cross-store scope
  makes read cursors `cursor_stale` and subscriptions/Handlers lagged until explicit
  rebaseline, with no partial old-store world;
- race Event delivery against unsubscribe, Agent close, participant/source revocation and
  recipient ACL broadening; only the input-owner transaction winner determines admission;
- hidden source with an unavailable deeper dependency: prove source denial first, skip it
  privately, perform no deeper provider lookup and expose no authorization_unavailable;
- visible source with transient group/security/DisclosureManifest/external-provider authority:
  `events.read` returns opaque `authorization_unavailable` with unchanged cursor; subscription/
  Handler uses bounded backoff and progress does not advance until proven allow/deny or
  ordinary lag/retention takes over. External mutable dependency fence must survive the
  EventInput commit;
- attempt subscription delivery to an Agent Input owner in another authoritative store:
  creation/re-enable fails explicitly; same-store cross-Domain delivery remains valid;
- one EventScan exercised through **all three** consumers: a proven hidden source skips
  without deeper dependency work; an indeterminate visible source blocks every consumer
  without progress; accepted subscription candidate is rechecked atomically at Input
  admission; Handler ack stays at-least-once; no second durable queue is introduced;
- source+manifest disclosure;
- self-delivery suppression;
- causal 16-hop / 256-delivery persistence;
- custom typed-vs-generic Event decoding;
- EventTypeRegistry one-leaf staged publication is atomic from its first implementation;
- EventTypeRegistry identity/handles survive later composition into ExtensionRegistry with no
  migration/re-registration owner swap;
- same-store cross-Domain authorized selection and explicit failure for multi-store
  cross-Domain selectors, without partial cursor results;
- migration cutover race: set cutover exactly to current next_event_rev while publishing
  around the boundary (including multiple Event names sharing that revision), with pending
  and already-accepted legacy deliveries; crash/restart proves every source revision has
  exactly one delivery owner/identity. Accepted legacy Inputs keep identity and receive
  conservative compatibility disclosure provenance, never re-admission/declassification.

---

## Phase 3 — logical Sandbox, Sources, Checkpoints and Vault grants

Implement the logical resource layer without promoting frozen physical mount/config-v13
work. Use the Phase-0B shared Prepared/ExternalEvidence vocabulary and Phase-2 typed
registration stage, not new operation/driver/registry owners. One internal durable
`DriverKey` (stable extension/built-in namespace + driver ID only) identifies the durable
driver. Each SourceBinding, SourceAnchor and RecoveryDescriptor stores its own adjacent
artifact schema version. Keep runtime registry generation and provider-held non-secret fence
separate. Store-open/recovery resolves `DriverKey + artifact schema` through the same typed
registry compatibility check.

1. Add `sandboxes_current`, named SourceBindings and durable Agent SandboxGrant evidence.
   Enforce one host/runtime `max_sources_per_sandbox` across bindings and every derived
   SourceSet/RecoveryDescriptor set: default 128, hard absolute ceiling 1024, with hosts
   allowed to configure lower only. Reject an over-limit capture/derive/restore before any
   external preparation/effect.
2. Keep Agent->Sandbox binding immutable.
3. Add host `sandbox.list/get/create/close` and `sandbox.sources.list/set`.
4. Implement open/closed lifecycle and quiescent source replace/derive.
5. Introduce the final typed `SourceDriverRegistry`, `SandboxDriverRegistry` and
   `VaultProviderRegistry` leaf owners as they become needed in this phase; Git is the
   mandatory first SourceDriver. Use the same private staged-registry primitive introduced
   in Phase 2, with one-leaf stage/validate/publish before the composition root exists.
   SourceDriver's private prepared-operation contract is symmetric for **Capture and
   Restore**: before any effectful snapshot/materialization, plan with the operation-owned
   prepare identity, persist the descriptor in the existing operation receipt, then
   execute/inspect/cleanup that exact descriptor after loss/restart. Durable descriptors are
   non-secret: opaque IDs/digests/version proofs/provider/Vault-held capability references
   only—never bearer tokens, signed credential URLs, raw secrets or serialized credentials.
   These exact registries are later composed into `ExtensionRegistry`, never migrated/
   replaced by parallel owners.
6. Implement Git clean/dirty checkpoint anchors:
   - private-index synthetic commit for dirty state;
   - private retention ref for both clean and dirty anchors;
   - exact tracked + nonignored untracked regular-file behavior;
   - explicit submodule gitlink semantics.
7. Add Checkpoint/SourceSet storage with captured access/checkpoint-policy layout metadata.
8. Add multi-source barrier/capture/revalidate + crash reconciliation.
9. Land the one **private lower construction primitive** used by both restore and later
   `agent.create`. Before its first external/materializing effect, durably claim the existing
   replay/operation owner as `Prepared` with semantic fingerprint, candidate IDs and the
   per-driver recovery/source-version/fence descriptors. `Prepared` after crash means an
   external effect may have happened and triggers inspect/recover, never blind repetition;
   uncertainty becomes `Unknown` in that same owner. Hold AW-owned source mutation fences
   through final publication. The final AW transaction revalidates current AW-store-owned
   source/destination/grant/ACL/lifecycle authority **inside the publication transaction**,
   publishes resources and transitions the same claim to `Applied`; that commit is the
   **only** public visibility/linearization point. Externally owned source evidence must be
   either immutable/non-revocable for the publication interval or protected by a provider-
   enforced fence/lease held valid through the entire AW commit; a pre-commit validity check
   alone is insufficient. Inability to guarantee that leaves Prepared/Unknown rather than
   publishing. Post-commit scheduler wakes/cache refreshes are
   reconstructible accelerators and never gate discoverability. This is not a new service or
   public API and must not duplicate Agent/Sandbox owners.
10. Add `checkpoint.list/get/create/restore/delete`; `checkpoint.restore` uses that shared
    lower constructor to publish the new Agent + new Sandbox only after exact HistoryBase +
    SourceSet preparation succeeds.
11. Restore always creates new Agent + new Sandbox; never mutates an existing environment.
12. Add VaultProvider/VaultGrant storage and `agent.vault.list/set`.
13. Keep projection kinds provider/host-defined safe profiles; no arbitrary env/path/mount
    target language.
14. On store-open/startup, before serving/effectful source operations, validate every live
    SourceBinding, live Checkpoint SourceAnchor/retention dependency and unresolved
    Prepared/Unknown RecoveryDescriptor against the installed SourceDriver registration +
    durable recovery schema. Missing/incompatible dependencies put affected source-dependent
    resources in fail-closed degraded/recovery-required state; never reinterpret/retry/clean
    them with another driver. A compatible replacement must explicitly validate the old
    schema before serving effects.
15. Keep immediate revocation factual: already-materialized capability may require Process
    termination.

Qualification:
- clean/dirty Git exactness and retention after reset/rebase;
- submodule behavior;
- multi-source all-or-nothing plus cardinality bounds: default <=128 named sources/Sandbox,
  hard <=1024, with descriptor/transaction size automatically bounded and rejection before
  external effects;
- cross-Project source authorization;
- SourceDriver/SandboxDriver/VaultProvider one-leaf staged publication is atomic from first
  use; registry identity is final here and later ExtensionRegistry composition adds no
  shadow/migration owner;
- Sandbox sharing vs authority;
- grant actor ceilings and ACL broadening;
- checkpoint history-only/selected/auto coverage;
- restore uses the same replay-safe prepare/publish/reconcile constructor later consumed by
  `agent.create`; no restore-specific constructor/owner and no half-published Agent/Sandbox;
- crash/fault during effectful SourceDriver **capture or restore** leaves a durable Prepared
  or Unknown claim with stable candidate/recovery/source-version/fence identity; recovery
  inspection never repeats snapshot/materialization effects blindly, and persisted
  descriptors contain no reusable credentials/secrets;
- race Agent/source/destination authority revocation or source replacement against final
  publication: revoke/replace committed first denies publication without repeating external
  preparation; publication committed first retains the factual created result. External
  immutable-version evidence may publish directly, while a revocable provider
  fence/lease must remain held through the AW commit; expiry/revocation uncertainty remains
  Prepared/Unknown rather than publishing;
- restart after deployment with a live binding/checkpoint/unresolved descriptor whose exact
  driver/schema is missing or incompatible fails closed before source effects; reinstalling
  exact compatible driver or explicit schema-compatible replacement restores reconciliation;
- crash immediately after authoritative AW commit but before wakes/cache refresh still leaves
  the new resources visible/recoverable from committed state; no second visibility bit;
- no credential/lease inheritance;
- persisted DriverKey recovers the same compatible implementation after a restart, while
  incompatible replacement remains blocked by unresolved Prepared/Unknown descriptors;
  external evidence never becomes a cached permission or a leaked credential.

---

## Phase 4 — unified Agent lifecycle, input plan and Jobs surface

Consume the existing Phase-1 **StorePos + cursor envelope + projection swap** helpers for
Jobs high-water pages. Do not reuse EventScan for the different hierarchical frontier or
create a second projection coordinator. Preserve the original authoritative JobManager,
actor admission, writer and exact read/ACK owners.

1. Add public host `agent.create`, `list/get/edit/close/admission`,
   `access.list/set`.
2. Expose fresh/fork construction through the **existing Phase-3 lower constructor**, adding
   the exact settled-Turn/history selection and public admission/defaulting rules; do not
   create another constructor/publication path.
3. Reuse the same authenticated mutation replay identity/fingerprint from existing tool/RPC
   envelopes; do not add per-method request IDs.
4. Implement model `agent(task, ..., from?)` with inherited source Project/ACL default and
   host-selected Sandbox policy through that same lower constructor.
5. Add host staged input plan:
   `list/add/edit/delete/send`, `inputs_rev`, creator/editor provenance, exact
   steer/interrupt Turn target and atomic Send.
6. Freeze execution-adjacent mutation at closing; retain closed history/access/checkpoint
   management.
7. Finalize model Jobs as exactly:
   `jobs_list/jobs_get/jobs_read/jobs_send/jobs_ack/jobs_cancel`.
8. Make `jobs_list` pagination stable without a database snapshot: one traversal is local to
   one authoritative exposure store. First page captures the existing immutable publication
   high-water and opaque cursor affinity; later pages consider only rows at/below it with the
   exact tuple `(publication_position DESC, canonical_ref ASC)`, and cursor state retains
   both fields. Cross-store recursive Jobs federation is explicitly unsupported/fails
   closed.
9. Keep the rebuildable exposure projection gap-free for cursor creation: normal publication
   writes exposure evidence transactionally where possible. A full rebuild captures head H
   and builds privately through H, then under the final write/swap fence either verifies
   `current_publication_head == H` or catches the replacement up for `(H,current_head]`
   under that same fence before atomically switching generations; otherwise abandon/retry.
   Never swap an H-stale replacement over newer active rows or issue a high-water over an
   index containing a hole at/below it. An equivalent contiguous safe-prefix scheme is
   acceptable only if online incremental repair is actually required.
10. For `scope=tree`, retain deterministic traversal frontier + Agent ancestor-gate chain
    privately. A stored frontier is navigation, never authority: reauthorize every resumed
    Agent frontier/ancestor gate before descending; if any becomes hidden, discard that
    entire queued subtree even when a descendant remains directly viewable. Gaining access
    later does not rewind an already-scanned cursor.
11. Row-authorize emitted child/tree rows; lineage never grants visibility.
12. Do not fabricate destination Jobs for inherited historical Process/Graph/tool facts.
13. Converge Process stdin/EOF/cancel into Jobs while retaining lower CAS/Prepared/Unknown
    evidence.
14. Retire model queue CRUD, pause-after-turn and Jobs-specific subscribe.
15. Preserve Graph ownership exactly: nested Agents are allowed within existing bounded
    causal/depth/cost/authority ceilings, but `batch`/Graph construction is prohibited
    transitively while an Agent or descendant remains owned by an active Graph node.
16. `jobs_get(rev,wait_ms)` registers with the existing authoritative Agent/Process/Graph
    revision notifier, **then rechecks the owner revision** before sleeping. On wake,
    timeout or restart it reauthorizes and rereads; no fixed 10 ms database polling,
    lost-wakeup race, or second stateful notification service. Reuse Phase-1 exact Agent
    lookups and index-backed pagination for Jobs discovery.

Qualification:
- constructor replay;
- fork source-vs-history defaults;
- parent vs history ancestry;
- staged input CAS/send atomicity;
- close lifecycle;
- Jobs exact-target authorization;
- paged `jobs_list` under concurrent Agent/Process/Graph creation: exact
  `(publication_position DESC, canonical_ref ASC)` ordering with both tuple fields in cursor;
  no duplicates/misses within the captured high-water, no post-high-water children until a
  fresh listing, and no cursor high-water over a partial exposure rebuild;
- publish immediately after rebuild snapshots H but before swap; the final swap must catch
  up that publication under the fence (or reject/retry), never erase it from the active
  projection;
- page-1 sees Agent parent A, revoke A while descendant B remains directly viewable, then
  page-2 tree traversal must discard A's queued subtree and not expose B through traversal;
  direct `jobs_get(B)` remains independently authorized;
- cross-store parent/child traversal fails explicitly rather than merging publication logs;
- inherited history reads/no ghost Jobs;
- nested Agent under active Graph ownership allowed within existing bounds;
- direct nested `batch`, Agent-mediated `batch`, and descendant-Agent `batch` all denied
  while that Graph ownership is active; a genuinely independently admitted later Turn
  after release regains ordinary top-level batch eligibility;
- legacy compatibility during migration;
- one conditional Agent `jobs_get` wait with an update between registration and first
  read (no missed wakeup), plus indexed exact lookup/pagination and revocation-safe reread.

---

## Phase 5 — native typed ExtensionRegistry

Compose the **same** leaf registry implementations staged in Phases 2/3. Shared private
registration key, typed Handle, local revision, staged snapshot and atomic-publish code
is factored once; family-specific schema, pinning, Tool/Provider binding and access remain
separate. Install definitions without automatically granting/activating anything. Introduce
remaining registry families only with their actual first consumers.

1. Rename current hook-only `Extension` role to `Hook`; reclaim `Extension` for the
   installed unit.
2. Add one typed `ExtensionRegistry` composition/publishing root over the **same final leaf
   registries introduced earlier** (`EventTypeRegistry`, `SourceDriverRegistry`,
   `SandboxDriverRegistry`, `VaultProviderRegistry`) plus the remaining registries. Reuse
   their existing staged-registry primitive so one Extension can stage several leaf deltas,
   validate all, then publish one atomic root snapshot/generation. Do not replace, migrate
   or shadow those owners.
3. Keep registration transactional and extension-owned; installed definitions are inert
   until explicitly activated/bound.
4. Add the remaining Tool/Hook/Action/Resource/View/EventHandler/Provider/ProviderHook/
   ProcessHook/UI/Setting/State/Compactor registries and compose all leaf registries under
   the one root.
5. Keep typed handles registration-local; unrelated root generations do not stale them.
6. Add scoped runtime `ctx.tools/state/events/settings/models/process/ui` ports.
7. Add typed State keys with per-key seq/CAS for snapshot-relative updates.
8. Add View enrichment with per-View errors that never fail the core snapshot.
9. Add durable EventHandler activation/baseline/cursor/lag/suspend semantics over commit_log,
   using the same StoreId/selector-placement affinity and explicit rebaseline on stale
   placement.
10. Keep Compactor as context projection only; never rewrite HistoryBase.
11. Validate Histate-style fully typed registration without JSON/stringly Rust domain types.

Qualification:
- atomic multi-leaf registration rollback using the same leaf-stage primitive introduced in
  Phases 2/3; no immediately-mutating leaf wrapper or migration layer;
- definition vs activation;
- typed-handle reload behavior;
- State CAS;
- View isolation;
- EventHandler replay/retention;
- provider/process hook authority;
- earlier EventType/SourceDriver/SandboxDriver/VaultProvider handles/definitions remain the
  same leaf-owner identities after root composition;
- unresolved Prepared/Unknown SourceDriver RecoveryDescriptors plus live SourceBindings/
  Checkpoint anchors pin the exact driver registration + recovery schema both at runtime and
  across restart/deployment. Removal/incompatible reload/startup is refused/fail-closed
  until terminal reconciliation; compatible replacement is allowed only when it explicitly
  supports and validates the original durable schema;
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

Retire v36 compatibility state after the original authoritative cutover and accepted
Input/history receipts are safely retained. Contract obsolete Event cutover state only
when no legacy delivery can legally recur; legacy provenance becomes normal immutable
DisclosureManifest dependencies, not a permanent special reader. R6's design gate already
normalizes documentation into one README mental model, one normative SPEC and historical
decision records; Phase 7 only synchronizes final status/handoffs/runtime-health wording
with implemented reality rather than reopening architecture.

1. Remove public Session vocabulary/surfaces while retaining internal SessionStore.
2. Remove Signal type/terminology.
3. Remove Jobs-specific subscribe and obsolete queue/pause helpers.
4. Remove temporary duplicate owner/projection layers only after callers are cut over.
5. Migrate persisted legacy state with explicit one-time adapters; no forever-parallel
   runtime.
6. Keep temporary SDK source wrappers client-only; one semantic wire/runtime surface.
7. Update docs/handoffs/runtime-health guidance to the final API, including **one current
   DL-200 status** in both AW and dotlink: sealed, separately implementation-promoted,
   Phase 0 active; keep earlier unsealed/design-only labels strictly in historical notes.
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

DL-200 implementation was explicitly promoted on **2026-11-21**. Phase 0 is active.
Later phases proceed only after the preceding coherent checkpoint is reviewed/accepted;
commit/push/release/deploy remain separately user-gated.
