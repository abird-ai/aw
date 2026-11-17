# CURRENT HANDOFF — AW live state

Author: coordinating implementation agent
Owner: @pvl
Updated: 2026-11-17
Source authority host: **L5 only**
Canonical checkout: `/home/pvl/spaces/abird/src/aw`

> Authority order: live source/tests/executed results > direct user instruction >
> active plan/TODO/spec > dated comprehensive handoffs.
>
> The November 15 comprehensive handoff is historical context for the Workspace
> continuation. It is not the current Git-state record.

## Latest AW-tools published checkpoint — 2026-11-17

- Shared-runtime implementation commit:
  `be6d48db552fd44b9dd85364cd8734454109ce68`
  — `Harden shared Job output and journal uncertainty`; docs/status follow-up
  `403eb8473498454a97871702ff37aaedb1de9ee1` is the current AW `main` tip.
- `HEAD == origin/main == 403eb8473498454a97871702ff37aaedb1de9ee1` at the
  start of the current D1 follow-up. Dotlink D0 consumes `be6d48d` in signed
  commit `d0a392234e918bd4ec9f0fbd7ab6fe771e1eb696`.
- Git reports **Good ED25519 signatures** for the published AW commits using key
  fingerprint `SHA256:4aB+NvRb1shCx4xjtZDr2Y7FzfHGph80eFtpF+pHuCI`; L5 has
  no `allowed_signers` principal file, so local trust status is `U` rather than
  a principal-mapped `G`.
- Published implementation qualification: `aw-tools` **339/339**,
  `aw-tools-adapter` **251/251**, read-only shared **110/110**, strict
  all-target/all-feature Clippy, formatting/diff checks, minimal shared build,
  MCP, minimal CLI and full CLI all green.

## Current uncommitted D1 shared prerequisite — 2026-11-17

- Dotlink's first D1 delegation removed its duplicate shared delivery watermark
  and exposed one shared bug: strict `acknowledge_delivered_output` rejected a
  lost-response replay of an already-ACKed owner watermark after another owner
  legitimately compacted the physical retained prefix below it.
- The live AW fix validates exact owner + delivery history first, accepts only an
  already-committed `through <= owner.through` replay, and still requires current
  retained bytes for every **new** ACK advance. Foreign owners and new ACKs into
  compacted-away bytes remain rejected.
- The owner-aware compaction regression now exercises the strict API for both the
  replaying Job owner and independent Task owner. Full shared **339/339** and
  minimal Job-runtime **200/200** tests pass; strict all-target/all-feature
  `aw-tools` Clippy, format and diff checks pass.
- This follow-up is not committed or pushed. Dotlink must not path-patch or add a
  host-side bypass; finish D1 only after the normal signed AW commit/push/repin
  sequence is explicitly authorized.

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
