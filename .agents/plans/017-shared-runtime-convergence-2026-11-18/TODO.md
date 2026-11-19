# 0.17 shared runtime convergence

Base: `796225e9b50e824606586859d7bcc88722b4919e`

Design: [README.md](README.md), [DESIGN.md](DESIGN.md)
Audit: [AUDIT.md](AUDIT.md)
Dotlink reference: `release/0.17 @ 887401398da297c458ffa2a152a07e1b85870c35`
(`v0.17.4 fd1cdb6` plus post-tag platform signal-label test cleanup `8874013`)

## Committed

### `796225e` — physical signal identity separated from lifecycle

- [x] `ProcessExit::Signal(i32)`.
- [x] Remove `ProcessExit::{Cancelled,DeadlineExceeded}`.
- [x] Carry Unix reaped signal number through AW adapter.
- [x] Keep Job lifecycle classification separate.
- [x] Real SIGKILL/SIGSEGV distinction test.
- [x] AW-requested termination does not fabricate a signal.

## Uncommitted

### Phase P1 — simplify and freeze shared stop vocabulary

- [x] `021b8ce` proved the first stop-attempt/cause invariants and four
      cancellation origins.
- [x] Parent/subagent audit proved richer cancellation provenance exists but does
      not change the low-level process stop primitive.
- [x] Remove the transitional public taxonomy, including `CancellationOrigin`,
      `StopOrigin`, `ProcessStopCause`, `StopAttemptReason`, `StopOutcome`,
      `StopAttempt`.
- [x] Freeze minimal `StopReason::{Timeout,Cancelled,RuntimeError}`; every variant
      is a legitimate terminal attribution.
- [x] Keep `StopEvidence::{Stopped,NoTarget,Unknown}`.
- [x] Use `StopRecord { reason: Option<StopReason>, evidence }`; `None` is
      housekeeping, not a synthetic Cleanup/Dropped reason.
- [x] Add const `new/stopped/no_target/unknown/housekeeping` constructors.
- [x] Make `terminal_reason()` exactly `Some(reason) + Stopped`; no second
      claimability allowlist.
- [x] Keep cleanup/Drop/caller/parent/runtime/authority as higher-layer hook or
      product context rather than canonical process state.
- [x] Prove NoTarget is positive absence distinct from unresolved Unknown;
      Guard disarm/retry behavior remains a P2 platform/RAII invariant.
- [x] Keep final values bounded/Copy and no-default-features compatible.

### Phase P2 — platform evidence + managed process-tree guard

- [x] Preserve platform layering: low-level stop returns `io::Result<bool>` and
      adapter maps `true/false/error` exactly to Stopped/NoTarget/Unknown.
- [x] Unix process-group evidence: success / ESRCH / unknown failure.
- [x] Windows Job Object implementation: active-count-aware
      Stopped/NoTarget/Unknown mapping, following v0.17.4 reference semantics.
- [x] Preserve/document unsandboxed Unix `setsid` escape limitation.
- [x] Add cross-platform `ManagedProcessGuard` containment owner with disarm +
      Drop fail-safe.
- [x] NoTarget/Stopped disarm; Unknown remains armed where a Drop retry is meaningful.
- [x] Preserve the original semantic reason across Unknown/Drop retry; Drop does
      not become a stop reason.
- [x] Add shared platform `signal_name()` utility without product prose/state.
- [x] Unix native tests cover live delivered stop, already-gone/external exit,
      Drop fail-safe and signal-name projection.
- [ ] Run the authored Windows native Job Object live/empty-membership test on a
      Windows target; L5 has no installed `x86_64-pc-windows-gnu` std target.

### Phase P3 — ProcessTerminal, JobManager, Task detail and lifecycle events

- [x] Add optional `StopReason` to `ProcessTerminal` from proven `StopRecord`.
- [x] Preserve allocation-free ordinary process terminal facts (`Copy` only).
- [x] Ensure `ProcessSettlement` / canonical JobManager retain the complete terminal.
- [x] Render bounded factual `stop=timeout|cancelled|runtime_error` in compact
      Process/Job status without product wording.
- [x] Add compact `TaskTerminalDetail { exit, stop }`; avoid embedding the full
      output-heavy `ProcessTerminal` or allocating in ordinary Task replies.
- [x] Preserve reaped exit/stop facts behind later internal/spool/projection failure.
- [x] Preserve prior timeout via `TaskTerminalDetail.stop=Timeout`; no redundant
      prior-disposition field is needed.
- [x] Add separate `ExecutionLifecycleEvent::ProcessStop`; do not extend Job wake events.
- [x] Add optional std-side lifecycle event sink; existing constructors remain no-op.
- [x] Keep canonical event facts minimal; richer provenance/path/Job context is
      captured by host/adapter observer context, not a generic metadata bag.
- [x] Adapter process guard emits factual events for graceful/forced/housekeeping/Drop attempts.
- [x] Tests prove Job lifecycle, physical exit, semantic stop, Task detail and
      lifecycle events remain orthogonal and factual.

### Phase P4 — semantic page vs serialized response budgets

- [x] Add zero-cost distinct `RawPageBytes` and `SerializedResultBytes`
      transparent non-zero budget types.
- [x] Shared `JobProfile::resolve_read_page` returns raw page budget only.
- [x] Keep raw offsets/EOF/delivery proof independent from response envelope;
      only the actually serialized raw prefix receives delivery credit.
- [x] `aw-mcp` owns the serialized complete-result ceiling via
      `SerializedResultBytes`; generic fitting/metadata remains P6.
- [x] Regression: one-byte Job page succeeds inside a normal 512-byte serialized envelope.
- [x] Regression: serialized envelope may narrow projection without changing
      the source raw page or retained-byte semantics.

### Phase P5 — typed flat Job refs, bounded diagnostics and receipt grammar

- [x] Add shared bounded flat `JobRef` with exact
      `j:<handle>[/n:<coordinate>]` parse/render grammar.
- [x] Keep hierarchical `ExecutionRef` for ownership/routing; finite Job
      transport output uses `JobRef` and prior hierarchical finite refs remain
      compatibility input.
- [x] Add shared 16 KiB UTF-8-safe bounded diagnostic helper; Task errors reuse it.
- [x] Make finite Job receipts consume typed `JobRef`, not arbitrary strings.
- [x] Add shared durable `Handoff` receipt and `render_handoff` fallback.
- [x] Add shared idempotent `Cancel` control receipt.
- [x] Reuse existing typed Finish/Through ACK and consume it from finite adapters.
- [x] Shared compact fallback preserves exact same Job + no-rerun semantics under tight budget.
- [x] Replay/tighter projection changes only receipt richness, never Job/effect/result identity.
- [x] Add malformed/oversized flat-ref, UTF-8 diagnostic and tight-budget receipt tests.

### Phase P6 — aw-mcp metadata decoration and result fitting

- [x] Preserve bounded result-level MCP `_meta` in public `McpToolResult`;
      whole result metadata is capped at 16 KiB.
- [x] Make `SerializedResultBytes` enforce actual serialized `CallToolResult`
      size at the raw result boundary.
- [x] Add shared Toolset receipt decorator with host-supplied timestamp and no clock.
- [x] Preserve stored `io.abird/receipt.ts` as immutable replay timestamp;
      fresh selected results require the host observation timestamp.
- [x] Add visible timestamp sidecar only when the exact `ts=` line is absent.
- [x] Preserve existing result metadata keys, structured JSON and typed media identity.
- [x] Add public generic complete-`CallToolResult` fitter in `aw-mcp`.
- [x] Fitter preserves metadata/result type/error class/block identity and
      atomically reserves receipt-sidecar overhead.
- [x] Only text is shrinkable; immutable media/structured facts that cannot fit
      return explicit `Limit` rather than being rewritten as text.
- [x] Add typed media/structured/replay/metadata/tight-budget/minimal-envelope tests.

### Phase P7 — aw-mcp heartbeat/progress adapter

- [x] Add optional token-owned `ProgressHeartbeat` +
      `await_with_progress_heartbeat`; server peer/request context is supplied
      by the product as a send closure, so `aw-mcp` keeps its client-only rmcp feature set.
- [x] Emit monotonic exact-integer heartbeat counters with no fake total/message.
- [x] Bound every notification send independently by host send budget and
      optional operation send deadline.
- [x] Poll the underlying operation concurrently with a pending send, so
      heartbeat backpressure cannot delay or cancel completion.
- [x] Use no detached queue/task; tests prove no post-return sends.
- [x] Add optional RAII observer summary `{attempted,delivered}` on both normal
      completion and wrapper cancellation/drop.
- [x] Add `semantic_progress_notification` /
      `notify_semantic_progress_bounded` using existing
      `filesystem::progress::End` for real known/unknown semantic progress.
- [x] Add focused no-token, monotonic, backpressure, independent-send-budget,
      expired-deadline, wrapper-cancel, observer and semantic-progress tests.

### Phase P8 — cross-repo shared checkpoint

- [x] Re-run full affected AW suites: core, adapter, MCP, Unix platform and terminal all green.
- [x] Strict all-target/all-feature Clippy across every touched crate is green.
- [x] Recheck no-default/minimal/stateless boundaries: aw-tools no-default,
      aw-tools batch-only, adapter no-default, Unix platform no-default,
      host-side Windows platform and aw-mcp all compile.
- [x] Downstream aw-mcp/host/cli checks plus full workspace all-feature compile are green.
- [x] Review public API for unnecessary allocation/product wording. Tightened
      `ProgressHeartbeatConfig` fields behind its validating constructor and
      `JobRef` fields behind existing typed constructor/accessors; no new
      product-specific shared semantics found.
- [x] Resolve the D1 source-map API gap: `aw-platform` now exposes
      `exit_signal(&ExitStatus) -> Option<i32>` (Unix factual signal extraction,
      Windows `None`), and the AW process adapter consumes that same helper.
      Focused signal/lifecycle tests, Unix platform **13/13 + 4/4 PTY**, adapter
      **256/256**, terminal **20/20**, strict affected Clippy, minimal configs,
      downstream host/CLI and full workspace all-feature check are green.
      Windows native assertion is authored but remains unexecuted on L5 because
      the Windows Rust std target is unavailable.
- [ ] Signed AW implementation checkpoints — requires explicit user authorization
      before staging/committing this accumulated P1-P8 working tree.
- [ ] Publish — requires separate explicit user authorization; nothing published.
- [ ] Dotlink companion repin — blocked until an exact AW implementation commit
      is authorized, signed and published; do not pin a dirty working tree.
      Mechanical commit/publish/repin order, the five-direct-dependency pin set,
      validation gates and rollback conditions are frozen in
      [PUBLISH-REPIN-PREFLIGHT.md](PUBLISH-REPIN-PREFLIGHT.md).

### Phase P9 — dotlink thin consumer

Owned by dotlink companion plan.

Current sequencing: dotlink D1-D7 shared-semantic hookup is blocked until this
exact P1-P8 AW implementation is explicitly authorized for signed commit/publish
and dotlink is atomically repinned. No temporary sibling/path dependency is
allowed. Product-owned work that does not duplicate AW semantics may proceed:
dotlink D8 Nix environment hardening is complete, D9 pre-repin
docs/equivalence/baseline is complete, the D2 structured logger prerequisite is
non-panicking/Drop-safe with a production-used always-visible lifecycle channel,
D6 compatibility is locked with real stdio/hosted Image+Audio+Resource identity,
structured/receipt regressions, a local text-only fitter that rejects
immutable-over-budget results, and transactional 16 KiB result-metadata receipt
stamping with stored-ts replay stability/no duplicate visible marker; dotlink
pre-repin D4/D5 locks
hostile-ref compactness plus one-byte raw-page/independent-envelope behavior;
D7 polls the operation through progress backpressure and drops a pending send
when the result wins. The actual shared stop/result/ref/budget/MCP type and
observer hookups remain blocked.

- [ ] Consume shared stop/cancellation/process terminal semantics.
- [ ] Consume shared Task failure detail.
- [ ] Consume shared Job refs/receipts/budgets.
- [ ] Consume aw-mcp decorator/fitter/heartbeat.
- [ ] Keep only product wire/log/config/transport policy.
- [ ] v0.17.4 equivalence matrix passes.

### Deferred Phase S — sandbox/mount convergence

- [ ] **DO NOT IMPLEMENT in this active plan.**
- [ ] Pull/promote sibling sandbox plan only after explicit user instruction.

## Tests and verification

- [x] `cargo test --offline --locked -p aw-tools --all-features --lib` — `796225e`.
- [x] `AW_BASH="$(command -v bash)" cargo test --offline --locked -p aw-tools-adapter --all-features --lib` — **253/253**, `796225e`.
- [x] strict affected Clippy — `796225e`.
- [x] downstream aw-mcp/aw-host/aw-cli checks — `796225e`.
- [x] Transitional P1 value-type gate at `021b8ce` — focused 4/4 plus full `aw-tools` **346/346**, strict Clippy, no-default build and downstream adapter check.
- [x] Final minimal P1 stop vocabulary — focused **5/5**; full `aw-tools --all-features --lib` **347/347**; strict all-target/all-feature Clippy; `aw-tools --no-default-features` check; downstream `aw-tools-adapter --all-features` check; rustfmt; `git diff --check`.
- [ ] P2 native process-stop evidence gate — Unix `aw-platform-unix` **13/13** plus integration files **4/4** (1 cross-UID fixture intentionally ignored); `aw-tools-adapter --all-features --lib` **254/254**; `aw-terminal --lib` **20/20**; strict affected Clippy and host-side `aw-platform-win` check green. Windows native Job Object test is authored but unexecuted on L5 because the Windows Rust std target is not installed.
- [x] P3 Job/Task/event propagation — full `aw-tools --all-features --lib` **348/348**; `aw-tools-adapter --all-features --lib` **255/255**; `aw-terminal --lib` **20/20**; strict core+adapter Clippy; no-default core; downstream `aw-mcp`/`aw-host`/`aw-cli` all-feature checks green.
- [x] P4 budget separation — full `aw-tools --all-features --lib` **350/350**; `aw-tools-adapter --all-features --lib` **256/256**; `aw-mcp --all-features --lib` **19/19**; strict core+adapter+MCP Clippy; no-default core; downstream `aw-host`/`aw-cli` all-feature checks; rustfmt and `git diff --check` green.
- [x] P5 refs/receipts/diagnostics — full `aw-tools --all-features --lib` **354/354**; `aw-tools-adapter --all-features --lib` **256/256**; `aw-mcp --all-features --lib` **19/19**; strict core+adapter+MCP Clippy; no-default core; downstream `aw-host`/`aw-cli` all-feature checks; rustfmt and `git diff --check` green.
- [x] P6 MCP metadata/fitter — `aw-mcp --all-features --lib` **24/24**; full `aw-tools` **354/354**; full adapter **256/256**; strict MCP+core+adapter Clippy; no-default core; downstream `aw-host`/`aw-cli`; rustfmt and `git diff --check` green.
- [x] P7 heartbeat/progress — `aw-mcp --all-features --lib` **33/33**; full `aw-tools` **354/354**; full adapter **256/256**; strict MCP+core+adapter Clippy; no-default core; downstream `aw-host`/`aw-cli`; rustfmt and `git diff --check` green.
- [ ] full AW shared-first convergence gate — all host-available P1-P8 gates are green, including the pre-publish `exit_signal` fact required by dotlink D1; native Windows Job Object/exit-signal tests remain authored but unexecuted because L5 has no Windows Rust std target, and commit/publish gates remain authorization-blocked.
- [ ] dotlink v0.17.4 consumer/equivalence gate.
