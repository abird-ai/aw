# CURRENT HANDOFF — AW live state

Author: coordinating implementation agent
Owner: @pvl
Updated: 2026-11-18
Source authority host: **L5 only**
Canonical checkout: `/home/pvl/spaces/abird/src/aw`

> Authority order: live source/tests/executed results > direct user instruction >
> active plan/TODO/spec > dated comprehensive handoffs.
>
> The November 15 comprehensive handoff is historical context for the Workspace
> continuation. It is not the current Git-state record.

## LIVE 0.17 fresh-review convergence checkpoint — 2026-11-19

This section supersedes older state summaries below where they conflict.

Current Git boundary:

- authoritative fresh-review AW implementation checkpoint:
  `7c7de6c9aeedf714adbe67b17ac863f27697e90e`
  (`Fix shared 0.17 projection boundaries`), signed and selected as
  `AW_IMPL_SHA`;
- previous source checkpoint `eb325b31...` is historical and must not be used
  for the dotlink repin;
- `origin/main` is still `792f0b38...` at this docs-edit instant; the user has
  explicitly authorized publishing this checkpoint in the current turn;
- dotlink committed HEAD is
  `8e14ee2b8575c35bc3d5a7204ac15c121410c751`;
  `origin/main` remains
  `5381729cd469eda54691e540deb3fee13e9f3e3e`;
- dotlink also has small uncommitted pre-repin parity fixes from this review;
- nothing is staged or pushed;
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

The publish/repin gate is explicitly authorized in the current turn.
`AW_IMPL_SHA` is fixed at `7c7de6c9aeedf714adbe67b17ac863f27697e90e`.
The direct dotlink AW set to repin atomically is:

- `aw-tools`;
- `aw-tools-fs-unix`;
- `aw-tools-fs-win`;
- `aw-platform`;
- **`aw-mcp-protocol`** (not the full `aw-mcp` client adapter).

Frozen sandbox/config-v13 work remains untouched and deferred.

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
