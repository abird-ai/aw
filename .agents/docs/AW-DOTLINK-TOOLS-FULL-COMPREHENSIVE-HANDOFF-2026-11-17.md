# AW–dotlink shared tools convergence — comprehensive handoff
Author: Cross-repository implementation handoff
Owner: @pvl
Prepared: 2026-11-17, L5 source authority
Scope: AW canonical toolset, filesystem/Graph/Job runtime, typed receipts, finite Job output/ACK/waits, dotlink operational port and subsequent MCP adoption.
Status: **handoff/documentation only; no code, dependency, Git, release or deployment action authorized by this document.**

> **Reading priority:** current live source + actual tests + Git status > AGENTS.md / active plan TODO and design > this cross-repo synthesis > older dated handoffs. This file is a navigation/reasoning aid, not a replacement for normative source or plan checklists. Revalidate the state when resuming; the below SHA and test counts are as observed on 2026-11-17.

## 0. First ten minutes — exact sequence

1. Use **dotlink---l5 exclusively** for source and Git. No A1. Do not create worktrees, edit release branches, or use a second source authoring machine.
2. AW root: /home/pvl/spaces/abird/src/aw. Dotlink root: /home/pvl/spaces/abird/src/dotlink. Stay in the existing main worktrees.
3. In each repository read, in order: AGENTS.md; .agents/plans/README.md; .agents/plans/TODO.md. Then this file, followed by the owning plan README/DESIGN/TODO (see section 17).
4. Read the **current** Git HEAD, upstream tracking, staged and unstaged status, untracked files, active worktrees, and *exact* diffs of the paths you intend to touch. Check for simultaneous edits. Only live code/tests determine what is implemented.
5. Read AW .agents/docs/CURRENT_HANDOFF.md, AW .agents/plans/dotlink-tool-port/TODO.md, AW .agents/plans/ai-native-toolset-builder-2026-11-15/{DESIGN,TODO}.md, and AW .agents/plans/ai-native-tool-output-grep-2026-11-15/{DESIGN,TODO}.md.
6. Read dotlink .agents/docs/CURRENT_HANDOFF.md and .agents/docs/RELEASE-017-OPERATIONS-PORT-2026-11-15.md, then .agents/plans/aw-job-output-grep-convergence-2026-11-15/{DESIGN,TODO}.md and the referenced Rust sources.
7. Observe the ownership reservation: do **not** read, list, write, stage, reset, stash, or otherwise inspect AW .agents/plans/ux/**. Preserve every unrelated dirty/staged item as opaque. The coordinator owns the shared queue and commits.
8. No commits, pushes, tags, releases, version bumps, dependency repins, rebase/reset, deployment, or force changes without **explicit new user authorization**. A request to continue implementation is not authorization to publish.
9. Limit each model/user implementation turn to roughly **30 minutes**. Select one coherent unit, test at its boundaries, write durable plan evidence, and **pause**. No background promises or endless qualification loops.

## 1. Exact snapshot at this handoff

| | AW (canonical shared source) | dotlink (consumer/host) |
| --- | --- | --- |
| Host | pvl-l5 only | pvl-l5 only |
| Worktree | /home/pvl/spaces/abird/src/aw | /home/pvl/spaces/abird/src/dotlink |
| Branch | main | main |
| HEAD | **7d09d5f3c882894cd46b427a89fa75280223115a** | **6723e3e632b2a73f051d7ce4fc589d99cca33c45** |
| Tracking | main matched origin/main at inspection | main ahead of origin/main by **one pre-existing local commit** |
| Last relevant committed shared merge | **0d2b4e3** — builder, metadata/receipts and initial Job hardening | **44b4bee** — selective 0.17 operational port; **6723e3e** is the later local cache-kind revert |
| Currently dirty | AW Job/output plans, Toolset plan, old port TODO, CURRENT_HANDOFF, shared Job/receipt/toolset/FS sources, adapter sources and Cargo.toml | dotlink convergence DESIGN/TODO, src/jobs.rs, src/mcp/tools/jobs.rs, tests/stdio_tools.rs |
| Shared dependency pin used by dotlink | newer changes NOT yet included in a signed post-merge revision | all three AW Git dependencies pinned to **0a6c6a0bea07a3886c4dc8be4f423eec3ea3c134** |
| Released? | No; other AW product/release work independent | No 0.17 release from current main |

At the snapshot no relevant source changes were staged; recheck before acting. AW's **0d2b4e3** merged the first builder/Job changes and **7d09d5f** was the later workspace/resources commit. The subsequent event-driven waits and many shared Journal/ACK/output fixes on the dirty tree **have not** been committed or consumed by dotlink. Never confuse old dated plan headings saying “uncommitted” with actual commit state: read the plan's top **Committed** section and inspect Git.

### Exact dirty-path preservation map at snapshot

**AW modified paths** (do not reset or overwrite): .agents/docs/CURRENT_HANDOFF.md; .agents/plans/ai-native-tool-output-grep-2026-11-15/{DESIGN,TODO}.md; .agents/plans/ai-native-toolset-builder-2026-11-15/TODO.md; .agents/plans/dotlink-tool-port/TODO.md; crates/aw-tools-adapter/src/{binary,job_tools,jobs_tools}.rs; crates/aw-tools/Cargo.toml; crates/aw-tools/src/filesystem/{direct,job_manager,request}.rs; crates/aw-tools/src/{receipt,toolset}.rs.

**dotlink modified paths**: .agents/plans/aw-job-output-grep-convergence-2026-11-15/{DESIGN,TODO}.md; src/jobs.rs; src/mcp/tools/jobs.rs; tests/stdio_tools.rs. The separate release/0.17 worktree is not this main worktree.

## 2. Original problem and why the design converged

**Original AW** was an Agent/TurnMachine/ToolRegistry/ToolPolicy/TurnJournal system, not a shell-only MCP server. It already had exact Actor/source/site/effect grants, immutable turn/intention journaling, Sessions and later Graph/Agent runtime ownership. **Original dotlink** was a Rust single-binary MCP tunnel/local host with native filesystem/shell operations, HTTP/stdio/OpenAI transports, Bubblewrap isolation, local permission policy, response-size budgets and job/Task semantics. Its earlier native filesystem engine had substantial work in grep/patch/mutation, typed results, safe path traversal and replay.

Initially AW ported useful dotlink tool semantics. Two code implementations would then drift in schemas, mutation safety, output, Graph control and replay. The accepted design is to **move reusable tool semantics into one canonical library** (AW `aw-tools`), keep each product's authority/transport in a **thin adapter**, and use the same canonical finite JobManager/Graph. This is not a full product merge.

**Architectural constraints chosen deliberately:**

- **One** AW Agent/TurnMachine/ToolRegistry/ToolPolicy/immutable TurnJournal, not a second tool executor or Session/Agent policy engine.
- **One** shared finite `JobManager` and deterministic Graph/NodePlan. No second workflow runtime, mutable Graph replay engine, persistent workflow DB, or shadow owner registry.
- Shared library contracts independent of MCP, transport, credentials, wall clock, shell sandbox, and product configuration. Favor cheap, immutable values, composable zero/low-cost Rust APIs, fixed bounded buffers, and efficient hot paths **without** opaque abstractions.
- Dotlink remains concrete Process/Graph executor and MCP/Task host; AW alone owns Agent, Session, membership, hierarchy, policy/Envelope, transcript, source epochs and upper-level orchestration.
- Keep dotlink's existing permission/sandbox model and AW's exact Actor authority distinct. A shared parameter or description is **never a grant**.
- User-facing tool grammar must be AI-native: flat names, compact model text, few predictable arguments, deterministic Graph batches, truthful partial/unknown results, bounded paging. Avoid generic JSON dumps or extra interaction round-trips when a safe compact receipt suffices.
- Qualify actual user-visible flows and failure boundaries in **coherent checkpoints**, not redundant requalification of unchanged primitives after every edit.

## 3. Chronology from the beginning (read the referenced records for exact commit-by-commit detail)

| Period/phase | What happened / decision | Source of detailed record |
| --- | --- | --- |
| Sep–Oct 2026, AW foundation | Original Agent/Turn lifecycle, Store, runtime, tools/permissions, source/journal authority, workspace/agent views. Do **not** reinterpret a model-facing tool as its own authority source. | AW .agents/docs/AW-COMPREHENSIVE-HANDOFF-2026-10-19.md; .agents/docs/AW-FULL-COMPREHENSIVE-HANDOFF-2026-11-15.md sections 2–5 |
| Oct 25–29, dotlink-native hardening | Reviewed dotlink native `ls/grep/read/write/edit/patch/bread/bwrite/bedit`, safe path/FD handling, mutation staging, byte budgets, symlinks, file types, replay and shell. | AW .agents/plans/dotlink-tool-port/README.md, PARITY-AUDIT-2026-10-31.md, PORT-RESULTS-2026-10-30.md; dotlink's native-tool plans |
| Oct 26–31, AW native port | Landed dotlink-style native tooling inside AW's existing Agent and platform backends; `find` is not a separate advertised model operation. Commit **7ce39d9** is the native port anchor. Fixed grep ranking/pagination/context/ignore behavior, read offset/binary checks, staged writes/patches/binary edits and Unix/Windows correctness. | AW .agents/plans/dotlink-tool-port/{README,TODO}.md, NEXT-FIVE*, NEXT-TEN*, REVIEW-FIX* |
| Oct 30–31, DL-150 design | Sealed compact text result, 25 canonical operation identities, Graph and durable-job semantics, explicit file vs dir search and bounded work. `c34ea5a`, `81a54ce` and later sealed snapshots are **historical design anchors**, not a reason to replace current source. | AW dotlink-tool-port/reference/DL150-SEALED-*.md, SHARED-TOOLSET-2026-10-30.md; dotlink AI-native filesystem plan |
| Oct 31–Nov 9, extraction **A0–A8** | Renamed `aw-tools-core → aw-tools`, the former AW-specific `aw-tools → aw-tools-adapter`; moved canonical filesystem/Graph/mutation/private output and `JobManager` into shared core; kept AW Process/PTY, policy/journal and projection in adapter. Hardening covers A1 rename, A2 adapter thinning, A3 native path safety, A4 mutation/private lifecycle, A5 shared Jobs, A6 AW binding, A7 consumer readiness, A8 dotlink cutover. | AW .agents/plans/dotlink-tool-port/SHARED-AW-TOOLS-CONVERGENCE-2026-10-31.md; TODO.md; dotlink .agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md |
| Nov 9 convergence | AW final reviewed historical baseline `4a23e92`; dotlink cutover `b31683c` to shared AW tool source. Consolidated shared limits, Graph footprints, Job admission, dynamic narrowing and security reviews. | AW dotlink-tool-port/TODO.md and reference handoff; Git history |
| Nov 10–14 unified Jobs/Agent design | Replaced overlapping batch/Agent/workflow abstractions with one unified Jobs runtime and Graph; canonical IDs `/d:<id>/i:<id>/p:<id>/a:<id>`, host-local aliases and scoped refs; actor/wake/control semantics, policy/envelope, typed execution strategies. After design checkpoint `890e9a3`, AW implementation landed at `0a6c6a0`; dotlink bridge/replay landed at `c90a660`. | AW .agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md; AW unified runtime plans; dotlink .agents/docs/CURRENT_HANDOFF.md |
| Nov 14–15 parallel dotlink **0.16 / 0.17.1** | Independent release lines kept evolving from dotlink's older implementation. **Decision: do not rebase main onto 0.17.x**; selectively port operational improvements (phases 1–3), **leave AW tool design last**. | dotlink .agents/docs/RELEASE-017-OPERATIONS-PORT-2026-11-15.md; Git tags `v0.16.8`, `v0.17.1` |
| Nov 15 selective operational port | Config/schema v12 + migration, NixOS, CLI/env precedence and no-tunnel, response-window/progress behavior, verification and iterative security fixes moved to main without importing release-line `job_*` or its JobManager. Pushed in `44b4bee`; later local `6723e3e` drops abird cache kind. | dotlink RELEASE-017-OPERATIONS-PORT-2026-11-15.md, corresponding sources/tests; Git log |
| Nov 15 AW builder and receipts | Implemented immutable configurable Toolset, per-tool metadata hooks, grep limits, Job profiles and typed compact receipts in shared AW; selected focused single-file `edit` + Graph. Merged `0d2b4e3`, AW HEAD `7d09d5f`. | AW .agents/plans/ai-native-toolset-builder-2026-11-15/{README,DESIGN,TODO}.md; .agents/docs/CURRENT_HANDOFF.md |
| Nov 15–17 AW Job output/security | Shared unknown-dominant Graph status, two-phase delivery proof, scoped consumer leases, current Actor gate and atomic ACK, finite event-driven waits, typed Process status, and sequential correctness reviews for truncation, bad journal states, output/no-op Finish, owner replay and uncertainty. **Later wave remains dirty/uncommitted**. | AW .agents/plans/ai-native-tool-output-grep-2026-11-15/{DESIGN,TODO}.md, chronological checkpoint headings |
| Nov 16–17 dotlink pre-pin corrections | Local settle-only `jobs_get`, byte/EOF `jobs_read`, node-skip correctness, contiguous proof, MCP Task owner retention, bounded Job/control/stdin receipts and immutable remote replay were verified against **old AW pin**. **Not canonical shared adoption.** | dotlink .agents/plans/aw-job-output-grep-convergence-2026-11-15/{DESIGN,TODO}.md |
| Nov 16–17 parallel 0.17 release continued | Separate `release/0.17` now at `acb4044` / `v0.17.3` with later shell-mount/cache-layer fixes beyond the `v0.17.1` port baseline; `release/0.16` at `e761319` / `v0.16.8`. These branches and distinct worktrees are **not** main or AW runtime truth. Do not silently import new mounts or rebase. | dotlink Git log/tags/release branch; operational comparison plan |

The original details are preserved in those historical plans and handoffs. Do not duplicate hundreds of pages of reference snapshots into new live source files; this handoff links the normative artifacts and maps their status.

## 4. Canonical crate and runtime boundaries (current)

**AW shared (single reusable implementation):**

- crates/aw-tools: canonical OperationId, typed effects/statuses, filesystem semantics/parsers/Graph, `JobManager`/NodePlan/output/owner/replay, Toolset factory, receipt renderer, remote Start digest and exact finite Job contracts. Its `filesystem/job_manager.rs` is the primary implementation of retention, ACK, notification and settlement.
- crates/aw-tools-fs-unix and crates/aw-tools-fs-win: descriptor-rooted/native path and mutation backend. Unix openat2/no-follow/no-cross-mount, safe final-symlink classes, staged writes and locking; Windows OS-specific pinning/DACL/replacement behavior. Native platform validation remains an external gate.
- crates/aw-tools-adapter: AW `configure_with_toolset`, canonical ToolRegistry binding, trusted Process/PTY custody, JobHost/JobRuntime, Graph request projection, policy-aware receipt, current Actor/source/site/effect revalidation, protected output disclosure.
- crates/aw-agent-core, aw-agent-kernel and higher AW crates: Agent/Turn/Session, ToolPolicy/Envelope, current Controller, authority, journaling, scheduling/SDK, view hierarchy and host services. No duplicate owner in `aw-tools`.

**dotlink (consumer):**

- Cargo.toml: all three `aw-tools`, `aw-tools-fs-unix` and `aw-tools-fs-win` Git dependencies currently pin **exactly 0a6c6a0...**, not HEAD AW. No path overrides during migration.
- src/jobs.rs: single dotlink Process/Graph lifetime owner, MCP native Task/replay and current host projection; the current *temporary* contiguous delivery high-water fence lives here.
- src/mcp/tools/jobs.rs: `batch`, `jobs_list/get/read/ack/send/cancel` models, checked schemas, bounded result/admission and cached remote Start projection. `src/mcp/tools/shared_native.rs` covers shared native filesystem, including grep.
- src/mcp/tools/shell.rs, graph.rs, src/mcp.rs, transport/OpenAI modules: host shell execution, Graph admission, tool transport, authenticated request budgets and exact remote Start replay.
- Dotlink security, profile, sandbox, OAuth and transport policies remain host owned. **AW's `AgentStepGate` is not dotlink's auth source.** Dotlink must use its own current authenticated machine/source/Task/transport authority when calling shared APIs.

**User-facing operations** are the sealed flat canonical filesystem primitives (ls, grep, read, write, edit, patch, cp, mv, rm, bread, bwrite, bedit), Process/Graph (bash or platform shell; batch) and finite Jobs (jobs_list/get/read/ack/send/cancel). Do not resurrect old `job_*`, `job_restart`, `job_start`, nested batches, a standalone model `find`, or AW-only Agent/history/filter/subscription surfaces inside dotlink without new design authorization.

## 5. Toolset factory: the order of decisions was intentional

The user explicitly deferred AW/dotlink-specific public variants until the canonical **factory/builder API** was elegant and reusable. This is not a generic dynamic/plugin system; it is immutable validated composition.

**Shared `Toolset::builder()` / `ToolsetBuilder::build()`** supports:
- `filesystem(SchemaCaps)`: host-selected (narrowing, never widening) filesystem schema/work ceilings.
- `grep(GrepProfile)`: context default/max, cumulative work bounds; default context is symmetrical and each explicit `context`, `before`, `after` validates independently, even if overridden. Shared parser/executor/direct/Graph alignment; 0..128 per side is the current shared bound.
- `jobs(JobProfile)`: aggregate get-tail default/max, finite get/read wait maxima, Job-list capacity hints. Default get tail **512 total bytes**, max **1024 total bytes**; sharing stdout/stderr is 256/256 by default, with unused share borrowed by the other stream. These are **shared approved/default profile semantics**; dotlink's pre-pin public API is not yet migrated.
- `receipts(ReceiptProfile { unix_ts, checkpoint_minutes })`: opt-in factual timestamp and optional host guidance. The host supplies time; the core never owns a clock or ends a model turn.
- `only([...])`: narrower advertised filesystem tool subset in AW direct and Graph. It cannot grant capabilities. Process/Job permission remains independent.
- `tool_text(OperationId, ToolText)`: per-tool metadata-only title, full description replacement, description suffix, and existing argument-description overrides; includes nested JSON Pointer paths for actual property nodes. Multiple calls merge deterministically. Validate paths against canonical/host schema and reject unknown fields, invalid control characters, unknown templates and oversized text **before** registering tools.
- `filesystem_descriptor`, `describe_operation`, `operation_title`, `operation_description`, `model_instructions` and structured `render_receipt`: one canonical vocabulary/limits. AW host consumes metadata via `configure_with_toolset` and a tiny `DescribedTool` proxy that delegates exact validation, execution and parallelism. Dotlink's MCP projection remains to be integrated after D0.

**Why not fold limits into a one-off tool:** AW and dotlink have different safe host defaults (FS grants, response budgets, process, timestamps, authority). A metadata/limit factory lets each product configure the shared engine without forked parsers, inaccurate advertised caps or a second dispatch registry.

**Focused edit — Option A explicitly selected:** canonical `edit` operates on one file and exact replacement tuples. For multiple files use distinct `["edit", {path, edits}]` nodes in existing `batch` Graph; Graph owns dependency edges, authority, effects, partial/unknown, replay and receipt. Do **not** bring the 0.17.x multi-file edit grammar or duplicate mutation engine back as a model-facing tool. A later change would require measured token/UX advantages and a reviewed cross-file transaction contract.

Other deferred product choices: dotlink-specific shell prose/turn-horizon defaults, recursive `ls` vs explicit `find` (no extra model find for now), typed MCP media, sandboxed AW shell, per-product `ts`, and optional new receipt fields. Avoid speculative schema changes.

## 6. Compact receipts, DL-150 and model/SDK separation

A concise receipt is a **pure projection of trustworthy typed facts**, not the source of truth or a permission token. Typed SDK/host results and persisted Job journal remain authoritative; never parse a model-visible string to reconstruct authority or execution state.

- `aw_tools::receipt::Receipt` is a closed enum with `JobStatus`, `NodeStatus`, and typed `Ack` facts; `NextAction::ObserveJob` is limited continuation advice. `Toolset::render_receipt(receipt, host_unix_ts, total_budget)` cannot decorate arbitrary file output.
- Job headers include exact ref, label, observed lifecycle/outcome, sequence/revision and optional Graph progress. Settled Process fields can include typed exit mode/code, raw stdout/stderr **total bytes**, truncated/incomplete flags; never infer them from `Snapshot.detail`.
- `journal_unknown` comes directly from `Job::journal_ambiguous()`. Show `unknown` and `journal=unknown` without claiming the Process or Graph is terminal. Node receipts keep independently known node facts and mark parent journal uncertainty.
- Optional `ts=<epoch_seconds>` is supplied by host, counted in the whole bounded receipt, never minted by core clock. The original desired 30–35 minute model-turn guidance belongs to dotlink's configured instructions/metadata, **not** the execution timeout or an automatic scheduler. Current connector may expose its own `ts`; that does not mean new AW/dotlink model schema adopted this default.
- Model Job handoff should keep exact `j:<id>` plus a short `next=jobs_get|jobs_read no-rerun`; routine get/list need not repeat it. A truncated success that loses a Job handle is forbidden. If a full status exceeds the authenticated MCP budget, return a complete compact handle or a truthful error that the effect may already have occurred; **never rerun**.
- Preserve literal source-byte offsets when paging output; JSON/UTF-8-lossy/base64 expansion changes serialized size, **not** how many raw bytes are delivered. Follow-up `next_offset` is absolute and must match the returned raw prefix.
- Receipt budgets cover **all** text, markers, headers and optional metadata; no hidden unbounded tail or post-success truncation. Avoid bulk JSON wrappers for ordinary model decisions, while SDK clients can request structured original facts.

Examples (illustrative grammar, not fabricated live Job observations):

    j:7 bash running rev=3
    j:7 bash settled rev=4 outcome=fail exit=1 out=20001 err=732 out_truncated
    j:7/n:2 bash waiting rev=4 journal=unknown
    j:7 unchanged ack=all released=true
    j:7 next=jobs_get no-rerun

## 7. Finite Job, Graph and uncertainty semantics: agreed contract

### 7.1 `jobs_get` — decision/status only

- Default combined stdout/stderr tail **512 raw bytes total**, explicit 0..1024 aggregate cap. Allocate roughly half to each stream and reassign unused balance. Small enough for quick model decisions; full output belongs to `jobs_read`.
- `tail=0` means status only. A default tail requires independent current `jobs_read` authority; with no such grant an implicit tail becomes `tail=denied` (status remains available), explicit nonzero tail is rejected. Never leak output merely because `jobs_get` was granted. Tail peeks never advance ACK proof.
- `wait_ms=0` returns current status. Positive wait is **settlement-only for the exact root or node**, or fixed wait deadline, not a new-byte wake and not a revision-change wake. Subscribe before checking authoritative state to avoid lost wakeups. An unknown **journal** is a separate observation boundary: report `journal=unknown` promptly, without calling execution settled.
- The wait controls only observation; **never modify Process execution timeout**. Respect transport's authenticated remaining response window in dotlink, not arbitrary client `_meta`.

### 7.2 `jobs_read` — bytes, peeks and output completeness

- `wait_ms=0` supports immediate nonblocking read/peek. Positive wait at a **nonnegative absolute raw offset** wakes for new bytes, truncation, exact writer EOF, target-node settlement or deadline. It must not wait on a running Graph sibling after an exact skipped/failed node settles.
- `offset=-N` is a pinned newest-N-byte tail snapshot **without waiting**; positive wait with negative offset is invalid. Do not call it a signed pointer into the future.
- Streams can attach after wait subscription; use the **same bounded per-Job watch** for new attachment, retained byte append, first truncation, writer failure, EOF and state transitions. No 10 ms spin polling and no second scheduler.
- EOF, empty output and missing output are distinct. A settled skipped node with no stream reports permanently unavailable output, not a fabricated page or `still waiting`. A journal-fenced Job whose requested bytes are unavailable returns a reconciliation conflict, not fake EOF.
- Output/read and owner delivery proof are distinct; streaming, reading, obtaining a status and joining a Task do **not** implicitly ACK anything.

### 7.3 `jobs_ack` — explicit Control, not read side effect

- `jobs_ack({ref})` (offset omitted) means intentional **Finish** of that authorized output audience's *known settled* Job/node scope, including unread bytes. Empty settled Skip/output-free scope returns idempotent `unchanged ack=all released=true`; an explicit nonexistent view still fails. Unknown/journal-ambiguous/in-flight effects reject before mutation.
- `jobs_ack({ref,offset})` means exact owner-specific **Through** acknowledgment of a contiguous delivered prefix. A model may explicitly request through a known offset without requiring synthetic read of every byte **if** the correct owner has proof. Out-of-order pages **do not bridge gaps**. Even offset-zero ACK needs a successful first-page projection when required by host proof.
- Shared `JobManager::record_delivered_output(id,coord,stream,owner,raw_start,raw_count)` is called only by a trusted host **after** a complete bounded response is successfully projected and current payload authority is rechecked. A fetched-but-not-returned buffer does not count. Stream tail is not prefix proof.
- Owner identity remains meaningful **after physical spool release**: public and trusted ACK replays must check exact owner and prior watermark, never accept arbitrary stranger/offset because `released=true`.
- Root Finish inspects every node + journal ambiguity; an unknown node refuses whole-Graph Finish, but a known terminal node may be finished independently when globally safe. `unknown` dominates any aggregate that has an unresolved effect; `mixed` means only heterogeneous settled known outcomes. Preserve known success/failure counts and orthogonal `more`/`incomplete`.

### 7.4 Owner/Task/replay model

- **AW normal finite Jobs** have one original Agent/Job audience across turns, with current Actor and producer payload policy independently revalidated. Do not create gratuitous per-turn leases or pretend AW has distinct independent clients today.
- Hosts supporting genuinely separate consumers can derive a domain-separated `consumer_output_owner(job, host_authenticated_32_byte_scope)` and explicitly, idempotently `enroll_output_consumer` in selected existing streams, after **current host authorization**. Enrollment preflights all streams atomically, caps owners at 16 per stream, never resurrects released output and cannot create a grant. A stream attached later requires new enrollment. Original Job owner and Task result owner are separate identities.
- Dotlink MCP native Task/result, hidden request/replay and ordinary model output can require **independent retention**. One model ACK must not discard a Task's exact retained result prefix; compact only when all relevant owners are released. Do not add another shadow high-water or duplicate JobManager during adoption.
- AW actor-controlled finite `jobs_ack` uses exact `ToolBindingAuthority::with_revalidated_binding` and live `AgentSessionController::with_authorized_tool_commit` around one short synchronous effect: lock order **ToolBindingAuthority → Actor runtime → JobManager → output**. A plain preflight check cannot substitute for atomic Control grant/revocation ordering. A host gate that fails *after* an effect yields outcome-uncertain/inspect, not a false guaranteed-no-effect error.
- Producer source/writer epoch, remote Task authorization, transport physical receipt confirmation and non-ACK Control verbs are **separate** gates; do not claim the current ACK control lock covers all external mechanisms.

### 7.5 Graph and journal failure/recovery

- Graph node IDs/refs are stable; root results are synthesized from typed node outcomes, not a retained output stream owned by node 0. Root `jobs_read` does not pretend child stdout is a root spool. Deterministic frozen NodePlan and dependency/footprint checks must occur before effects.
- `Status` separates outcome family `ok/fail/unknown/mixed/unchanged/skip` from orthogonal `more` and `incomplete`. Unknown-dominant aggregation is **intentional**: [ok,unknown]→unknown, [fail,unknown]→unknown, [ok,fail]→mixed. User explicitly rejected mixed until all effects are settled.
- A transition with external persistence outcome unknown fences the journal; a mismatched in-flight proposal also fences it and wakes the watch. Do **not** claim settlement, release execution permits, replay the operation, erase its output or let terminal expiry/GC drop a journal-unknown Job.
- Pending AW (uncommitted) now marks journal uncertainty in Snapshot/status/receipt; finite get returns a truthful unknown observation, and an unreadable read wait returns a reconciliation conflict. Completed Process bytes already available may be read with current authority; no fabricated EOF.
- **Destructive output mutation** (Through ACK, trusted ACK, physical compaction) must hold the same shared manager lock as journal state, then output lock. While persistence transition in-flight → `Busy`; journal ambiguous → `ReplayUnknown`. Root Finish and known-node restrictions remain. Terminal-looking journal-unknown Jobs cannot be explicit expired or capacity-pruned; reclaim other known terminal Jobs instead or return Capacity.
- Output quota/failure watches: signal late attachment/new retained bytes, **first truncation** (whether append or direct discard), first writer failure and EOF; avoid waking on every later discarded chunk while still counting source bytes. Change notifications are bounded invalidations of authoritative state, not events to replay/ACK.

## 8. Dotlink operational port from parallel 0.16/0.17 — specific decisions

The user reviewed **every commit** on the parallel 0.16 and 0.17 lineage and the 0.17.1 state, then chose **selective port into main**, *not* “rebase main atop 0.17.x”. The AW-tool-contract questions were deliberately left until after the operational port.

**Phase 1** — configuration and NixOS: historical config migrations v9→v10→v11→v12; strict versioned decode, explicit path/env precedence and profile resolution once; fail closed malformed selector; OAuth config strict nested fields, private OAuth/credential generations; NixOS typed single `settings`, private staged config, declarative profile identity and collision assertions. Preserve main's newer private state, replay and ownership work.

**Phase 2** — CLI/transport/flow: `--no-tunnel` stops saved OpenAI tunnel before credential/env resolution; `--no-serialize` and explicit CLI choices override env; selective `DOTLINK_PROFILE`, verbosity/color, progress/REQ logs. Authenticated response window limits synchronous shell hold and observation waits, without altering original shell execution timeout or Job identity. No imported `job_*` aliases.

**Phase 3** — grouped qualification, reviews: current config/transport, integrated tests, NixOS module checks. Review fixed implicit profile selection, disabled tunnel env parsing, unknown OAuth fields, `--list-tools` environment projection, ingress port collisions, verbosity precedence and fail-closed NixOS ExecStartPre staging. Operational main port committed as `44b4bee`, with latest local `6723e3e` cache-kind revert; no version/release/tag.

**Parallel release branches are reference material:** `v0.16.8` = historical branch state and `v0.17.1` = original selective port baseline `07ca25f...`; `v0.17.3` = later `release/0.17` tip `acb4044` with additional mount/cache-layer design. Do not rebase main, migrate all unreviewed branch changes, change a release worktree, or claim the operational port includes later 0.17.2/0.17.3 mount changes. Future branch drift must get its own read-only semantic/authority diff review and approval.

**Deferred AW-first tool decisions from 0.17.x:** tail-oriented read vs absolute offsets, optional ACK/Finish, status/receipt codes, grep context, description fidelity, 35-minute timestamp guidance, output/task/replay semantics, multi-file edit shape, tool schema. Current shared decisions are in sections 5–7; check **current source** before saying dotlink has adopted them.

## 9. Dotlink pre-pin MCP/Task correctness wave (uncommitted, 2026-11-16/17)

The following was **implemented/tested in dotlink's existing main worktree with AW pin 0a6c6a0**, not by updating shared dependencies:

1. `standalone_get`: exact Process root/Graph node settle-or-deadline; stale `rev`, early stdout or another node cannot end the wait; existing `Notify` subscribed before state check, with transport wait budget.
2. `output_node_page_wait`: exact byte/EOF/short deadline, skipped/failed node without stream ends when its own node settles rather than waiting on sibling; permanently absent stream has truthful diagnostic. Negative offset semantics remain future shared API work.
3. MCP `record_standalone_delivery`: old high-water `max(next_offset)` was unsafe—out-of-order page could ACK unseen prefix. Temporary fence now only advances on a **contiguous returned page** in the exact Job/node/stream, and no zero-prefix proof before first-page projection.
4. Native MCP Task retained result owner remains distinct from model Job output owner; model `jobs_read → jobs_ack` cannot release Task's exact result prefix. Duplicate ACK and Task replay remain safe; bounded text/base64 expansion accounts only for returned raw source bytes.
5. `jobs_get` status is complete within authenticated/profiler result cap or errors; no truncated successful status.
6. `batch`/Graph and remote Start handoff use complete original status or minimal `j:<id> next=jobs_get no-rerun` after admission; if no handle fits, explicitly report already-admitted and discovery through `jobs_list`. Immutable cached remote Start receipt is reprojected for a narrower envelope, never rewritten or rerun.
7. `jobs_ack`/`jobs_cancel` return complete original Control status or compact action-specific status, with identical exact Job identity under replay; a tiny response reports already-applied outcome, not safe re-execution. Other Control verbs and transport physical delivery remain gated.
8. `jobs_send` preflights a minimal `j:<id> sent` receipt **before** irreversible stdin delivery; after send, preserve original revisioned success if it fits, otherwise compact. Remote Start duplicate validates exact original Job and receipt grammar and does not resend. Source/transport authority and physical delivery remain separate.
9. Existing stdio MCP and binary unit suites were rerun at checkpoints. Most recent recorded dotlink source-gate from the 2026-11-16 send phase: **676 binary passed, 1 ignored, 5 stdio MCP passed**, strict Clippy, source check, formatting and diff green. Some large test builds were externally terminated before execution; always distinguish that from failures. **Do not claim these are fresh after the current handoff or cover the unpinned AW changes.**

**Remaining pre-pin decisions:** do not extend the temporary high-water table into another durable owner engine. Once a reviewed AW revision exists, replace the temporary dotlink delivery proof with canonical shared owner-range APIs, preserving current MCP Task and hidden replay leases. Do not assume a transport wrote/delivered all bytes merely because model result projection succeeded.

## 10. AW post-merge Job/output checkpoint ledger — **source currently uncommitted**

The AW Job/output plan's checkpoint headings are chronological provenance, **not extra execution engines**. Every item below has detailed tested evidence and failure conditions in AW .agents/plans/ai-native-tool-output-grep-2026-11-15/TODO.md. Current live source includes them in one dirty tree; only commit `0d2b4e3` is merged before that wave.

| Date / checkpoint family | Detailed change and invariant to preserve |
| --- | --- |
| Nov 15 — shared Jobs get/read/ACK + grep plan | Designed and then integrated typed status, default aggregate tail 512/max 1024, read-offset/byte windows, direct+Graph grep 0..128 context, and per-owner output representation. |
| Nov 15 — unknown-dominant Graph | Graph aggregate `unknown` if any node unknown; `mixed` is heterogeneous **known** results. Keep exact known counts and reject root Finish while any effect unresolved. |
| Nov 15 — two-phase delivery proof | Side-effect-free `output_page_for_owner_at` and tails; trusted `record_delivered_output` after final bounded projection. Only contiguous raw returned bytes may be ACKed. |
| Nov 15 — scoped host consumer leases | Domain-separated Job+host-consumer owner, explicit atomic enroll across selected streams, capacity 16, idempotent, no resurrection. AW still has one Agent/Job audience. |
| Nov 15 — live Actor Envelope | `ToolContext` carries live gate for a controlled Agent; status tail requires independent current `jobs_read`; finite read and ACK recheck their actual policy. No permission inferred from a Job ref. |
| Nov 15 — atomic finite ACK Control commit | Exact ToolBindingAuthority + AgentSessionController guard holds through synchronous JobManager ACK/Finish effect, correct lock order; post-effect uncertainty is not reported as definite no-effect. |
| Nov 16 — event-driven waits | One bounded Tokio watch per Job, subscribe before state checks, no revision polling; root/node settle-only get; byte/EOF/first-truncation read, deadline and independent observer cancellation. |
| Nov 16 — node-local read | Missing or unsealed stream of a settled skipped/failed node stops its own wait; do not wait on an independent slow Graph sibling, fabricate output or give ACK proof. |
| Nov 16 — race qualification | Late-attached dependent stream wakes real reader. Withdrawing current read capability mid-wait denies the response before projection and creates no credit. |
| Nov 16 — quota-truncation notifications | Direct `discard_if_full` publishes first truncation even when no retained bytes were appended; not every later discarded chunk. |
| Nov 17 — zero-stream Finish | Settled known Skip/output-free Job/node has idempotent no-op Finish without an explicit stream; requested nonexistent view remains error; nonterminal/unknown cannot use shortcut. |
| Nov 17 — owner validation after physical release | A released spool is not universal ACK permission: original owner and prior watermark required; stranger/oversized ACK rejected. Same-owner idempotence after compaction retained. |
| Nov 17 — direct append notification | Wake for new retained bytes or **first** truncation; repeated discarded source data only updates exact totals, without watcher stampede. |
| Nov 17 — failed writer drain | After spool failure, count every drained source byte; wake on first failure/truncation and final EOF, not every discarded chunk. Failed append itself wakes. |
| Nov 17 — mismatched journal transition | If returning persistence callback sees an in-flight proposal mismatch, fence unknown **and notify** bounded observers. That notification is not settlement or a new replay grant. |
| Nov 17 — unknown-journal status/get/read | Explicit `Snapshot.journal_unknown`, truthful `unknown ... journal=unknown` receipt; finite get observes fence promptly, unreadable read wait returns reconciliation conflict, underlying lifecycle stays nonterminal if it was running. |
| Nov 17 — journal-fenced destructive output mutations | Through ACK, trusted ACK and physical compaction share manager→output guard: unresolved persistence = Busy, ambiguous journal = ReplayUnknown; retain all physical bytes and owner state for reconciliation. |
| Nov 17 — terminal-journal retention | Explicit expiry and capacity auto-prune cannot drop a terminal-looking but journal-unknown Job; reclaim only known-safe records or reject admission with Capacity. No false recovery. |

**Most recent recorded AW regression totals (from the live plan, not freshly executed in this documentation turn):** `aw-tools --all-features --lib` **338 passed**; `aw-tools-adapter --all-features --lib` **250 passed**; `aw-tools --features read-only --lib` **110 passed**; minimal/non-async build, strict Clippy, MCP, minimal CLI and default/full CLI reported green. Some earlier grouped runs were terminated externally before individual reruns; the plan records their provenance. Do not call this a committed Git revision or complete D0.

The **latest pending follow-up** in the terminal-journal checkpoint is review of restart-source eligibility and host nonterminal-work summaries. No automatic restarts/reconciliation were authorized in these changes. An actual unknown journal remains unknown until an owner performs explicit authority-safe reconciliation.

## 11. Exact current implementation status versus pending adoption

| Surface | Merged AW `0d2b4e3` | AW working tree after `7d09d5f` | dotlink main after `6723e3e` |
| --- | --- | --- | --- |
| Canonical FS/Graph/JobManager | Yes (prior unified AW baseline + merge) | Same core, further hardening | Consumes pinned older AW `0a6c6a0` |
| Immutable Toolset builder, grep/Job profile, metadata hooks | Yes | Minor review/config diagnostics | **Not** yet MCP factory/descriptor adoption |
| Typed receipts and Process terminal facts | Yes | Journal-unknown status refinements | Own bounded MCP receipts; no shared new timestamp/description config |
| Shared contiguous delivery proof and host consumer API | Initial implementation merged | Additional release/replay/journal fences | Existing **temporary** host high-water delivery, does not use latest shared API |
| Live AW Agent Envelope / atomic finite ACK | Initial merge | Preserved by additional Job correctness | Distinct dotlink auth; does **not** import AW Agent gate |
| Event-driven finite get/read + watch | Not included in pinned commit | Implemented/qualified in dirty tree | Independent existing-host `Notify`, does **not** use new AW watch |
| Default combined get tail, signed read offset, omission-based Finish | Approved/shared design | AW Job profiles/tools in current working tree | D1 pending; dotlink's older model-facing controls remain |
| File/dir grep context parity up to 128 | Canonical AW engine | Same | D2 pending for exact MCP schema/engine+Graph parity |
| MCP Task and transport replay leases | AW generic owner primitives only | More owner/journal protections | dotlink native Task, tunnel replay and auth still own these gates; D3 partial |
| Native Windows/macOS, media, AW sandbox, real release | Separate external gates | Not claimed | Not claimed by current main |

**Do not conflate:** “AW shared Job runtime merged” (true), “all new wait/owner/unknown refinements merged” (false), “dotlink pins the new AW source” (false), “dotlink's current standalone MCP fixes pass on the old AW pin” (qualified locally), or “whole production release/transport/platform gates are complete” (false).

## 12. Work still open — exact dependency order and stop conditions

### D0 — shared AW commit-readiness / new reviewed Git pin (blocking)

1. Revalidate AW actual current dirty state and the owning plan; do a focused read-only review of *all* remaining shared Job/receipt/metadata/runtime source diff, not a superficial latest-patch check. Identify unfinished safety gaps, notably terminal journal ambiguity/restart-source eligibility, state/owner retention, policy/revocation and async feature matrix.
2. Preserve unrelated work; do not touch reserved AW UX plan subtree. Check feature-gated `no-default-features`/`read-only`, full shared + adapter tests, strict targeted Clippy, MCP/CLI compile. Requalify grouped user-visible flows/failure boundaries, not every unchanged primitive in a loop. Record precise results, failures and exceptions.
3. Coordinator/user then authorizes one **signed AW commit** containing only the reviewed owned changes; verify signature, exact SHA and remote availability **after** explicit push approval. Do not call a dirty tree, local SHA without remote availability, or unsigned build evidence a consumable pin.
4. If test/source review finds unresolved correctness, fix, test and checkpoint; do not force D0 closed. Coherent 30-minute slice, then pause.

### D1 — dotlink canonical Job/output/ACK adoption (requires D0)

1. Revalidate dotlink's current main/outstanding port, the exact AW Git SHA, Cargo.lock and all three shared dependency pin lines. Obtain approval for the dependency update before editing; update all three to **the same** reviewed SHA. No path patches or mixed sources.
2. Replace the dotlink-local delivery high-water fence with canonical `record_delivered_output` and owner state: exact stream/coordinate/consumer, only complete fitted MCP response bytes, contiguous prefix, no automatic ACK on read/get/tail.
3. Wire approved get-tail defaults (512 combined, max 1024), bounded actual two-stream redistribution, fast status-only, independent `jobs_read` permission, node/Graph-root output distinction. Wire read absolute and negative snapshot offsets and positive byte/EOF wait through the shared watch, preserving transport remaining-window clamp.
4. Wire optional offset `Through` versus omitted-offset explicit terminal `Finish` to **the caller's** authorized stream(s), with correct Task/producer/replay owner isolation. Scope any independent consumer enrollment to a stable authenticated host identity; never model-selected or per-turn UUIDs.
5. Test real MCP stdio + HTTP/remote bridge: root/node Process output, multi-stream totals/truncation, offset windows, skipped/missing streams, low budgets, combined tails, read revocation, output/ACK after release, ambiguous journals and Restart/replay source. Avoid second JobManager and stop on any cross-owner release.

### D2 — grep direct/Graph parity (requires shared review/pin)

- Use the **canonical shared grep parser/schema/engine** for exact file and directory tree. Check `context`/`before`/`after` independently at 0, defaults, 8, 128 and invalid 129; no silent clamp, including when one explicit field overrides another.
- Verify hidden/.gitignore filtering, candidate top-N, safe native path resolution, UTF-8/control/binary classification, work caps, glob and incomplete notices. Direct/Graph must behave identically under configured limits and same host security ceiling.
- Project the factory metadata/argument description hooks (ToolText) to actual MCP ToolSpecs without changing canonical operation names/effects/grants. Host may customize titles/descriptions, not schema permissions or execution.

### D3 — MCP Task, remote Start and transport budget correctness (partly implemented on current pin)

- Dotlink owns its authenticated OAuth/HTTP/OpenAI tunnel request replay and native Task result leases. The shared owner API is a **library primitive**, not a substitute for host authentication or physical transport delivery guarantees.
- Preserve exactly-once remote Start claim-before-effect and immutable original replay receipt. Existing pre-pin dotlink tests cover narrower budget projection of Job/ACK/Cancel/Send, but that proof must be repeated against the new AW pin. Do not change canonical original receipt or resend stdin because a transport result was lost.
- Verify encoded success envelopes, profile and authenticated remaining-window budgets, cursor/offset, tail, ACK, cancellation, response loss, failed delivery, expired/old-generation proofs, source/generation mismatch and Task/replay retention. Keep request timeout/response hold separate from Process execution timeout.
- Confirm actual physical output cannot be reclaimed while a distinct Task or delivery lease is active. Outcome uncertain is inspect/reconcile, never re-execute by assumption.

### D4 — qualification, review, documentation and handoff

- Group focused Rust core/adapter, real stdio and relevant HTTP/remote/Task integration, feature-gated builds, strict Clippy and formatter/diff checks. Mark unexecuted native Windows/macOS/full flake/deployment gates **open** rather than inferred green.
- Independent security review for changed ACK/replay/producer-source leases; preserve AW source/grant lock order and dotlink tunnel request idempotency under revocation.
- Reconcile the owner plan checkboxes only when implementation/test evidence is committed, current and independent. Update actual docs and current handoff. Get separate user approval for signed commits, pushes, version/tag/release/deploy; no Git history rewrite.
- Stop when the next coherent checkpoint is reached; report exact changed paths, tests that ran, tests blocked and next step.

### Other genuine open product decisions (not D0 blockers unless changed)

- Model-facing timestamp `ts` and per-product 30/35-minute checkpoint instructions; factory supports optional host-supplied values, but defaults/adoption remain product choices.
- Typed native MCP media versus AW encoded text/JSON, and separately-authorized sandboxed AW shell; do not infer either from current FS convergence.
- Original AW Agent and workspace UI/platform release readiness, native Windows/macOS/firmware tests, external credentials/provisioning. They belong to different plans and should not be bundled into this migration.
- Later `release/0.17.3` mount/cache-layer changes still need an explicit selective-port review if the user requests them, separate from shared toolset and no-rebase decision.
- Keep product-specific source/exact private data off logs, Git artifacts and handoffs. Never paste actual secrets, live credentials or replay transcripts into docs.

## 13. Validation commands and truth rules

Run from the **current source checkout**, with the correct working tree and no speculative dependency update.

AW shared / adapter (choose grouped coherent checks; do not repeatedly rebuild unchanged code):

    cargo fmt -p aw-tools -p aw-tools-adapter --check
    cargo check --offline --locked -p aw-tools --no-default-features
    cargo test --offline --locked -p aw-tools --all-features --lib -- --test-threads=4
    AW_BASH="$(command -v bash)" cargo test --offline --locked -p aw-tools-adapter --all-features --lib -- --test-threads=4
    cargo test --offline --locked -p aw-tools --features read-only --lib
    cargo clippy --offline --locked -p aw-tools -p aw-tools-adapter --all-features --all-targets -- -D warnings
    cargo check --offline --locked -p aw-mcp
    cargo check --offline --locked -p aw-cli --no-default-features --features minimal --lib
    cargo check --offline --locked -p aw-cli --lib

Dotlink before/after a reviewed pin (avoid running wide suites if compiler environment is already unstable; use narrow reruns and state limitation):

    cargo fmt --all -- --check
    cargo check --offline --locked
    AW_BASH="$(command -v bash)" cargo test --offline --locked --bin dotlink -- --test-threads=4
    cargo test --offline --locked --test stdio_tools -- --test-threads=2
    cargo clippy --offline --locked --all-targets -- -D warnings

- L5 sometimes externally terminates large Rust test compilation before test execution; a terminated build does **not** establish test failure or pass. Try a bounded `CARGO_BUILD_JOBS=1`/`CARGO_INCREMENTAL=0` rerun when useful, do not claim a nonexecuted suite. Avoid excessive repeated retries; checkpoint uncertainty honestly.
- State the **exact** number of tests executed and outcome, not “full suite passed” when only source `cargo check` ran. Preserve native target gates explicitly as blocked if not installed.
- Snapshot counts are provenance only; do not promise they remain the same after code edits. Run `git diff --check` on owned paths, inspect staged files only if authorized, never silently stage other agents' changes.

## 14. Source and plan map — exact artifacts to read, not duplicate

### AW canonical sources

- `crates/aw-tools/src/{lib.rs,operation.rs,toolset.rs,receipt.rs,conformance.rs}` — operation IDs, factory, descriptions, profiles, status receipt and shared test vectors.
- `crates/aw-tools/src/filesystem/{schema.rs,request.rs,direct.rs,discovery.rs,walk.rs,batch.rs,job.rs,job_manager.rs,runner.rs,status.rs}` — shared files/Graph, state machine, owner output/spool, finite output/wait and Process terminal facts. Check exact filenames with `rg --files` before opening.
- `crates/aw-tools/src/{task.rs,task_spool.rs,task_fence.rs,remote_start.rs}` — bounded spool, Task/error/remote authorization helpers.
- `crates/aw-tools-adapter/src/{lib.rs,jobs_tools.rs,job_tools.rs,job_host.rs,job_runtime.rs}` — AW tool registration, current Actor gate, finite Process/Graph, output projections, delegated control.
- `crates/aw-agent-core/src/{agent.rs,tool.rs,control.rs}` — original Agent, ToolContext/ToolRegistry/Policy and live step gate, atomic Control commit.
- `crates/aw-tools-fs-unix`, `crates/aw-tools-fs-win` and AW platform crates — safe IO and portable mutation; preserve no_std/alloc and platform boundaries.

### AW planning/historical provenance

- `.agents/plans/README.md` and `.agents/plans/TODO.md` — coordinator queue, owner and checklist rules.
- `.agents/plans/dotlink-tool-port/{README.md,TODO.md,SHARED-AW-TOOLS-CONVERGENCE-2026-10-31.md}` — first native parity and extraction chronology A0–A8; historical baseline refs.
- `.agents/plans/dotlink-tool-port/reference/DL150-SEALED-*.md` — sealed earlier model-side spec snapshots; do not override newer accepted decisions.
- `.agents/plans/ai-native-toolset-builder-2026-11-15/{README.md,DESIGN.md,TODO.md}` — builder profiles, metadata hooks, edit Option A, receipts, exact B/R/P checkpoints and B6 product follow-ups.
- `.agents/plans/ai-native-tool-output-grep-2026-11-15/{README.md,DESIGN.md,TODO.md}` — **authoritative current shared Job/output/grep plan** and detailed every-post-merge checkpoint tests.
- `.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md` — original unified Jobs/Agent/authority design and implementation lineage.
- `.agents/docs/AW-FULL-COMPREHENSIVE-HANDOFF-2026-11-15.md` — broader AW product/Workspace context, not current Job status.
- `.agents/docs/CURRENT_HANDOFF.md` — older post-merge review status; newer Job plan and live source supersede its recorded open wait work.

### dotlink sources/historical plans

- `src/jobs.rs` — process lifetime, shared Job retention, native Task owner, pre-pin contiguous delivery fence, local get/read waits.
- `src/mcp/tools/{jobs.rs,graph.rs,shell.rs,shared_native.rs}`, `src/mcp.rs`, `src/remote_start.rs` (verify actual path) — MCP schemas, budgeted receipts, Graph/Process and validated remote Start.
- `tests/stdio_tools.rs` — real MCP stdio integration and ACK/continuation tests; in-module Job/Process tests cover other boundary cases.
- `.agents/plans/aw-job-output-grep-convergence-2026-11-15/{README.md,DESIGN.md,TODO.md}` — **authoritative dependent D0–D4 plan** with each pre-pin correction.
- `.agents/docs/RELEASE-017-OPERATIONS-PORT-2026-11-15.md` — selective 0.17.1 configuration/NixOS/CLI/transport Phases 1–3 and all seven review fixes. This is NOT AW tool semantic migration.
- `.agents/docs/CURRENT_HANDOFF.md` — finalized original unified runtime/pin, November 14; later port and Job plan supersede its age.
- `.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md` — detailed 33-part original cross-repo source map and design decisions A0–A8.
- `.agents/plans/dotlink-handoff-2026-10-01/{HANDOFF.md,02-CONTEXT-AND-HISTORY.md,10-DECISION-LOG.md,11-VALIDATION-AND-KNOWN-LIMITATIONS.md,13-NEXT-WORK-ROADMAP.md}` — full pre-migration product/security/history baseline. Do not confuse prior operational options with current main.
- `git log --all`, tags `v0.16.8`, `v0.17.1`, `v0.17.2`, `v0.17.3` — complete historical release implementation details; do not edit their worktrees without approval.

## 15. Next implementation slices recommended for ~30-minute user turns

**Slice 1 (read-only):** fresh AW shared diff review + reconcile latest Journal/owner/wait fixes and root/adapter receipts, inspect exact staged/dirty. Record concrete remaining correctness issues, do not copy branch code or commit. Pay attention to terminal journal restart-source eligibility and source/Actor revocation in deferred effects.

**Slice 2 (AW-only, if needed):** fix one independently demonstrable shared correctness issue; add a red/green regression, run affected suite and strict lint, update owning AW plan. Pause.

**Slice 3 (when explicitly authorized):** coordinator-approved signed AW commit on current main, no unrelated files; validate exact Git SHA and remote availability after separately authorized push, then update D0 record. Do not name a SHA in advance.

**Slice 4 (after D0):** dotlink exact three-pin update and mechanical shared Job/output adapter bridge; compile/pin test, no model contract change yet. Pause.

**Slice 5:** dotlink combined 512-byte tail, signed read offsets, optional-offset ACK/Finish, current authority and real MCP Task owners. Qualify actual stdio/HTTP flows and loss/replay. Pause.

**Slice 6:** canonical grep context/file/dir parity plus ToolText MCP metadata and optional host `ts`/turn guidance decisions, followed by comprehensive D4 independent review. Pause after each coherent group rather than doing all at once.

**If the new agent is only preparing handoff**, do not implement any slice merely because these are listed.

## 16. Ready-to-paste prompt for the next agent

> Use **dotlink---l5** as the **only** source-authoring connector. Continue in the existing `main` worktrees at `/home/pvl/spaces/abird/src/aw` and `/home/pvl/spaces/abird/src/dotlink`. Never use A1 or make another worktree. Preserve all unrelated dirty/staged work and the reserved AW `.agents/plans/ux/**` subtree as opaque. Do not commit, push, tag, rebase, release, deploy or repin any dependency without explicit authorization. Keep each implementation user turn to ~30 minutes, checkpoint at the next coherent boundary, update durable docs, and pause.
>
> Read in each repository `AGENTS.md`, `.agents/plans/README.md`, `.agents/plans/TODO.md`, then AW `.agents/docs/AW-DOTLINK-TOOLS-FULL-COMPREHENSIVE-HANDOFF-2026-11-17.md` and dotlink `.agents/docs/DOTLINK-AW-TOOLS-NEXT-AGENT-HANDOFF-2026-11-17.md`. Read AW `.agents/plans/dotlink-tool-port/{README,TODO}.md`, `.agents/plans/ai-native-toolset-builder-2026-11-15/{DESIGN,TODO}.md`, `.agents/plans/ai-native-tool-output-grep-2026-11-15/{DESIGN,TODO}.md` and dotlink `.agents/plans/aw-job-output-grep-convergence-2026-11-15/{DESIGN,TODO}.md`. Follow older linked full handoffs when a design origin is unclear; live source and current tests outrank dated prose.
>
> First verify exact Git status, HEAD/origin, staged/untracked paths, worktrees and all three dotlink AW Git pins. Historical baseline at handoff: AW `7d09d5f` with uncommitted shared Job/output changes (latest plan records 338 shared, 250 adapter and 110 read-only tests), dotlink `6723e3e` ahead of origin/main by one commit with uncommitted MCP output/replay corrections and all AW pins still `0a6c6a0`. Revalidate; do not infer that AW working-tree source is available to dotlink from the committed pin.
>
> Task: perform the next **read-only AW shared-runtime commit-readiness review** first. Inspect Journal/unknown effects, output release/owner/Task lease correctness, exact live Actor/ToolPolicy authority, event-driven get/read wait semantics and the Toolset/receipt/grep factory. Identify any real remaining defects. Only if a concrete issue is confirmed, implement one targeted fix with failure-boundary tests, update the owning plan and pause. Do **not** stage/commit/push or repin automatically. Report exact code paths, test counts, blocker and next step.

## 17. Mechanical cross-repository checkpoint index (never mistake history for current state)

This is a **source-linked index generated from the existing plan/handoff headings** on 2026-11-17. It provides an exact reference for every preserved phase or checkpoint without duplicating hundreds of pages of normative test evidence. For a heading below, open the linked file and search the full heading; read the current plan status before treating a historical checkbox as open or done.

### Original AW port & A0–A8 continuation

- L7: [AW post-merge status — 2026-11-16](../plans/dotlink-tool-port/TODO.md)
- L26: [Current convergence status — 2026-11-09](../plans/dotlink-tool-port/TODO.md)
- L61: [`7ce39d9` — native dotlink-style tooling under original AW authority](../plans/dotlink-tool-port/TODO.md)
- L106: [Hardening parity follow-up (2026-10-26)](../plans/dotlink-tool-port/TODO.md)
- L127: [Parity validation](../plans/dotlink-tool-port/TODO.md)
- L165: [Audit-remediation validation — 2026-10-27](../plans/dotlink-tool-port/TODO.md)
- L186: [Shared toolset and 16-commit dotlink post-sync integration — 2026-10-30](../plans/dotlink-tool-port/TODO.md)
- L225: [Current validation record](../plans/dotlink-tool-port/TODO.md)
- L242: [Five sequential shared-toolset continuation phases — 2026-10-30](../plans/dotlink-tool-port/TODO.md)
- L262: [Five sequential continuation phases TS-N6..N10 — 2026-10-31](../plans/dotlink-tool-port/TODO.md)
- L281: [TS-N6..N10 test evidence](../plans/dotlink-tool-port/TODO.md)
- L291: [Late post-sync dotlink drift — 2026-10-31](../plans/dotlink-tool-port/TODO.md)
- L299: [Fresh eight-pass tooling review/fix — 2026-10-31](../plans/dotlink-tool-port/TODO.md)
- L318: [TS-N11..N20 tooling continuation (2026-10-31)](../plans/dotlink-tool-port/TODO.md)
- L341: [Fresh eight-pass post-N20 review/fix — 2026-10-31](../plans/dotlink-tool-port/TODO.md)
- L361: [Sealed DL-150 convergence checkpoint — 2026-10-31](../plans/dotlink-tool-port/TODO.md)
- L391: [Latest sealed durable-job spec follow-up — 2026-10-31](../plans/dotlink-tool-port/TODO.md)
- L406: [Sealed canonical Job/Graph and Windows shell update — 2026-10-31](../plans/dotlink-tool-port/TODO.md)
- L419: [Core-engine resumed verification — 2026-10-31](../plans/dotlink-tool-port/TODO.md)
- L442: [Parity audit and next-agent handoff — 2026-10-31 (current)](../plans/dotlink-tool-port/TODO.md)
- L475: [Shared `aw-tools` convergence plan — 2026-10-31](../plans/dotlink-tool-port/TODO.md)
- L503: [Fresh pre-pin cross-repo review — 2026-11-01](../plans/dotlink-tool-port/TODO.md)
- L595: [Fresh pre-commit convergence review — 2026-11-05](../plans/dotlink-tool-port/TODO.md)
- L623: [Cross-process private-artifact recovery fix — 2026-11-05](../plans/dotlink-tool-port/TODO.md)
- L647: [Fresh AW configuration/runtime review — 2026-11-08](../plans/dotlink-tool-port/TODO.md)
- L655: [Final cross-repo pin review — 2026-11-09](../plans/dotlink-tool-port/TODO.md)
- L665: [Fresh pre-integration topology review — 2026-11-09](../plans/dotlink-tool-port/TODO.md)
- L673: [Shared convergence verification](../plans/dotlink-tool-port/TODO.md)

*Indexed 28 section headings from this authority file.*

### AW Toolset builder B/R/P chronology

- L24: [Working checkpoint — 2026-11-15 (uncommitted)](../plans/ai-native-toolset-builder-2026-11-15/TODO.md)
- L35: [Current implementation extension — 2026-11-15](../plans/ai-native-toolset-builder-2026-11-15/TODO.md)
- L42: [Metadata hook checkpoint — 2026-11-15 (uncommitted)](../plans/ai-native-toolset-builder-2026-11-15/TODO.md)
- L52: [Compact receipt phase — 2026-11-15](../plans/ai-native-toolset-builder-2026-11-15/TODO.md)
- L59: [Typed receipt checkpoint — 2026-11-15 (uncommitted)](../plans/ai-native-toolset-builder-2026-11-15/TODO.md)
- L68: [Typed Process receipts checkpoint — 2026-11-15 (uncommitted)](../plans/ai-native-toolset-builder-2026-11-15/TODO.md)

*Indexed 6 section headings from this authority file.*

### AW Job/output/grep dated checkpoint chronology

- L35: [Working checkpoint — 2026-11-15 (uncommitted)](../plans/ai-native-tool-output-grep-2026-11-15/TODO.md)
- L44: [Unknown-dominant Graph checkpoint — 2026-11-15 (uncommitted)](../plans/ai-native-tool-output-grep-2026-11-15/TODO.md)
- L53: [Two-phase delivery-proof checkpoint — 2026-11-15 (uncommitted)](../plans/ai-native-tool-output-grep-2026-11-15/TODO.md)
- L64: [Scoped multi-consumer lease checkpoint — 2026-11-15 (uncommitted)](../plans/ai-native-tool-output-grep-2026-11-15/TODO.md)
- L74: [Current Actor Envelope and Control gate checkpoint — 2026-11-15 (uncommitted)](../plans/ai-native-tool-output-grep-2026-11-15/TODO.md)
- L84: [Atomic finite-ACK Control commit checkpoint — 2026-11-15 (uncommitted)](../plans/ai-native-tool-output-grep-2026-11-15/TODO.md)
- L93: [Event-driven finite Job waits — 2026-11-16 (uncommitted)](../plans/ai-native-tool-output-grep-2026-11-15/TODO.md)
- L103: [Node-scoped byte-wait correctness — 2026-11-16 (uncommitted)](../plans/ai-native-tool-output-grep-2026-11-15/TODO.md)
- L110: [Pre-pin wait and disclosure race qualification — 2026-11-16 (uncommitted)](../plans/ai-native-tool-output-grep-2026-11-15/TODO.md)
- L117: [Quota-truncation wake checkpoint — 2026-11-16 (uncommitted)](../plans/ai-native-tool-output-grep-2026-11-15/TODO.md)
- L124: [Empty terminal-output Finish checkpoint — 2026-11-17 (uncommitted)](../plans/ai-native-tool-output-grep-2026-11-15/TODO.md)
- L131: [Post-release owner ACK replay checkpoint — 2026-11-17 (uncommitted)](../plans/ai-native-tool-output-grep-2026-11-15/TODO.md)
- L139: [Post-truncation append notification checkpoint — 2026-11-17 (uncommitted)](../plans/ai-native-tool-output-grep-2026-11-15/TODO.md)
- L147: [Failed-output drain notification checkpoint — 2026-11-17 (uncommitted)](../plans/ai-native-tool-output-grep-2026-11-15/TODO.md)
- L155: [Unknown-journal mismatch watch checkpoint — 2026-11-17 (uncommitted)](../plans/ai-native-tool-output-grep-2026-11-15/TODO.md)
- L162: [Journal-unknown finite Job status and wait checkpoint — 2026-11-17 (uncommitted)](../plans/ai-native-tool-output-grep-2026-11-15/TODO.md)
- L171: [Journal-fenced output mutation checkpoint — 2026-11-17 (uncommitted)](../plans/ai-native-tool-output-grep-2026-11-15/TODO.md)
- L179: [Journal-unknown terminal retention checkpoint — 2026-11-17 (uncommitted)](../plans/ai-native-tool-output-grep-2026-11-15/TODO.md)

*Indexed 18 section headings from this authority file.*

### Unified Jobs/Agent foundational decision sections

- L25: [0. Immediate state — do not edit before reading](UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md)
- L89: [1. Exact reading order for the next agent](UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md)
- L138: [2. Historical lineage — why the current design looks like this](UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md)
- L281: [3. Permanent architecture and decision record](UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md)
- L639: [4. Current AW implementation reality](UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md)
- L809: [5. Current dotlink D1 implementation reality](UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md)
- L908: [6. Final exact cross-repo pin — complete](UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md)
- L963: [7. Runtime completion and final qualification](UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md)
- L1057: [8. Validation discipline](UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md)
- L1087: [9. Git and concurrent-work safety](UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md)
- L1114: [10. Source map for the next implementation agent](UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md)
- L1156: [11. Known decision boundaries / do not improvise](UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md)
- L1178: [12. What not to do next](UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md)
- L1194: [13. Next coherent work sequence](UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md)
- L1210: [14. Copy/paste prompt for the next agent](UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md)
- L1221: [15. Archival/reference map](UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md)
- L1244: [16. Accuracy rule for future handoffs](UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md)

*Indexed 17 section headings from this authority file.*

### Dotlink original 33-section cross-repo handoff

- L13: [1. Read this first](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L60: [2. Original problem and design goal](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L92: [3. Canonical final package architecture](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L157: [4. Package rename decision](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L175: [5. Canonical model-facing tool surface](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L223: [6. Canonical status grammar](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L261: [7. Path and metadata contract](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L295: [8. Hidden names and `.gitignore`](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L312: [9. Direct result/paging contract](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L329: [10. `read` semantics](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L348: [11. Final-symlink operation classes](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L374: [12. Write/edit/patch semantics](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L413: [13. Lifecycle semantics](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L456: [14. Hard-link, mount and reparse decisions](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L467: [15. Graph language](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L496: [16. Deterministic Graph execution](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L520: [17. JobManager / NodePlan ownership](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L552: [18. Process and shell correctness](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L574: [19. Job output and ACK](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L592: [20. Replay and restart boundaries](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L625: [21. AW authority and larger architecture alignment](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L646: [22. AW configuration readiness refactor](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L673: [23. Shared engine maxima versus product limits](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L696: [24. Historical implementation chronology](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L823: [25. Key AW commits in the convergence series](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L872: [26. Current validation evidence](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L910: [27. Remaining genuine work](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L954: [28. Fresh-review checklist before any new implementation](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L984: [29. Source map for the next agent](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L1033: [30. Testing principles](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L1055: [31. Git/working rules](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L1070: [32. Recommended next step](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)
- L1086: [33. Next-agent starting prompt](../../../dotlink/.agents/plans/dotlink-ai-native-filesystem-2026-10-31/FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md)

*Indexed 33 section headings from this authority file.*

### Dotlink selective 0.17.1 operational-port phases

- L5: [Authority and safety](../../../dotlink/.agents/docs/RELEASE-017-OPERATIONS-PORT-2026-11-15.md)
- L15: [Phase 1 — complete: configuration and NixOS](../../../dotlink/.agents/docs/RELEASE-017-OPERATIONS-PORT-2026-11-15.md)
- L26: [Phase 2 — complete: CLI and transport behavior](../../../dotlink/.agents/docs/RELEASE-017-OPERATIONS-PORT-2026-11-15.md)
- L35: [Phase 3 — grouped qualification and operational compatibility](../../../dotlink/.agents/docs/RELEASE-017-OPERATIONS-PORT-2026-11-15.md)
- L49: [Fresh review remediation — 2026-11-15](../../../dotlink/.agents/docs/RELEASE-017-OPERATIONS-PORT-2026-11-15.md)
- L105: [Second fresh review remediation — 2026-11-15](../../../dotlink/.agents/docs/RELEASE-017-OPERATIONS-PORT-2026-11-15.md)
- L133: [Deferred intentionally — AW tool design last](../../../dotlink/.agents/docs/RELEASE-017-OPERATIONS-PORT-2026-11-15.md)
- L137: [Next checkpoint / external gates](../../../dotlink/.agents/docs/RELEASE-017-OPERATIONS-PORT-2026-11-15.md)

*Indexed 8 section headings from this authority file.*

### Dotlink current pre-pin D0–D4/checkpoint chronology

- L19: [AW dependency checkpoint — 2026-11-16](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)
- L24: [Pre-pin standalone Job wait checkpoint — 2026-11-16 (uncommitted)](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)
- L32: [Standalone MCP settlement-wait checkpoint — 2026-11-16 (uncommitted)](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)
- L41: [Contiguous MCP delivery-proof checkpoint — 2026-11-16 (uncommitted)](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)
- L50: [Model ACK vs native MCP Task retention — 2026-11-16 (uncommitted)](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)
- L57: [Node-targeted finite output waits — 2026-11-16 (uncommitted)](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)
- L64: [AW shared-pin prerequisite review — 2026-11-16 (uncommitted)](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)
- L71: [Complete bounded Job status and replay handoff — 2026-11-16 (uncommitted)](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)
- L79: [Bounded ACK/Cancel and remote Control replay — 2026-11-16 (uncommitted)](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)
- L86: [Bounded live stdin and replay — 2026-11-16 (uncommitted)](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)
- L94: [AW shared truncation-notification prerequisite — 2026-11-16 (uncommitted)](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)
- L100: [AW empty-output Finish prerequisite — 2026-11-17 (uncommitted)](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)
- L106: [AW post-release owner-proof prerequisite — 2026-11-17 (uncommitted)](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)
- L112: [AW append-notification prerequisite — 2026-11-17 (uncommitted)](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)
- L117: [AW failed-output watcher prerequisite — 2026-11-17 (uncommitted)](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)
- L123: [AW uncertain-transition notifier prerequisite — 2026-11-17 (uncommitted)](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)
- L129: [AW journal-unknown status / wait prerequisite — 2026-11-17 (uncommitted)](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)
- L135: [AW journal-fenced output retention prerequisite — 2026-11-17 (uncommitted)](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)
- L141: [AW terminal-journal retention prerequisite — 2026-11-17 (uncommitted)](../../../dotlink/.agents/plans/aw-job-output-grep-convergence-2026-11-15/TODO.md)

*Indexed 19 section headings from this authority file.*

## 18. Documentation completeness and handoff accuracy rule

- This handoff is a **versioned synthesis**. The indexed original plans preserve line-level phase requirements, design rationale, tests and decisions all the way back to AW foundations and dotlink original tool implementation. Read the indicated source when making any change; do not silently reduce a historical security requirement to this summary.
- Never infer current Git HEAD, staged work, platform capabilities, number of tests, remote availability, or approval to commit from a handoff. Always verify. If a new bug or decision supersedes an earlier dated statement, update the *owning* live design/TODO and reference the exact source/test evidence rather than rewriting historical records.
- Keep documentation terse and maintenance-light where possible: one canonical 2026-11-17 cross-repo handoff in AW, one dotlink entrypoint, and source-linked existing plans. Future agents should update the **current plan** and only create a new dated full handoff when explicitly requested.
