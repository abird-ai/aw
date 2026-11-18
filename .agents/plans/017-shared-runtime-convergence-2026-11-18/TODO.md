# 0.17 shared runtime convergence

Base: `796225e9b50e824606586859d7bcc88722b4919e`

Design: [README.md](README.md), [DESIGN.md](DESIGN.md)
Audit: [AUDIT.md](AUDIT.md)
Dotlink reference: `v0.17.4 @ fd1cdb6429a341a4e3d27805cae0bec288fc26df`

## Committed

### `796225e` — physical signal identity separated from lifecycle

- [x] `ProcessExit::Signal(i32)`.
- [x] Remove `ProcessExit::{Cancelled,DeadlineExceeded}`.
- [x] Carry Unix reaped signal number through AW adapter.
- [x] Keep Job lifecycle classification separate.
- [x] Real SIGKILL/SIGSEGV distinction test.
- [x] AW-requested termination does not fabricate a signal.

## Uncommitted

### Phase P1 — shared cancellation and stop value types

- [x] Add bounded/Copy `CancellationOrigin` with Caller/Parent/Runtime/Authority.
- [x] Audit caller/runtime/authority/parent cancellation paths; explicit parent-cancel flow makes Parent a distinct origin.
- [x] Add `ProcessStopCause`.
- [x] Add `StopAttemptReason`, `StopOutcome`, `StopAttempt`.
- [x] Define conversion rule: only delivered, claimable attempt can become terminal stop cause.
- [x] Unit tests prove cleanup/drop/already-gone/failed never become terminal stop claims.

### Phase P2 — platform evidence + managed process-tree guard

- [ ] Change shared platform stop primitive to return delivered/gone/failed evidence.
- [ ] Unix process-group evidence: success / ESRCH / failure.
- [ ] Windows Job Object evidence: active-count-aware delivered/gone/failed.
- [ ] Preserve unsandboxed Unix setsid limitation.
- [ ] Add/extend RAII managed process-tree guard with disarm + Drop fail-safe.
- [ ] Failed stop remains armed where a Drop retry is meaningful.
- [ ] Add shared signal-name utility without product prose.
- [ ] Real platform tests for external exit, delivered stop, already-gone and Drop.

### Phase P3 — ProcessTerminal, JobManager, Task detail and lifecycle events

- [ ] Add optional `ProcessStopCause` to `ProcessTerminal`.
- [ ] Preserve zero-allocation ordinary process terminal path.
- [ ] Ensure `ProcessSettlement` / canonical JobManager retain the complete terminal.
- [ ] Audit compact Job/Node status rendering and size bound after new stop token.
- [ ] Add typed `TaskTerminalDetail` (final shape after Task API audit).
- [ ] Preserve reaped process facts behind later internal/spool/projection failure.
- [ ] Preserve prior deadline/disposition when a later internal failure becomes final Task failure.
- [ ] Add separate `ExecutionLifecycleEvent::ProcessStopAttempt`; do not extend Job wake events.
- [ ] Add optional std-side lifecycle event sink; absent sink is no-op.
- [ ] Adapter process guard emits factual attempt event at the signal site and Drop.
- [ ] Tests prove Job lifecycle, process terminal, Task detail and lifecycle event cannot contradict.

### Phase P4 — semantic page vs serialized response budgets

- [ ] Add zero-cost distinct raw-page and serialized-result budget types.
- [ ] Shared JobProfile resolver returns raw page budget only.
- [ ] Keep raw offsets/EOF/delivery proof independent from response envelope.
- [ ] aw-mcp owns serialized response ceiling.
- [ ] Regression: one-byte Job page succeeds inside a normal serialized envelope.
- [ ] Regression: transport envelope may narrow projection without changing page semantics.

### Phase P5 — typed flat Job refs, bounded diagnostics and receipt grammar

- [ ] Add shared bounded flat `j:<handle>[/n:<coordinate>]` type backed by execution_ref primitives.
- [ ] Parse raw transport ref once; use typed identity internally where practical.
- [ ] Add shared bounded diagnostic helper for defense in depth.
- [ ] Extend shared receipt grammar for durable Job handoff.
- [ ] Extend shared receipt grammar for cancellation control receipt.
- [ ] Reuse existing typed Finish/Through ACK.
- [ ] Shared compact fallback preserves same Job + no-rerun semantics under tight budget.
- [ ] Replay projection can become more compact but never changes effect/result identity.
- [ ] Budget and malformed-ref tests.

### Phase P6 — aw-mcp metadata decoration and result fitting

- [ ] Preserve bounded MCP result `_meta` in public AW MCP result boundary.
- [ ] Add shared Toolset receipt decorator using host-supplied timestamp.
- [ ] Support stored immutable replay timestamp vs fresh observation timestamp.
- [ ] Add visible timestamp content only when no typed receipt already carries it.
- [ ] Preserve raw text, structured content and typed media identity.
- [ ] Move generic complete-`CallToolResult` serialized fitter into aw-mcp.
- [ ] Fitter preserves `_meta`, error class and receipt reservation.
- [ ] Explicit error when minimal factual envelope cannot fit.
- [ ] Typed media/structured/replay/tight-budget tests.

### Phase P7 — aw-mcp heartbeat/progress adapter

- [ ] Add optional MCP progress-token heartbeat helper.
- [ ] Monotonic counter, no fake total.
- [ ] Bound each notification send independently.
- [ ] Heartbeat cannot delay/cancel underlying wait.
- [ ] No detached queue or task.
- [ ] Optional host observer gets attempted/delivered summary.
- [ ] Integrate with existing semantic progress where real progress exists.
- [ ] Focused backpressure/deadline/no-token tests.

### Phase P8 — cross-repo shared checkpoint

- [ ] Re-run full affected AW suites.
- [ ] Strict all-target/all-feature Clippy.
- [ ] no-default/minimal/stateless/no_std boundaries where touched.
- [ ] Downstream aw-mcp/host/cli checks.
- [ ] Review public API for unnecessary allocation or product-specific wording.
- [ ] Signed AW implementation checkpoints.
- [ ] Publish only with explicit user authorization.
- [ ] Dotlink companion repins all AW dependencies atomically to exact published commit.

### Phase P9 — dotlink thin consumer

Owned by dotlink companion plan.

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
- [x] P1 value-type tests — focused 4/4 plus full `aw-tools` **346/346**, strict Clippy, no-default build and downstream adapter check.
- [ ] P2 native process-stop evidence tests.
- [ ] P3 Job/Task/event propagation tests.
- [ ] P4 budget-separation tests.
- [ ] P5 ref/receipt/diagnostic tests.
- [ ] P6 MCP metadata/fitter tests.
- [ ] P7 heartbeat tests.
- [ ] full AW shared-first convergence gate.
- [ ] dotlink v0.17.4 consumer/equivalence gate.
