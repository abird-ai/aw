# Shared toolset builder

Base AW: `62b8da6babfa729d6bbb9c21e75ac89c228b22b1`
Authoritative design: [DESIGN.md](DESIGN.md)
Merged implementation: `0d2b4e3` — `Build configurable toolset and harden shared Job control`.
Current review HEAD: `7d09d5f`.

## Committed

- [x] `0d2b4e3`: B1–B5, B7–B10, R1–R4 and P1–P4 are merged on
  `main`. The historical checkpoint sections below intentionally retain their
  pre-merge wording as provenance; they are not evidence that those slices are
  still uncommitted.

## Open

- [ ] B6. Targeted qualification and exact implementation handoff; then revisit AW/dotlink-specific public options and compact receipt grammar before dotlink cutover.
- [ ] Product-specific receipt/timestamp/turn-horizon defaults and MCP metadata
  adoption remain host decisions; the shared renderer must stay clock-free and
  authority-free.
- [ ] Job observation waits remain owned by the Job output/grep plan:
  `jobs_get` settlement-only event wait and finite `jobs_read` byte/EOF wait.

## Working checkpoint — 2026-11-15 (uncommitted)

- **B1 implemented:** new `crates/aw-tools/src/toolset.rs` with `Toolset::builder()`, typed `GrepProfile`, `JobProfile`, `ReceiptProfile`, enabled filesystem-tool subset and validation; no execution/authority/clock dependencies. Default 512/1024 combined Job tails, 30s waits, 128-line grep context ceiling.
- **B2 implemented:** configured `SchemaCaps.grep_context_default/max`, `GrepLimits.context_max`, canonical parser validates **all explicit context inputs**, and shared search/direct+Graph use the same bound. `Toolset::parse_filesystem` uses enabled set + configured schema; AW direct Search consumes configured work/context. The engine maximum remains 128 independent of host max.
- **B3 implemented:** canonical titles and per-field descriptions from shared schema; `Toolset::filesystem_descriptor` and `describe_operation` yield host-consumable metadata. Dormant tools can be *described* before authorization/validation without accidentally requiring unrelated disabled capabilities. AW native tool specs and Job get/read/list descriptors use the shared factory; host-specific Agent-history/control descriptions remain intact.
- **B4 implemented as pure shared opt-in:** `ReceiptProfile{unix_ts,checkpoint_minutes}`, `model_instructions()` and `render_receipt(body,host_timestamp,budget)`. No core clock/timer, no raw file decoration. **Applying host-specific receipt timestamps and full compact-result grammar is intentionally deferred**; AW/dotlink will explicitly choose renderer calls after product design.
- **B5 implemented:** AW `LocalToolOptions::with_toolset` / `LocalToolLimits::with_toolset` map canonical schema, grep work and Job profiles without issuing grants or bypassing AW's narrowing checks. AW Jobs runtime applies configured list/get/read ceilings and combined-tail defaults.
- **B6 remains:** write/review product-specific choices (multi-file edit vs Graph, output receipt grammar, full Job wait/ACK authority, timestamp host integration) before dotlink pins or code change. The predecessor Job/grep plan retains its own unresolved runtime safety gates. This builder phase makes no claim those gates passed.
- **Executed green:** `cargo test --offline --locked -p aw-tools --all-features --lib`; `AW_BASH="$(command -v bash)" cargo test --offline --locked -p aw-tools-adapter --all-features --lib` (**232 passed**); `cargo test --offline --locked -p aw-tools --features read-only --lib toolset::tests` (**4 passed**); `cargo check --offline --locked -p aw-tools --no-default-features`; strict `cargo clippy --offline --locked -p aw-tools -p aw-tools-adapter --all-targets --all-features -- -D warnings`; scoped `cargo fmt --check`, `git diff --check`. Focused configured grep parser/host integration + configurable Jobs spec tests are green.
- No commit, push, version bump, worktree change, external-provider effect, AW UX edits, or dotlink code/pin changes.

## Current implementation extension — 2026-11-15

- [x] B7. Add builder-only `ToolText` overrides for title, main description, suffix and root/nested argument descriptions keyed by immutable `OperationId`; typed placeholder expansion from already-validated toolset limits.
- [x] B8. Reject malformed metadata and unknown argument paths; no schema/effect/name/authority mutations, no hidden tools made available.
- [x] B9. Wire per-tool metadata through real AW `configure_with_toolset` registration and delegate all execution/validation to the original Tool; confirm direct/Job/batch/shell descriptions are customized without changing tool results.
- [x] B10. Confirm single-file `edit` + existing Graph/multi-node composition, no new multi-file edit API; focused catalog and subset tests.

## Metadata hook checkpoint — 2026-11-15 (uncommitted)

- **B7 implemented:** `ToolText::new().title(...).description(...).description_suffix(...).argument(path,...)` and `ToolsetBuilder::tool_text(OperationId, ToolText)`, supporting all 25 canonical operation identities. Repeated calls merge field overrides, append hints and resolve validated profile placeholders once. `Toolset` is immutable; metadata is in `BTreeMap`, not a runtime callback/dispatch layer.
- **B8 implemented:** bounded single-line metadata validation, max aggregate bytes and number of entries, known typed placeholder expansion, root/nested argument JSON Pointer resolution, and strict unknown-field rejection. Filesystem argument references validate at build even when disabled; Process/Job/Graph fields validate against their host-supplied schema before registration. Only `description` text changes, never operation IDs, effects, schema shapes, default limits or grants.
- **B9 implemented:** `configure_with_toolset(builder, actor, options, &toolset)` projects toolset limits and metadata through AW's existing AgentBuilder/ToolPolicy/ToolRegistry; a thin `DescribedTool` delegates `validate`, `execute`, `is_available` and `can_run_parallel` to the original tool. `Toolset::only` restricts available AW native and Graph leaf tools, while existing AW selected names, ToolMode and positive authority remain independent. Existing `configure` behavior is unchanged. `LocalToolOptions::with_toolset` remains limits-only, not the metadata registration API. AW `ToolSpec` has no title field; the shared `Descriptor.title` is available for hosts such as MCP.
- **B10 implemented:** focused `edit` remains one file; live AW `batch` test edits two independent files in two Graph nodes, then asserts the same replay identity cannot execute them again. No import of dotlink legacy multi-file edit.
- **Executed green after hook work:** `cargo test --offline --locked -p aw-tools --all-features --lib` **318 passed**; `AW_BASH="$(command -v bash)" cargo test --offline --locked -p aw-tools-adapter --all-features --lib` **234 passed**; `cargo check --offline --locked -p aw-tools --no-default-features`; `cargo test --offline --locked -p aw-tools --features read-only --lib toolset::tests` **9 passed**; strict `cargo clippy --offline --locked -p aw-tools -p aw-tools-adapter --all-features --all-targets -- -D warnings`; crate-scoped `cargo fmt --check` and owned `git diff --check`.
- **Still pending:** user-visible receipt grammar and host-specific metadata/turn-horizon selection, plus the earlier Job authorization, ACK/delivery and finite-wait gates. Builder metadata hooks are qualified within the two affected crates; whole-product release gates, dotlink adoption and signing are **not** qualified.
- No commits, pushes, version bump, dotlink source/pin change, or changes to AW's reserved UX subtree.

## Compact receipt phase — 2026-11-15

- [x] R1. Closed typed shared `Receipt` projection with Job snapshot + ACK fact variants; no arbitrary raw text receipt API or alternate Job state.
- [x] R2. Budget-safe optional `ts` via host-supplied UTC Unix seconds and immutable `ReceiptProfile`; no clock or raw-output decoration.
- [x] R3. Use the same formatter in AW `jobs_list/get` and appropriate constructor handoffs; no change to authority, replay, exact Job state or output delivery.
- [x] R4. End-to-end adjacent status/receipt tests, strict affected-crate lint and minimal feature build.

## Typed receipt checkpoint — 2026-11-15 (uncommitted)

- **R1 implemented:** `aw-tools/src/receipt.rs` exposes a closed, trusted-fact `Receipt` projection: canonical `JobStatus { ref, &Snapshot }`, `NodeStatus { ref, tool, rev, NodeState }`, and typed `Ack { ref, Finish|Through(offset), changed, released }`. No free-form raw-body receipt entry point. The manager snapshot, original NodeState, and existing Primary semantics remain authoritative; renderer never changes Job state.
- **R2 implemented:** `Toolset::render_receipt(Receipt, host_unix_ts, total_budget)` reserves the *entire* optional timestamp footer and rejects a receipt if its status or metadata cannot fit. No implicit clock, raw file / retained output decoration, partial or fabricated fields, or ACK mark. UTC timestamp is supplied by the embedding host and enabled only through `ReceiptProfile`; live product defaults remain undecided.
- **R3 implemented for status:** AW `jobs_list`/root `jobs_get`, Node `jobs_get`, and the existing Process/Graph constructor handoff now call the same pure typed status renderer rather than three divergent string formatters. Existing output-tail disclosure, replay, and ToolPolicy/Envelope paths were not changed. ACK variant is **projection-only**; live `jobs_ack` integration is deferred until ownership/preflight/delivery security gates pass.
- **R4 validated:** `aw-tools` **322 passed** (`--all-features --lib`), `aw-tools-adapter` **234 passed** (`--all-features --lib`, `AW_BASH` set); minimal `aw-tools --no-default-features` check and read-only receipt tests; strict `cargo clippy -p aw-tools -p aw-tools-adapter --all-targets --all-features -- -D warnings`, scoped fmt and diff checks. Tests include root/Graph node lifecycle, mixed/unknown *preservation*, result-size boundary, injected newline rejection, exact ACK receipt, timestamp enabled/disabled, and existing Process/handoff regressions.
- **Follow-up contract:** optional typed Process exit/stream byte facts and compact `next` guidance must come from trustworthy typed sources, not parsing `Snapshot.detail` or raw output; do not silently add these fields or consume more than configured output budgets. Graph `unknown` dominance is addressed by the separate Job/grep checkpoint; `jobs_get` settlement-only wait and ACK owner scope remain active correctness gates in Job/grep convergence.
- **No git commit/push/version/pin change**, no dotlink source changes and no changes to reserved AW UX subtree.

## Typed Process receipts checkpoint — 2026-11-15 (uncommitted)

- [x] **P1 (implementation only):** `Receipt::JobStatus` and `Receipt::NodeStatus` accept `Option<ProcessTerminal>` from the original shared `JobManager::process_terminal`; the shared formatter adds exact `exit`, raw `out`/`err` total byte counts and truthful truncation/quiescence flags. It does **not** parse `Snapshot.detail` or reclassify Primary outcomes. Process terminal facts are forbidden for running root/node status. This remains an **uncommitted** checkpoint, not a committed TODO completion.
- [x] **P2 (implementation only):** typed `NextAction::ObserveJob` adds `next=jobs_get|jobs_read no-rerun` only to constructor handoff receipts. Normal `jobs_get` / `jobs_list` do not carry a repetitive hint. Graph roots do not aggregate child output or process fields. The hint is advice, not a scheduler decision or retry command.
- [x] **P3 (implementation only):** AW `jobs_get` uses the same shared renderer for terminal Process roots and settled Process-backed Graph nodes; the existing constructor handoff queries typed Process facts for terminal single-node Jobs. AW read/ACK permissions, underlying Job transitions, replay and retained output readers remain unchanged. Running stream byte counters are **not yet exposed** by this slice; completed Process totals are factual.
- [x] **P4 (qualification only):** `cargo test --offline --locked -p aw-tools --all-features --lib` **323 passed**; `AW_BASH="$(command -v bash)" cargo test --offline --locked -p aw-tools-adapter --all-features --lib` **234 passed**; `cargo check --offline --locked -p aw-tools --no-default-features`; `cargo test --offline --locked -p aw-tools --features read-only --lib receipt::tests` passed; strict Clippy, formatting and owned diff checks passed. Specific tests cover typed exit, 20,001/732 raw byte totals, truncated retained prefix marker, unknown output, atomic result budget, forbidden nonterminal metadata and explicit no-rerun handoff.
- [ ] **Next:** separate security/semantics plan still owns delivery-proof on actual rendered bytes, Job-scoped vs caller-scoped output leases, unknown Graph dominance, and `jobs_get` completion-only / `jobs_read` byte-wait contracts. Cross-product receipt settings and MCP metadata adoption remain explicitly pending; do not pre-empt those changes by altering this renderer.
- No commit/push, new worktree, version/pin update, dotlink source edit or AW UX subtree modifications.

## Tests and verification

- [x] Validate preset/default/narrowed/invalid profiles and no feature/authority expansion.
- [x] Direct/Graph shared grep/schema parity and output-limit continuity.
- [x] Exact docs/title fields, including dynamic numbers and no undocumented properties/placeholder drift.
- [~] Shared receipt timestamp/size/disabled-mode behavior is qualified; actual
  product timestamp/turn-horizon selection remains open above.
- [x] `cargo fmt --check`, strict affected-crate Clippy, targeted
  `aw-tools` + `aw-tools-adapter` suites and minimal feature build.
- [x] Merge review preserves independent work; no push/version/pin change.

## Post-merge review — 2026-11-16

- [x] Revalidated merged commit `0d2b4e3` from current `7d09d5f`: Toolset
  subsets constrain both direct registration and Graph native leaves; metadata
  wrappers remain projection-only and delegate original validation/execution;
  AW ToolMode/selection/ToolPolicy/Actor authority remain independent.
- [x] Fixed four Rust-1.98 strict-Clippy `chunks_exact(2)` findings in shared
  hex/digest parsing using `slice::as_chunks::<2>()`. Rust documents that API
  as stable since 1.88, matching the declared repository MSRV.
- [x] Fixed stale hard-coded Job-profile diagnostics: tail and wait validation
  already used configured ceilings but no longer falsely reports fixed
  1024-byte/30-second limits.
- [x] Fresh stable-tree gates: `aw-tools` **327/327**,
  `aw-tools-adapter` **242/242**, Agent core **90 passed / 1 ignored**,
  read-only Toolset **9/9**, minimal `aw-tools`, strict affected-crate
  all-target/all-feature Clippy, rustfmt, plus downstream `aw-mcp`,
  `aw-host` and `aw-cli --lib` checks.
- [ ] B6 still owns cross-product product defaults and dotlink adoption. In
  particular, define/document product-neutral shared grep hard maxima before a
  consumer intentionally configures work budgets above AW's default ceilings;
  do not invent those caps in an AW-only review.
