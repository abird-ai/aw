# AW + dotlink 0.17 shared-runtime full comprehensive handoff — 2026-11-18

Author: coordinating implementation agent
Owner: @pvl
Source-authoring host: L5 only
Canonical AW checkout: /home/pvl/spaces/abird/src/aw
Canonical dotlink main checkout: /home/pvl/spaces/abird/src/dotlink
Canonical dotlink release reference worktree: /home/pvl/spaces/abird/src/dotlink-017-mounts

This document is the durable context package for the current AW-first / dotlink-thin-consumer workstream.

For this workstream, it supersedes older dated handoffs as the current state summary. Older comprehensive handoffs remain historical evidence and should still be read when a detail predates this document.

## 0A. LIVE P8 SUPERSEDING UPDATE — 2026-11-18

This section supersedes any later historical prose in this file that still
describes P1/P2 as pending or mentions the rejected transitional
`StopOrigin/Cleanup/Dropped` design. Do **not** follow any later "first task"
instruction that conflicts with this section or the active plan.

P1 through P7 are implemented in the current AW working tree. P8's
host-available technical checkpoint is green. P9 (dotlink thin-consumer
conversion) is the next implementation phase.

Current AW refs remain:

    HEAD        021b8cec7b9122a6f6acaf5ef9137fb31f0a6659
    origin/main 792f0b38d104659264b70c31736655df1a46b60f

The accumulated P1-P8 implementation is intentionally **uncommitted** and
nothing is staged because the user has not authorized staging/commit/push in
this workstream.

Current dotlink refs remain:

    HEAD        6598de605a006a75473a2043de3f60db384b0d02
    origin/main 5381729cd469eda54691e540deb3fee13e9f3e3e

Dotlink source is still untouched by P1-P8. Only its companion consumer-plan
README/TODO are dirty.

Release authority remains:

    release/0.17 = 887401398da297c458ffa2a152a07e1b85870c35
    v0.17.4 tag baseline = fd1cdb6429a341a4e3d27805cae0bec288fc26df

### Final shared semantics implemented in AW

Process stop core:

    StopReason::{Timeout, Cancelled, RuntimeError}
    StopEvidence::{Stopped, NoTarget, Unknown}
    StopRecord {
        reason: Option<StopReason>,
        evidence: StopEvidence,
    }

There is no `StopOrigin`, `Cleanup`, or `Dropped` canonical stop reason.
`reason=None` is housekeeping. Only `Stopped + Some(reason)` can contribute
a terminal semantic stop cause. Physical `ProcessExit::{Code,Signal,Unknown}`
remains orthogonal and no stop request invents a signal number.

Platform stop evidence:

    aw-platform-* -> io::Result<bool>
      Ok(true)  = proven live target reached/stopped
      Ok(false) = positive NoTarget
      Err       = Unknown

The adapter maps this once to `StopEvidence`. Cross-platform
`ManagedProcessGuard` disarms on Stopped/NoTarget and stays armed on Unknown.
A later pre-publish D1 source audit found and resolved the last platform fact
gap: `aw_platform::exit_signal(&ExitStatus) -> Option<i32>` now owns Unix
physical signal extraction (Windows returns `None`), and AW's process adapter
consumes that same helper rather than its own cfg-sensitive branch. Fresh gates:
Unix platform **13/13 + 4/4 PTY**, adapter **256/256**, terminal **20/20**, strict
affected Clippy, minimal configs, downstream host/CLI and workspace all-feature
compile. Native Windows Job Object/exit-signal assertions remain authored but
unexecuted because L5 has no Windows Rust std target installed.

Process/Task/event propagation:

- `ProcessTerminal { exit, stop, output }` retains physical and semantic facts.
- `TaskTerminalDetail { exit, stop }` is the compact Task projection.
- `ExecutionLifecycleEvent::ProcessStop { record }` is separate from Job wake
  events.
- optional `ExecutionEventSink` records stop attempts; reason-less
  housekeeping cannot become a semantic terminal cause.

Budget split:

    RawPageBytes
    SerializedResultBytes

`JobProfile::resolve_read_page` owns raw retained-page bytes.
`SerializedResultBytes` owns actual serialized result envelopes. A one-byte
raw page can live inside a normal larger MCP response envelope.

Finite Jobs refs/receipts:

    JobRef = j:<handle>[/n:<coordinate>]

Finite model/product output uses `JobRef`; hierarchical `ExecutionRef`
remains internal ownership/Agent-routing identity. Shared finite receipt grammar
is `JobStatus / NodeStatus / Handoff / Cancel / Ack(Finish|Through)`.
`render_handoff` may reduce richness under a tight budget but always names the
same durable Job and keeps explicit `no-rerun` semantics.

Bounded diagnostics:

- shared UTF-8-safe `bounded_diagnostic`, 16 KiB;
- Task diagnostic bounding reuses it.

MCP result metadata/fitting:

- `McpToolResult` preserves bounded result-level `_meta`;
- `io.abird/receipt.ts` is the shared stored receipt timestamp;
- stored replay timestamp wins over a fresh host timestamp;
- visible `ts=` sidecar is added only when absent;
- `fit_complete_result` measures actual serialized `CallToolResult` bytes;
- only text may shrink;
- media/resources, structured JSON, result metadata, result type, error class,
  block order and per-block metadata remain immutable;
- if immutable facts cannot fit, fitting fails explicitly instead of rewriting
  a typed result as prose.

MCP progress/heartbeat:

- `ProgressHeartbeatConfig`, `ProgressHeartbeat`,
  `ProgressHeartbeatSummary`, `ProgressHeartbeatObserver`;
- `await_with_progress_heartbeat` emits monotonic unknown-total/no-message
  ticks only when a request token exists;
- each send is independently bounded;
- the underlying operation remains polled while a send is backpressured;
- no detached queue/task;
- observer summary is emitted via RAII on normal return or wrapper cancellation;
- real semantic progress uses the existing
  `filesystem::progress::End::{Known,Unknown}` through
  `semantic_progress_notification` / `notify_semantic_progress_bounded`.

### P8 validation state

Green on L5:

- `aw-tools --all-features --lib`: **354/354**
- `aw-tools-adapter --all-features --lib`: **256/256**
- `aw-mcp --all-features --lib`: **33/33**
- `aw-platform-unix`: **13/13**, live PTY integrations **4/4**, cross-UID
  fixture intentionally ignored
- `aw-terminal --lib`: **20/20**
- strict all-target/all-feature Clippy for every touched crate
- no-default `aw-tools`
- batch-only `aw-tools`
- no-default adapter
- no-default Unix platform
- host-side Windows platform check
- downstream `aw-host` and `aw-cli`
- full workspace all-feature compile
- rustfmt and `git diff --check`

P8 API review tightened two surfaces:

- `ProgressHeartbeatConfig` fields are private so callers cannot bypass the
  positive-duration validating constructor; getters remain available.
- `JobRef` fields are private behind its typed constructor/accessors.

Still authorization-gated:

- stage/sign/commit accumulated AW P1-P8 implementation;
- publish/push;
- repin dotlink to an exact published AW commit.

Do not perform any of those without explicit user authorization.

Frozen sandbox scope remains untouched:
`.agents/plans/017-sandbox-mount-convergence-2026-11-18` plus sandbox source
paths have no diff.

## 0. Authority order

When anything conflicts, use this order:

1. direct user instruction in the current conversation;
2. live source and live tests on L5;
3. active owning plan README / DESIGN / AUDIT / TODO;
4. this comprehensive handoff;
5. older dated comprehensive handoffs and archived plans.

Never treat an old release implementation as architectural authority merely because it predates the shared AW runtime. The release lane is reference behavior. Portable semantics should live in AW; dotlink should consume them.

## 1. Non-negotiable operating instructions

- Use dotlink---l5 only.
- Do not use A1.
- Do not create another worktree.
- Work only in the canonical AW and dotlink paths listed above.
- Preserve unrelated dirty and staged work exactly.
- Do not commit, push, tag, release, deploy, publish or change release versions unless the user explicitly authorizes that action.
- Keep implementation turns to roughly 30 minutes.
- Aim for the next coherent technical checkpoint, update durable plans/handoff evidence, then pause.
- Revalidate refs/status/staging before every new source-edit phase.
- Read live source/tests for every owner you intend to touch.
- Keep sandbox/mount implementation frozen until the user explicitly asks to promote/pull the sandbox plan. The sandbox design may be read, reviewed and refined, but not implemented.
- Do not resurrect retired Jobs/subagent routes, a second JobManager, old job_output APIs, line-tail semantics, or release-local duplicate runtime state.
- Keep AW as the canonical reusable runtime semantics layer. Dotlink should be a minimal consumer/projection/configuration/transport layer wherever practical.
- Maintain zero-cost / allocation-light shared types where the design allows it.
- Prefer clean, simple, future-proof shared APIs over product-specific patches.
- Never infer authority from labels, paths, tabs, browser auth, or presentation metadata.
- Keep product logging/prose separate from canonical shared facts.

## 2. Exact current Git state

### 2.1 AW

Current local AW main:

    021b8cec7b9122a6f6acaf5ef9137fb31f0a6659
    Add shared process stop evidence types

Published GitHub AW main and refreshed local origin/main:

    792f0b38d104659264b70c31736655df1a46b60f
    Allow scoped extended Agent tool results

AW local main is three commits ahead of published origin/main:

    796225e  Separate process signal identity from lifecycle
    63f4056  Plan shared-first 0.17 runtime convergence
    021b8ce  Add shared process stop evidence types

Important: 021b8ce is a committed invariant-proving transitional P1 API. It is NOT the final approved host-facing stop API. The very next implementation action must simplify it before P2.

Current AW dirty files before this handoff:

    M .agents/docs/CURRENT_HANDOFF.md
    M .agents/plans/017-shared-runtime-convergence-2026-11-18/AUDIT.md
    M .agents/plans/017-shared-runtime-convergence-2026-11-18/DESIGN.md
    M .agents/plans/017-shared-runtime-convergence-2026-11-18/README.md
    M .agents/plans/017-shared-runtime-convergence-2026-11-18/TODO.md
    M .agents/plans/ai-native-tool-output-grep-2026-11-15/TODO.md
    M .agents/plans/ai-native-toolset-builder-2026-11-15/TODO.md

The CURRENT_HANDOFF / ai-native-tool-output-grep / ai-native-toolset-builder edits predate the latest stop-vocabulary correction. Preserve them. The four 017-shared-runtime files are current owned design edits from the latest conversation.

Nothing is staged.

### 2.2 dotlink main

Current local dotlink main:

    6598de605a006a75473a2043de3f60db384b0d02
    Plan v0.17.4 shared consumer backport

Published GitHub dotlink main / local origin/main:

    5381729cd469eda54691e540deb3fee13e9f3e3e
    Complete shared Toolset metadata adoption

Local dotlink commits above published main:

    4f60bc3  Complete pre-0.17 shared convergence
    6598de6  Plan v0.17.4 shared consumer backport

Current dotlink dirty files before this handoff:

    M .agents/plans/017-main-shared-consumer-backport-2026-11-18/README.md
    M .agents/plans/017-main-shared-consumer-backport-2026-11-18/TODO.md

Those two edits update the consumer plan to the latest release head and latest approved StopReason / StopEvidence / StopRecord vocabulary.

Nothing is staged.

### 2.3 dotlink release/0.17 authority

Local release worktree is clean and exactly matches GitHub:

    release/0.17 = 887401398da297c458ffa2a152a07e1b85870c35

Tagged v0.17.4 baseline:

    fd1cdb6429a341a4e3d27805cae0bec288fc26df

Latest post-tag commit:

    8874013 test(jobs): ask the platform what it calls a signal

That post-tag commit only removes hardcoded platform signal-name expectations from release tests and uses the release platform signal-label helper. It reinforces the decision that signal naming belongs in AW platform/shared projection, not dotlink-local hardcoded tests.

## 3. Required reading order for the next agent

Read in this order before editing:

1. AW AGENTS.md
2. this document
3. .agents/plans/017-shared-runtime-convergence-2026-11-18/README.md
4. .agents/plans/017-shared-runtime-convergence-2026-11-18/DESIGN.md
5. .agents/plans/017-shared-runtime-convergence-2026-11-18/AUDIT.md
6. .agents/plans/017-shared-runtime-convergence-2026-11-18/TODO.md
7. .agents/plans/017-stop-provenance-2026-11-18/README.md
8. .agents/plans/pre-017-shared-convergence-2026-11-18/README.md
9. .agents/plans/pre-017-shared-convergence-2026-11-18/TODO.md
10. .agents/docs/AW-DOTLINK-TOOLS-FULL-COMPREHENSIVE-HANDOFF-2026-11-17.md for D1-D4/B6 historical detail
11. .agents/docs/AW-FULL-COMPREHENSIVE-HANDOFF-2026-11-15.md for earlier AW/Workspace history where needed
12. dotlink AGENTS.md
13. dotlink .agents/plans/017-main-shared-consumer-backport-2026-11-18/README.md
14. dotlink .agents/plans/017-main-shared-consumer-backport-2026-11-18/TODO.md
15. release/0.17 source/tests/CHANGELOG at 8874013 when comparing behavior
16. .agents/plans/017-sandbox-mount-convergence-2026-11-18/README.md only as read-only design context

Do not start by reading the old release backport TODO and assuming it is current. The final release source/tests/changelog and the active AW master plan outrank the old backport prose.

## 4. Workstream chronology

### 4.1 AI-native tools and Graph foundations — October / early November

Dotlink's AI-native filesystem/Graph design work converged around:

- canonical native filesystem tools;
- Graph-native batch execution;
- durable Jobs;
- compact, token-efficient receipts;
- shared Job/Graph runtime semantics;
- exact execution policy and authority;
- no duplicate foreground/background execution paths.

Relevant historical dotlink plan/commit line includes the October 31 AI-native filesystem and Job Graph design sequence ending around:

    c6f0e1c  spec AI-native filesystem tool surface
    9b7d4b4  refine AI-native filesystem limits and edit
    6c1cb16  specify filesystem status and batch grammar
    f28ce2c  design batch execution graph
    ebb5b84  seal AI-native filesystem design
    8a3e7bb  unify durable batch controls with jobs
    a156142  make jobs canonical graph runtime
    bec528f  finalize canonical job graph design
    81a54ce  seal reviewed AI-native job graph design
    c34ea5a  seal tool design edge cases

This design lineage matters because current shared AW semantics intentionally replace older release-local APIs such as job_output and line-tail defaults.

### 4.2 Unified Jobs + Agent runtime design — November 9–14

Cross-repo design moved Jobs, Graph execution, replay, authority, sessions and Agent runtime semantics into AW.

Important AW design/implementation checkpoints:

    406bfa5  Resolve unified Graph, replay, authority, and integration design
    890e9a3  Resolve shared Job, replay, wake, and Graph ownership design
    9e9ab04  Implement unified Jobs and Agent runtime checkpoint
    0a6c6a0  Complete unified Jobs and Agent runtime
    88cd965  Record finalized unified runtime pins

Important dotlink integration checkpoints:

    886e861  Align dotlink Start and Job contracts with AW design
    3f59bb1  Cut over standalone Jobs to shared AW runtime
    c90a660  Finalize dotlink unified Jobs bridge

Architectural decision established here:

- one canonical shared JobManager/runtime;
- Process/Graph/Task identities and replay are shared;
- dotlink must not retain a second independent Job runtime;
- Graph nodes are immutable;
- authority/policy checks remain explicit and layered;
- output ownership/delivery proof is part of correctness;
- background is a disposition of the same execution, not a second runner.

### 4.3 Workspace/remote-view work — November 14–15

AW also completed major Workspace/remote-view work:

    c9aaf0e  Complete delegated browser authority and surface lifecycle
    aaab161  Complete workspace views UX and remote trust
    62b8da6  Complete remote Workspace target integration
    7d09d5f  Advance workspace views, remote resources and handoff

This is not the active workstream now, but it remains a major existing subsystem. Do not regress it while editing shared tools/runtime. The user-designated .agents/plans/ux/** subtree remains read-only unless explicitly authorized.

### 4.4 Shared Toolset + Job-control merge — November 15–17

AW moved configurable tool metadata/defaults and more finite Job semantics into the shared layer:

    0d2b4e3  Build configurable toolset and harden shared Job control
    be6d48d  Harden shared Job output and journal uncertainty
    26a8c78  Preserve strict output ACK replay across compaction
    beef1bc  Harden Job writer quiescence
    7e6fe20  Align shared Job list metadata

Key decisions/fixes:

- Toolset is immutable after build.
- Filesystem/Graph/Job descriptors derive from shared profiles.
- Toolset metadata is presentation only; it does not grant authority.
- finite jobs_get / jobs_read / jobs_ack share one canonical JobManager.
- output delivery proof is exact-owner aware.
- ACK Through may only advance bytes that were actually delivered.
- omitted-offset Finish releases all owned streams only under the correct owner.
- lost-response replay of an already committed ACK remains valid after another owner compacts the physical retained prefix.
- aggregate jobs_get tail is a pure peek and grants no delivery/ACK credit.
- writer quiescence prevents terminal Job removal/pruning while retained OutputWriter activity remains live.
- jobs_get wait is event-driven/settlement-aware.
- jobs_read wait is byte/EOF aware.

Dotlink D1-D4 adoption and review converged through:

    d0a3922  Adopt reviewed AW Job output runtime
    4f343c3  Complete AW shared-tools convergence
    5381729  Complete shared Toolset metadata adoption

Full historical detail is in AW-DOTLINK-TOOLS-FULL-COMPREHENSIVE-HANDOFF-2026-11-17.md.

### 4.5 B6 Toolset metadata convergence

B6 centralized canonical filesystem, shell, Batch and finite Job metadata into one shared Toolset projection.

Important decisions:

- shared canonical titles/descriptions/argument schema facts;
- dotlink dynamic shell timeout/output ceilings remain product-owned;
- one shared Job profile drives advertised/runtime values;
- jobs_list lower bound moved into AW and dotlink shim was removed;
- list_tools / get_tool / inspection converge on the same metadata projection;
- Toolset does not replace sandbox/policy/replay/transport authority.

Dotlink published B6 checkpoint:

    5381729 Complete shared Toolset metadata adoption

### 4.6 Typed MCP media boundary — November 17

AW added a bounded raw MCP result boundary:

    d6009de  Preserve typed MCP media results
    30dc55a  Record typed MCP media checkpoint

The public aw_mcp::McpToolResult:

- preserves Text, Image, Audio, embedded Resource, ResourceLink and structured content;
- bounds content block count and semantic payload;
- rejects unknown future block kinds fail-closed;
- keeps ordinary Agent ToolOutput text-only;
- does not implicitly grant media/provider/device consent.

A real cross-repo stdio test used dotlink to prove:

- PNG -> image/png;
- WAV -> audio/wav;
- PDF -> embedded application/pdf resource.

Important architectural boundary:

typed MCP wire/result preservation is complete;
provider/model-visible media delivery, durable media history/receipts and consent remain separate future work.

### 4.7 Pre-0.17 release audit and convergence — November 18

A full semantic audit accounted for the 55 dotlink release-line commits before the v0.17.0 release commit.

Canonical AW plan:

    .agents/plans/pre-017-shared-convergence-2026-11-18/

The audit reduced the actual missing source work to three semantic deltas:

1. scoped result/receipt metadata and timestamps;
2. shared jobs_read page default/max;
3. dotlink synchronous shell omitted-output default.

AW implementation checkpoints:

    723ec24  Add scoped receipt metadata and Job page profiles
    792f0b3  Allow scoped extended Agent tool results

AW shared receipt design:

- ReceiptProfile selects semantic categories and explicit include/exclude.
- zero-allocation fixed masks.
- host supplies timestamp; AW owns no clock.
- immutable result/Task replay reuses the original receipt timestamp.
- fresh jobs_list/get/read observations get fresh timestamps.
- AW defaults timestamp/checkpoint policy off.
- dotlink opts into Process + Batch + Jobs timestamps and 30-minute model guidance.

AW Job read profile:

- default raw page 256 KiB;
- configurable max;
- shared hard max 8 MiB;
- schema and runtime derive from the same immutable profile;
- page semantics are separate from Agent/transport presentation budget.

AW Agent result budget:

- ordinary tools remain 128 KiB;
- trusted Host tools may opt into an absolute 256 KiB Agent result ceiling;
- ceiling is captured at registration;
- external MCP/extension sources cannot self-widen;
- mixed batch fair-share remains per-call;
- pass-through extensions do not break an extended trusted Host result.

Dotlink local pre-0.17 implementation:

    4f60bc3 Complete pre-0.17 shared convergence

That local dotlink commit is NOT published.

It includes:

- repin to published AW 792f0b3;
- timestamp scope Process + Batch + Jobs;
- checkpoint_minutes = 30;
- model guidance appended once;
- MCP meta receipt timestamp and visible ts block;
- replay-stable immutable result timestamps;
- fresh observation timestamps;
- 256 KiB omitted jobs_read default under configured max;
- 256 KiB omitted synchronous shell output default under configured max;
- wire tests for stdio and hosted OpenAI EmbeddedMcp;
- complete pre-0.17 semantic parity.

Qualification at that checkpoint:

- dotlink binary: 688 passed / 1 intentional ignored;
- integrations: 37/37;
- strict all-target/all-feature Clippy;
- fmt and diff checks;
- explicit stdio and hosted receipt timestamp wire assertions.

### 4.8 Fresh v0.17.x audit and shared-first redesign

The release lane was repeatedly refreshed during the audit.

Tagged v0.17.4:

    fd1cdb6

Canonical v0.17.4 stop/provenance implementation:

    c185558 feat(jobs,shell): report a stop, its cause and where it came from

Hardening commits:

    74820ff  bound caller-controlled Job IDs in every diagnostic
    06d5548  separate raw page bound from serialized response envelope behavior
    30099aa  reject Nix environment keys owned by the module
    5b27784  final operational/backport/runtime-health documentation

Current release branch is newer:

    8874013 test(jobs): ask the platform what it calls a signal

That post-tag test cleanup delegates signal labels to the platform helper rather than hardcoding Unix names.

Major architectural conclusion from the audit:

portable stop/process/Task/Job/MCP semantics should live in AW first;
dotlink should become a thin producer/consumer/projector.

This led to:

    796225e Separate process signal identity from lifecycle
    63f4056 Plan shared-first 0.17 runtime convergence
    021b8ce Add shared process stop evidence types

and dotlink planning checkpoint:

    6598de6 Plan v0.17.4 shared consumer backport

## 5. Current shared-first architecture

### 5.1 Physical process exit

AW ProcessExit is physical evidence only:

~~~rust
pub enum ProcessExit {
    Code(i32),
    Signal(i32),
    Unknown,
}
~~~

Rules:

- only observed/reaped OS status may produce Signal(n);
- a cancellation request never invents SIGKILL/SIGTERM/etc.;
- cancellation/deadline/revocation/failure are lifecycle/stop-reason facts, not physical exit variants.

796225e implemented and qualified this.

A real Unix regression proves SIGKILL 9 and SIGSEGV 11 remain distinct.

AW-requested cancellation can correctly be lifecycle cancelled while physical exit is Unknown.

### 5.2 IMPORTANT: 021b8ce is transitional, not final API

Current source in crates/aw-tools/src/execution_stop.rs still exposes:

- CancellationOrigin;
- ProcessStopCause;
- StopAttemptReason;
- StopOutcome;
- StopAttempt.

That API proved the required invariants, but the user explicitly rejected it as overloaded/confusing.

Do NOT begin P2 on those types.

The latest approved design supersedes them.

### 5.3 Final approved stop vocabulary

The next source change must refactor P1 to:

~~~rust
pub enum StopOrigin {
    Caller,
    Parent,
    Runtime,
    Authority,
}

pub enum StopReason {
    Timeout,
    Cancelled {
        origin: Option<StopOrigin>,
    },
    RuntimeError,
    Cleanup,
    Dropped,
}

pub enum StopEvidence {
    Stopped,
    NoTarget,
    Unknown,
}

pub struct StopRecord {
    pub reason: StopReason,
    pub evidence: StopEvidence,
}
~~~

This is the current authoritative design.

Meanings:

StopOrigin:
- Caller: explicit owner/user/model-facing control.
- Parent: parent execution cancels child/subagent execution.
- Runtime: host shutdown/restart/runtime replacement/owner teardown.
- Authority: source/grant/policy authority revoked/replaced.

Cancellation origin is optional:
- a host with evidence may attach Some(origin);
- a minimal host may use Cancelled { origin: None };
- AW must not force provenance a host does not know.

StopReason:
- Timeout;
- Cancelled with optional origin;
- RuntimeError;
- Cleanup;
- Dropped.

StopEvidence:
- Stopped: runtime has sufficient evidence its stop acted on a live target.
- NoTarget: positive knowledge that there was no live containment target left.
- Unknown: runtime cannot establish the resulting state; target may still be alive.

NoTarget and Unknown are intentionally distinct.

NoTarget:
- positive knowledge;
- desired terminal absence;
- disarm the Guard;
- no retry needed;
- does NOT attribute the disappearance to our stop.

Unknown:
- unresolved;
- target may still be alive;
- Guard may remain armed;
- fail-safe Drop retry may still be appropriate.

Never collapse NoTarget into Unknown with a reason field.

### 5.4 Terminal attribution rule

Only:

    evidence == Stopped

plus a claimable reason can contribute to ProcessTerminal.stop.

Claimable:
- Timeout;
- Cancelled { .. };
- RuntimeError.

Forensic-only:
- Cleanup;
- Dropped.

Required behavior:

    Timeout + Stopped
      -> terminal stop = Timeout

    Cancelled{Caller} + Stopped
      -> terminal stop = Cancelled{Caller}

    RuntimeError + Stopped
      -> terminal stop = RuntimeError

    Cleanup + Stopped
      -> no terminal stop

    Dropped + Stopped
      -> no terminal stop

    anything + NoTarget
      -> no terminal stop

    anything + Unknown
      -> no terminal stop

The preferred host API is ergonomic:

    StopReason::cancelled()
    StopReason::cancelled_by(StopOrigin::Caller)

    StopRecord::stopped(reason)
    StopRecord::no_target(reason)
    StopRecord::unknown(reason)

Exact constructor names may be refined, but do not reintroduce overlapping cause/reason/attempt/outcome taxonomies.

### 5.5 Future ProcessTerminal shape

Target:

~~~rust
pub struct ProcessTerminal {
    pub exit: ProcessExit,
    pub stop: Option<StopReason>,
    pub output: ProcessOutputMeta,
}
~~~

ProcessTerminal.stop is populated only from StopRecord::terminal_reason().

Do not add dotlink product fields such as:
- stopped_by;
- signal_name;
- product prose.

## 6. Active AW master plan

Canonical active plan:

    .agents/plans/017-shared-runtime-convergence-2026-11-18/

Files:
- README.md
- DESIGN.md
- AUDIT.md
- TODO.md

### P1 — immediate next work

P1 is NOT complete despite 021b8ce.

First action:

1. replace transitional execution_stop API with StopOrigin / StopReason / StopEvidence / StopRecord;
2. preserve Parent as a distinct optional origin;
3. implement terminal_reason attribution rule;
4. prove Cleanup/Dropped forensic-only;
5. prove NoTarget vs Unknown Guard semantics in pure unit tests;
6. keep types Copy / bounded / no-default-features;
7. remove old public names;
8. update plan evidence;
9. run focused + full aw-tools + strict Clippy + no-default + downstream adapter compile;
10. checkpoint and pause.

Do not begin P2 until P1 cleanup is green.

### P2 — platform evidence + managed process-tree Guard

After P1 cleanup:

- platform stop primitive returns Stopped / NoTarget / Unknown;
- Unix:
  - successful process-group signal -> Stopped;
  - ESRCH -> NoTarget;
  - other error -> Unknown;
- Windows Job Object:
  - proven empty/no active processes -> NoTarget;
  - proven live membership + successful termination -> Stopped;
  - unprovable query/termination -> Unknown;
- preserve unsandboxed Unix setsid limitation;
- RAII managed process-tree Guard;
- StopReason supplied by caller;
- Guard returns StopRecord;
- Unknown may remain armed for Drop retry;
- NoTarget disarms;
- Drop produces forensic Dropped reason;
- Cleanup is forensic only;
- shared platform signal-name helper;
- real platform tests.

The fresh 8874013 release test strengthens the signal-name-helper requirement.

### P3 — ProcessTerminal / JobManager / Task detail / lifecycle events

- add optional StopReason to ProcessTerminal;
- ProcessSettlement / JobManager retain exact terminal;
- preserve zero-allocation ordinary path;
- add typed TaskTerminalDetail;
- preserve reaped physical exit behind later spool/internal/projection failure;
- preserve prior deadline/disposition behind later Task failure;
- add ExecutionLifecycleEvent::ProcessStop with StopRecord;
- do not overload Job wake/subscription JobEventKind;
- optional execution-event sink, no-op when absent;
- process Guard emits event at actual platform stop site and Drop;
- Job lifecycle, ProcessTerminal, Task detail and lifecycle event must never contradict.

### P4 — raw page vs serialized response budget

The 06d5548 release fix exposed a generic semantic bug.

A one-byte jobs_read page must not imply a one-byte serialized MCP response envelope.

Target shared distinction:

- raw page budget: AW JobProfile / jobs_read semantic bytes;
- serialized result budget: aw-mcp / transport envelope.

Current dotlink consumer still needs a regression for jobs_read(limit=1) without transport-supplied result budget.

### P5 — typed flat Job refs + bounded diagnostics + shared receipts

Move more dotlink-local grammar into AW:

- bounded j:<handle>[/n:<coordinate>] typed ref;
- parse raw transport string once;
- internal APIs use typed bounded identity where practical;
- bounded shared diagnostics as defense in depth;
- shared durable Job handoff receipt;
- cancellation control receipt;
- reuse Finish/Through ACK;
- no-rerun continuation grammar shared;
- narrower replay projection may be more compact but cannot change effect/result identity.

This is the shared-first form of the 74820ff caller-controlled diagnostic hardening.

### P6 — aw-mcp receipt metadata + result fitter

Move generic dotlink result decoration into aw-mcp:

- preserve bounded result _meta;
- shared Toolset receipt decorator;
- host supplies timestamp;
- immutable replay timestamp reused;
- fresh observation timestamp regenerated;
- visible timestamp sidecar only when needed;
- preserve raw/structured/media payload;
- generic serialized CallToolResult fitter;
- preserve meta and error class;
- reserve receipt overhead;
- explicit error if minimal factual response cannot fit.

### P7 — aw-mcp heartbeat/progress adapter

Move generic MCP heartbeat semantics into aw-mcp:

- only when progress token exists;
- monotonic tick;
- bounded send;
- cannot delay/cancel underlying wait;
- no detached queue/task;
- optional host observer gets attempted/delivered summary;
- integrate with semantic progress when meaningful progress exists.

### P8 — shared qualification / publication boundary

- full affected AW suites;
- strict all-target/all-feature Clippy;
- minimal/no-default/no_std/stateless boundaries touched;
- downstream aw-mcp/host/CLI;
- review public API for unnecessary allocation/product wording;
- signed checkpoints;
- publish only with explicit user authorization.

### P9 — dotlink thin consumer

After published AW shared checkpoints:
- repin all AW dependencies atomically;
- remove/reduce dotlink duplicate semantics only after parity tests;
- project AW facts into dotlink wire/log wording;
- run full release equivalence.

## 7. Dotlink active consumer plan

Canonical plan:

    /home/pvl/spaces/abird/src/dotlink/.agents/plans/017-main-shared-consumer-backport-2026-11-18/

### D0 — dependency boundary

- wait for reviewed AW shared phases;
- publish AW only with explicit authorization;
- repin all AW Git dependencies and lock records atomically;
- no path/sibling dependency;
- preserve local dotlink work.

### D1 — process provenance thin consumer

- consume shared process Guard and StopRecord;
- map ProcessExit::Signal(n);
- no parallel stop_proven -> Cancelled/DeadlineExceeded model;
- compatibility projection:
  - Timeout -> stopped_by="timeout";
  - Cancelled{Caller|Parent} -> stopped_by="cancel";
  - Cancelled{Runtime|Authority} -> stopped_by="shutdown";
  - RuntimeError -> stopped_by="failed";
- only when canonical evidence is Stopped;
- external signal -> signal number, no stopped_by;
- Cleanup/Dropped forensic only;
- NoTarget vs Unknown remain distinct;
- preserve actual deadline/race semantics.

### D2 — execution lifecycle event -> operator log

Dotlink owns exact operator log, not semantic fact type.

Consume AW ProcessStop event/StopRecord and render:

    LOG job stop id=<id> reason=<reason> outcome=<...>

Requirements:
- survives quiet mode;
- fixed order;
- non-panicking fallible logging, because event may come from Drop;
- outside kill -> no connector stop line;
- Drop/cleanup/no-target/unknown cases tested.

### D3 — Job/Task terminal projection

- shared terminal/detail is authoritative;
- remove duplicate private termination fields where possible;
- failed native Task error data projects shared typed facts;
- preserve reaped exit/signal behind later failure;
- preserve prior deadline;
- one dotlink compatibility mapper for sync/jobs_get/Task failure;
- handoff remains a handle, not a second process verdict.

### D4 — refs/diagnostics/receipts

- shared flat Job ref;
- shared handoff/status/control/ACK receipts;
- remove local duplicate ControlReceipt grammar after parity;
- bounded product error backstop remains;
- huge caller ref cannot inflate local HTTP/stdio response;
- replay under narrow budget still same Job/effect.

### D5 — page/result budget

- shared raw-page resolver;
- shared serialized response budget;
- fix jobs_read(limit=1) response-envelope bug;
- raw offsets/EOF/delivery/ACK proof unchanged.

### D6 — aw-mcp receipt decorator/fitter

- replace local generic receipt decorator;
- preserve io.abird/receipt metadata;
- visible ts;
- replay-stable immutable timestamps;
- fresh observation timestamps;
- typed media/structured/raw identity;
- replace local generic result fitter.

### D7 — aw-mcp heartbeat

- replace local generic ProgressHeartbeat;
- preserve progress-token behavior and bounded sends;
- preserve attempted/delivered operator summary through host observer.

### D8 — product-only Nix hardening

Port 30099aa behavior:
- reject Nix environment keys owned by module;
- loop over every reserved key in tests;
- preserve allowed unrelated env;
- update service docs.

This stays dotlink-owned.

### D9 — operational docs + v0.17 equivalence

Adapt runtime-health docs to current jobs_* shared semantics:
- current jobs_get/jobs_read/jobs_ack;
- raw-byte tails;
- shared output owner/ACK;
- 30-minute ReceiptProfile;
- io.abird/receipt;
- current stop evidence.

Full semantic matrix:
every release behavior is Done / Superseded / intentionally product-only.

## 8. Sandbox/mount design — frozen

AW sandbox plan:

    .agents/plans/017-sandbox-mount-convergence-2026-11-18/

dotlink pointer plan:

    .agents/plans/017-sandbox-consumer-deferred-2026-11-18/

Status: designed, frozen, DO NOT IMPLEMENT until the user explicitly promotes/pulls it.

The design records:

AW-owned future semantics:
- generic SandboxMountSpec;
- MountAccess;
- Required vs Optional source;
- target-keyed merge;
- nesting/conflict validation;
- reserved environment validation;
- pinned mount source capabilities;
- final validation at bind time;
- descriptor-backed Bubblewrap RO/RW binds.

Dotlink-owned future consumer:
- config v13;
- profile mounts;
- DOTLINK_MOUNTS;
- repeatable --mount;
- developer cache discovery;
- precedence builtin/discovered < profile < env < CLI;
- Nix integration;
- product diagnostics/docs.

Reference final release mount implementation:
- 21de8b3
- e7db299
- 349a7d0
- cb7418a
- c71f426
- 6f48703
- ba947c6
- ae4d623
- d855672
- 0983d39

Mount core did not drift through v0.17.4.

Do not touch sandbox source/config/Nix mount implementation until explicit promotion.

## 9. Release/0.17 authority and audit rules

Current authoritative release branch:

    887401398da297c458ffa2a152a07e1b85870c35

Tagged v0.17.4:

    fd1cdb6429a341a4e3d27805cae0bec288fc26df

Canonical stop/provenance implementation:

    c185558

Independent hardening/reference commits:
- 74820ff: bound caller-controlled Job IDs in every diagnostic;
- 06d5548: page limit is not serialized response envelope;
- 30099aa: Nix module-owned environment key rejection;
- 5b27784: final runtime-health/backport docs;
- 8874013: signal-name tests use platform signal_label.

Source-of-truth hierarchy when porting release behavior:

1. release/0.17 source + tests at 8874013;
2. v0.17.4 CHANGELOG;
3. c185558 / hardening commit messages;
4. older 0.17.x incremental commits for rationale;
5. old release backport README/TODO only as historical background.

Do not cherry-pick the release branch wholesale.

## 10. Important invariants that must not regress

### 10.1 Physical exit vs lifecycle vs stop evidence

These are independent:

- physical exit: Code / Signal(n) / Unknown;
- Job lifecycle: completed / failed / cancelled / revoked / timed out / uncertain;
- stop reason/evidence: why the runtime attempted a stop and what it actually knows.

Never infer one from another.

### 10.2 No invented signal

A stop request does not imply signal 9, 15, etc.

Only OS/reaped evidence may produce Signal(n).

### 10.3 NoTarget vs Unknown

NoTarget is positive knowledge and disarms.
Unknown is unresolved and may require retry/Drop.

Never merge them.

### 10.4 Terminal stop attribution

Only StopEvidence::Stopped + claimable StopReason may populate ProcessTerminal.stop.

Cleanup/Dropped never become terminal attribution.

### 10.5 Exact Job output ownership

Preserve:
- per-owner delivery proof;
- Through ACK;
- Finish;
- replay-safe already-ACKed owner state;
- no ACK credit from a peek/tail;
- no foreign-owner ACK;
- no new ACK into compacted-away undelivered bytes.

### 10.6 Raw page vs serialized response envelope

Raw jobs_read page size and transport/MCP response budget are different.

A tiny page must still have enough serialized envelope for factual metadata.

### 10.7 Receipt timestamp semantics

AW owns no wall clock.

Host-supplied ts means first finalization of the represented result/observation.

- immutable result exact replay reuses original ts;
- fresh jobs_list/get/read observation gets fresh ts;
- ts is not process start time, mtime, byte-production time or settlement time.

Dotlink uses 30-minute checkpoint guidance.
AW has no global 30-minute runtime policy.

### 10.8 Typed media boundary

MCP wire may preserve Image/Audio/Resource.
Ordinary Agent ToolOutput remains text-only unless a separate provider/media design is authorized.

### 10.9 Replay

AW owns:
- semantic Job replay;
- output owner/delivery proof;
- Remote Start authorization;
- no-rerun/uncertainty inner semantics.

Dotlink still owns:
- OpenAI request IDs;
- serialized hosted response body caching;
- delivery leases;
- hosted horizon/tunnel namespace;
- outer request replay transport policy.

Do not move the outer OpenAI RequestReplayLedger into AW in this effort.

### 10.10 Sandbox frozen

No generic mount/config-v13 implementation until explicit user promotion.

## 11. Validation evidence accumulated so far

### Shared Toolset / Job runtime era

Published AW checkpoints through 7e6fe20 were validated with:
- aw-tools full suites;
- aw-tools-adapter full suites;
- strict all-target/all-feature Clippy;
- minimal/no-default builds;
- downstream MCP/host/CLI;
- dotlink full binary + integration qualification.

### Typed MCP

d6009de / 30dc55a:
- aw-mcp 18/18;
- real dotlink stdio PNG/WAV/PDF typed content;
- strict Clippy and downstream CLI check.

### Pre-0.17 AW convergence

723ec24 / 792f0b3:
- aw-tools 342/342;
- aw-tools-adapter 252/252;
- read-only shared 111/111;
- aw-agent-core 92 passed / 1 ignored;
- aw-mcp 18/18;
- strict Clippy;
- host/CLI/minimal/fmt/diff green.

### Pre-0.17 dotlink convergence

4f60bc3:
- dotlink binary 688 passed / 1 ignored;
- integrations 37/37;
- stdio receipt timestamp wire assertion;
- hosted EmbeddedMcp receipt timestamp assertion;
- strict all-feature Clippy;
- fmt/diff green.

### Physical signal identity

796225e:
- aw-tools full suite;
- aw-tools-adapter 253/253;
- strict Clippy;
- minimal/no-default;
- downstream aw-mcp / aw-host / aw-cli;
- real Unix SIGKILL 9 vs SIGSEGV 11;
- AW-requested cancellation does not fabricate signal.

### Transitional P1 values

021b8ce:
- focused 4/4;
- full aw-tools 346/346;
- strict Clippy;
- no-default build;
- downstream adapter check.

Those tests prove invariants, but the public API names are superseded and must be refactored.

## 12. Current dirty-state ownership

At handoff creation, nothing is staged.

AW pre-existing dirty files that must be preserved:
- .agents/docs/CURRENT_HANDOFF.md
- .agents/plans/ai-native-tool-output-grep-2026-11-15/TODO.md
- .agents/plans/ai-native-toolset-builder-2026-11-15/TODO.md

AW current owned plan edits:
- .agents/plans/017-shared-runtime-convergence-2026-11-18/README.md
- DESIGN.md
- AUDIT.md
- TODO.md

This new handoff and NEXT-AGENT file are additional owned docs.

dotlink current owned plan edits:
- .agents/plans/017-main-shared-consumer-backport-2026-11-18/README.md
- TODO.md

Do not stage/commit any of these unless explicitly authorized.

## 13. Historical handoff references

Use these only for history/details not repeated here:

- .agents/docs/AW-DOTLINK-TOOLS-FULL-COMPREHENSIVE-HANDOFF-2026-11-17.md
  - D1–D4 shared output/grep/replay/security details;
  - B6 tool metadata adoption;
  - writer quiescence / ACK replay context.

- .agents/docs/AW-FULL-COMPREHENSIVE-HANDOFF-2026-11-15.md
  - broader AW history before the latest shared-tool work.

- .agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md
  - finalized unified Jobs/Agent runtime design and pins.

- .agents/docs/WORKSPACE-VIEWS-UX-COMPREHENSIVE-HANDOFF-2026-11-14.md
  - Workspace/view work; separate from this runtime thread.

The live active plans and this document override stale state claims in those older files.

## 14. Exact next implementation action

Do not start P2.

First finish P1 cleanup in AW only.

### Step 1: revalidate

From /home/pvl/spaces/abird/src/aw:

- read AGENTS.md;
- re-read this handoff;
- re-read active master README/DESIGN/AUDIT/TODO;
- git status --short;
- git diff --cached --name-status;
- git rev-parse HEAD;
- git rev-parse origin/main;
- git diff --check;
- confirm sandbox files have no diff;
- confirm unrelated dirty docs are preserved.

Expected:
- HEAD 021b8ce;
- origin/main 792f0b3;
- local main 3 commits ahead;
- no staged files.

### Step 2: refactor execution_stop.rs

Replace the transitional public API with:

- StopOrigin;
- StopReason;
- StopEvidence;
- StopRecord.

Preserve the accepted invariants from 021b8ce.

Required unit tests:

- all four origins survive optional cancellation detail;
- Cancelled with None is valid;
- Stopped + Timeout -> terminal reason;
- Stopped + Cancelled -> terminal reason;
- Stopped + RuntimeError -> terminal reason;
- Stopped + Cleanup -> no terminal reason;
- Stopped + Dropped -> no terminal reason;
- NoTarget + any reason -> no terminal reason;
- Unknown + any reason -> no terminal reason;
- NoTarget and Unknown are distinct;
- values remain Copy / small;
- no-default build.

### Step 3: update active plans

Mark P1 final vocabulary complete only after source/tests match the approved design.

### Step 4: validation

At minimum:

    cargo fmt -p aw-tools
    cargo test --offline --locked -p aw-tools --all-features --lib
    cargo clippy --offline --locked -p aw-tools --all-targets --all-features -- -D warnings
    cargo check --offline --locked -p aw-tools --no-default-features
    cargo check --offline --locked -p aw-tools-adapter --all-features
    git diff --check

### Step 5: checkpoint

Update the master TODO and this handoff with factual results.

Pause.

Do not begin P2 in the same turn unless the P1 cleanup is trivial and the user explicitly asks to continue; the desired behavior is one coherent ~30-minute checkpoint.

## 15. Starting prompt for the next agent

A copy-ready prompt is also stored in:

    .agents/docs/AW-DOTLINK-017-NEXT-AGENT-2026-11-18.md

The next agent should begin from that prompt, not from an old dated handoff.

## 16. Definition of success for the whole active workstream

The active non-sandbox work is complete when:

1. AW owns the final simple stop vocabulary and process-stop evidence model.
2. AW platform/adapter owns cross-platform stop evidence and RAII Guard semantics.
3. ProcessTerminal / JobManager / Task failure detail retain canonical facts.
4. execution lifecycle event facts are shared; dotlink only renders operator log.
5. raw page and serialized response budgets are distinct.
6. Job refs/receipts/diagnostics are shared and bounded.
7. aw-mcp owns receipt metadata decoration, generic result fitting and heartbeat utility.
8. dotlink consumes those shared contracts and removes/reduces duplicate semantics.
9. Nix module-owned environment hardening is ported in dotlink.
10. current release/0.17 behavior at 8874013 is fully accounted for as Done / Superseded / product-only.
11. full AW and dotlink qualifications are green.
12. sandbox/config-v13 remains untouched unless separately promoted.
13. no version/tag/release/deploy occurs without explicit authorization.

That is the current complete context and execution map.
