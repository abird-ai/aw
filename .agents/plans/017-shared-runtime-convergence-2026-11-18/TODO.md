# 0.17 shared runtime convergence

Base: `796225e9b50e824606586859d7bcc88722b4919e`

Design: [README.md](README.md), [DESIGN.md](DESIGN.md)
Audit: [AUDIT.md](AUDIT.md)
Current dotlink release reference:
`release/0.17 = origin/release/0.17 = v0.17.5 =`
`3995448d7bf33794ef84c7bb51150d77cc260ada`.
Historical v0.17.4 baseline remains `fd1cdb6429a341a4e3d27805cae0bec288fc26df`.

## Committed

### `796225e` — physical signal identity separated from lifecycle

- [x] `ProcessExit::Signal(i32)`.
- [x] Remove `ProcessExit::{Cancelled,DeadlineExceeded}`.
- [x] Carry Unix reaped signal number through AW adapter.
- [x] Keep Job lifecycle classification separate.
- [x] Real SIGKILL/SIGSEGV distinction test.
- [x] AW-requested termination does not fabricate a signal.

## Implementation phases and current P9 queue

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
- [x] Shared MCP projection owns the serialized complete-result ceiling via
      `SerializedResultBytes`; generic fitting/metadata lives in the lightweight
      `aw-mcp-protocol` crate and `aw-mcp` re-exports it for client users.
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

### Phase P6 — MCP protocol metadata decoration and result fitting

- [x] Preserve bounded result-level MCP `_meta` in public `McpToolResult`;
      whole result metadata is capped at 16 KiB.
- [x] Make `SerializedResultBytes` enforce actual serialized `CallToolResult`
      size at the raw result boundary.
- [x] Add shared Toolset receipt decorator with host-supplied timestamp and no clock.
- [x] Preserve stored `io.abird/receipt.ts` as immutable replay timestamp;
      fresh selected results require the host observation timestamp.
- [x] Add visible timestamp sidecar only when the exact `ts=` line is absent.
- [x] Preserve existing result metadata keys, structured JSON and typed media identity.
- [x] Add public generic complete-`CallToolResult` fitter in
      `aw-mcp-protocol`; `aw-mcp` re-exports it without duplicating semantics.
- [x] Fitter preserves metadata/result type/error class/block identity and
      atomically reserves receipt-sidecar overhead.
- [x] Only text is shrinkable; immutable media/structured facts that cannot fit
      return explicit `Limit` rather than being rewritten as text.
- [x] Add typed media/structured/replay/metadata/tight-budget/minimal-envelope tests.

### Phase P7 — MCP protocol heartbeat/progress adapter

- [x] Add optional token-owned `ProgressHeartbeat` +
      `await_with_progress_heartbeat`; server peer/request context is supplied
      by the product as a send closure. `aw-mcp-protocol` depends only on rmcp
      model types and timing; `aw-mcp` keeps the client transport stack separate.
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
      **255/255**, terminal **20/20**, strict affected Clippy, minimal configs,
      downstream host/CLI and full workspace all-feature check are green.
      Windows native assertion is authored but remains unexecuted on L5 because
      the Windows Rust std target is unavailable.
- [x] Initial signed P1-P8 implementation checkpoint exists at `eb325b31` and
      docs checkpoint at `61ed065d` per explicit user authorization.
- [x] Fresh-review implementation checkpoint `7c7de6c9...` plus downstream
      dependency-range follow-up `1a5d0f65208e4a05b737c804a55a3aaad3e2fa59`;
      the latter is the final dependency pin and supersedes `eb325b31`/`7c7de6c9`.
- [x] Publish final AW implementation pin
      `1a5d0f65208e4a05b737c804a55a3aaad3e2fa59`; docs/main are published
      through `4234447612421069a746c925c76a25e3ba27774a` and remote reachability
      was verified.
- [x] Dotlink companion repin completed atomically: direct `aw-tools`,
      `aw-tools-fs-unix`, `aw-tools-fs-win`, `aw-platform`,
      `aw-mcp-protocol` plus every transitive AW Git lock entry resolve to
      `1a5d0f65...`, with no registry package/version churn and no path override.
      Historical publish/repin mechanics remain recorded in
      [PUBLISH-REPIN-PREFLIGHT.md](PUBLISH-REPIN-PREFLIGHT.md).

### Phase P9 — dotlink thin consumer

Owned by dotlink companion plan.

Current sequencing: the original P9 thin-consumer convergence is complete.
Dotlink still consumes published AW
`1a5d0f65208e4a05b737c804a55a3aaad3e2fa59`; current local dotlink HEAD is
`20b6f045f79fa7725f286e05b40e9262b1c9e4d3`, four commits ahead of
`origin/main = 4c3b96fd5ba9e6772bc159f10a3ad893c9729887`. D4-D7 and the
v0.17.5 semantic-equivalence proof are closed and must not be reopened without
a reproduced regression. No temporary sibling/path dependency is allowed.

- [x] Consume shared stop/cancellation/process terminal semantics (D1-D2).
- [x] Consume shared Task failure detail (D3).
- [x] Consume shared Job refs/receipts/budgets (D4-D5).
- [x] Consume `aw-mcp-protocol` decorator/fitter/heartbeat (D6-D7); dotlink
      remains a thin consumer and does not pull in the `aw-mcp` client adapter.
- [x] Keep only product wire/log/config/transport policy after duplicate generic
      helpers were removed.
- [x] v0.17.x semantic-equivalence matrix through published v0.17.5 is complete.

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
- [ ] P2 native process-stop evidence gate — Unix `aw-platform-unix` **13/13** plus integration files **4/4** (1 cross-UID fixture intentionally ignored); current `aw-tools-adapter --all-features --lib` **255/255**; `aw-terminal --lib` **20/20**; strict affected Clippy and host-side `aw-platform-win` check green. Windows native Job Object test is authored but unexecuted on L5 because the Windows Rust std target is not installed.
- [x] P3 Job/Task/event propagation — full `aw-tools --all-features --lib` **348/348**; `aw-tools-adapter --all-features --lib` **255/255**; `aw-terminal --lib` **20/20**; strict core+adapter Clippy; no-default core; downstream `aw-mcp`/`aw-host`/`aw-cli` all-feature checks green.
- [x] P4 budget separation — full `aw-tools --all-features --lib` **350/350**; `aw-tools-adapter --all-features --lib` **256/256**; `aw-mcp --all-features --lib` **19/19**; strict core+adapter+MCP Clippy; no-default core; downstream `aw-host`/`aw-cli` all-feature checks; rustfmt and `git diff --check` green.
- [x] P5 refs/receipts/diagnostics — full `aw-tools --all-features --lib` **354/354**; `aw-tools-adapter --all-features --lib` **256/256**; `aw-mcp --all-features --lib` **19/19**; strict core+adapter+MCP Clippy; no-default core; downstream `aw-host`/`aw-cli` all-feature checks; rustfmt and `git diff --check` green.
- [x] P6 MCP metadata/fitter — `aw-mcp --all-features --lib` **24/24**; full `aw-tools` **354/354**; full adapter **256/256**; strict MCP+core+adapter Clippy; no-default core; downstream `aw-host`/`aw-cli`; rustfmt and `git diff --check` green.
- [x] P7 heartbeat/progress historical checkpoint — pre-extraction `aw-mcp` **33/33**; retained as historical evidence only.
- [x] Fresh-review projection checkpoint — `aw-tools` **357/357**, adapter **255/255**, `aw-mcp-protocol` **12/12**, `aw-mcp` **20/20**, Unix platform **13/13 + 4/4 PTY** (1 cross-UID fixture intentionally ignored), terminal **20/20**; minimal/no-default checks, host-side Windows check, downstream host/CLI and workspace all-feature check, strict Clippy for all touched crates, rustfmt and diff check are green. Protocol dependency-tree check proves it enables **no aw-tools feature** and leaks no Agent/runtime, reqwest, jsonschema, clap or MCP transport dependency.
- [ ] full AW shared-first convergence gate — all published 0.17 host-available AW technical gates are green and the final implementation pin is published; native Windows Job Object/exit-signal tests remain authored but unexecuted because L5 has no Windows Rust std target. This stays open only for that external native-Windows gate.
- [x] dotlink v0.17.x consumer/equivalence gate through the published v0.17.5 reference.

## Post-P9 runtime admission / Agent ownership hardening — 2026-11-20

Canonical current handoff:
[../../docs/AW-DOTLINK-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md](../../docs/AW-DOTLINK-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md).

The original P1-P9/D4-D7 work is historical baseline here. Do not interpret old
unchecked D4-D7/equivalence prose as an instruction to reimplement completed
dotlink convergence.

### Implemented locally, uncommitted

- [x] Schema v36 indexed runtime-admission projections:
      policy_admission_current and remote_start_current.
- [x] Policy replay lookup precedes mutable writer/policy/participation checks.
- [x] Session resource owner and initiating policy subject are distinct facts.
- [x] Shared-session participant routing requires the participant's current
      Submit + exact tool authority; owner privileges are never unioned.
- [x] Policy projection revisions are monotonic and audit remains immutable
      historical evidence.
- [x] Remote Start replay/counting no longer scans lifetime Session audit.
- [x] SourceEventBook is serializable/validated for durable Agent/Turn sources.
- [x] Durable agent_job_event_sources owns Agent/Turn rules, generations,
      event revisions and exact pending deliveries.
- [x] Agent root result events commit on newly attached Turns; exact Turn commits
      result+settled at one event revision; Agent root settled commits on close.
- [x] Durable agent_job_wake_admissions supports fresh-Router recovery while
      revalidating current participant/tool/capability generations.
- [x] Agent jobs_get(wait_ms) uses revision notification rather than the old
      10 ms store polling loop.
- [x] Focused replay/participant/source/restart/notification regressions green.
- [x] Relevant aw-store and AW lib/bin checks + focused strict Clippy + fmt/diff
      are green for the pre-existing v36 slice.
- [x] Explicit v35 -> v36 open regression reconstructs the v36 projections from
      real legacy evidence and preserves exact issuer-less v35 Remote Start replay.
- [x] AW Remote Start issuer identity is explicit:
      `RemoteStartIssuer { client_id, trust_epoch }` is target-authenticated,
      `RemoteStartLease` carries target+issuer, new v36 Start authorization uses
      an issuer-bound v2 digest, and issuer-less facts remain v1 replay-only.
- [x] Exact committed Remote Start recovery is issuer-independent: immutable
      attempt semantics recover the original authorization before any new
      issuer/lease is obtained. A changed semantic request fails closed.
- [x] Canonical Agent resolution uses indexed domain/identity/project/Agent lookup
      and then rechecks current Session authorization; hot resolve paths no longer
      enumerate every Agent binding.
- [x] Mixed Agent/finite Job listing uses the v36 `job_exposure_current`
      query-only projection plus keyset pagination under the existing position
      cursor contract. SessionStore/JobManager/commit_log remain the owners.
- [x] Historical P9/D4-D7/equivalence status above is reconciled with the live
      completed dotlink source-cutover/equivalence docs.
- [x] Current focused migration/replay/participant/source/restart/list regressions
      are green; `aw-store --lib` and `aw-cli --lib --bins` checks are green.
      Strict `-D warnings` Clippy is green for aw-tools/aw-mcp all targets,
      aw-store lib, and aw-cli all targets (`--no-deps`). Repeated aw-store
      all-target Clippy/check attempts were externally SIGKILLed by L5 without a
      compiler diagnostic and are not classified as source failures.
- [x] Dotlink post-equivalence process/control hardening is complete locally:
      ordinary-exit descendant cleanup, 250 ms post-exit output grace,
      escaped-`setsid` factual exit + incomplete output, shared-owner-only
      revision-fenced cancel, stdin authority linearization, and single MCP
      instruction ownership.
- [x] Dotlink final L5 gate is green at **703/0/1 + 42/42**, strict
      all-target/all-feature Clippy, fmt/diff, Alejandra and Markdown
      **0 issues / 86 files**.
- [x] Shared process projection now also keeps known exit/stop truth orthogonal
      from output completeness: `ProcessTerminal::status()` uses
      `Status.incomplete` instead of rewriting known exit to `Unknown`.
      Exact full-feature regression, aw-tools **359/359** and strict
      all-target/all-feature Clippy are green.

### Remaining

- [ ] Dotlink must adopt the published issuer-bearing AW wire after a future
      authorized AW publish/repin and enforce that only explicitly trusted OAuth
      bridge clients can mint a new issuer lease; do not add a temporary path
      dependency or duplicate the AW type locally.
- [ ] On that same authorized repin, remove dotlink's temporary
      `shared_process_status` published-pin compatibility shim and consume the
      corrected shared `ProcessTerminal::status()` directly.
- [ ] Native Windows process/Job Object qualification remains an external gate.
- [ ] Sandbox/config-v13 remains frozen pending explicit user promotion.
