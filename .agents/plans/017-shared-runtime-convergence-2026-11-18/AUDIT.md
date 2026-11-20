# Audit and ownership map

Parent: [README.md](README.md)

Current authoritative release reference:
`release/0.17 = origin/release/0.17 = v0.17.5 =`
`3995448d7bf33794ef84c7bb51150d77cc260ada`.
Historical v0.17.4 baseline: `fd1cdb6429a341a4e3d27805cae0bec288fc26df`;
`8874013` is the v0.17.4 post-tag signal-label test cleanup.

## Shared-first capability map

| # | Capability | Owner | Phase | Status |
|---|---|---|---|---|
| 1 | Physical `Code/Signal(n)/Unknown` | AW core | P0 | done in `796225e` |
| 2 | Minimal stop reason/evidence/record | AW core | P1 | final minimal API implemented in working tree; provenance moved to higher-layer hooks/context |
| 3 | Terminal stop attribution rule | AW core | P1 | final `StopRecord::terminal_reason()` implemented and green in working tree |
| 4 | Evidence-preserving managed process-tree guard | AW platform/adapter | P2 | implemented; Unix native gate green, authored Windows native gate pending Windows target |
| 5 | Stop evidence in `ProcessTerminal` / JobManager | AW core | P3 | implemented and green in working tree |
| 6 | Typed Task terminal/failure detail | AW core | P3 | compact `{exit, stop}` projection implemented and green |
| 7 | Execution lifecycle stop sequencing/events | AW core/adapter | P3 | shared zero-cost `StopTracker`, attempt-level `ProcessStop` sink and final factual `ProcessReceipt::stop_record()`; product logging remains host-owned |
| 8 | Generic sandbox mount semantics | AW sandbox | S | **deferred** |
| 9 | Linux pinned generic mount/Bubblewrap binding | AW sandbox_linux | S | **deferred** |
| 10 | Page vs serialized response budget types | AW core + adapter + aw-mcp-protocol | P4 | implemented and green; complete MCP fitter remains P6 |
| 11 | Complete Job handoff/control receipts | AW receipt | P5 | typed JobStatus/NodeStatus/Handoff/Cancel/Ack grammar; revocation distinct from cancellation and `ProcessTerminal.stop` retained |
| 12 | Typed bounded flat Job refs + diagnostics | AW execution_ref/Job API | P5 | `JobRef` + shared bounded diagnostics implemented and green |
| 13 | MCP receipt metadata decorator | aw-mcp-protocol + feature-neutral receipt policy | P6 | copyable `ReceiptProfile`, zero aw-tools features, no full-result clone for sidecar reservation; bounded `_meta`, replay-stable timestamp and full-budget visible-marker preservation |
| 14 | MCP serialized result fitter | aw-mcp-protocol | P6 | text-only fitting with mandatory marker; immutable typed/structured facts fail explicitly; `aw-mcp` re-exports |
| 15 | MCP heartbeat/progress adapter | aw-mcp-protocol | P7 | no client/server transport dependency; token-gated no-total heartbeat, concurrent bounded delivery, RAII summary observer, semantic progress adapter |
| 16 | Signal/exit platform facts | aw-platform | P2 | `signal_name` plus allocation-free `exit_signal(&ExitStatus)` implemented; AW adapter consumes the same exit-signal helper; Unix green, Windows projections are correctly None with native assertion authored |
| 17 | Cross-repo shared checkpoint | AW + dotlink plan boundary | P8 | fresh-review source `7c7de6c9...` plus downstream-range follow-up `1a5d0f65...`; final pin is `1a5d0f65...`; native Windows test remains external |

## v0.17.x release disposition

### v0.17.0 / v0.17.1

Already satisfied or superseded by current shared AW/dotlink architecture:

- `90169e3` response-window wording/invariant;
- `6049791` explicit ACK cleanup;
- `c0dcdf7` progress continuations;
- `ee7e51e` both-stream cleanup;
- `3cea39a` ACK receipt;
- `7aa0c44` `--no-tunnel`;
- `f76a19a` compact Job payloads;
- `cf7586d` `utf8_lossy`;
- `26c5368` strict argument surfaces;
- `e61a3c3` AW registry migration target;
- `77add69` root schema check;
- `07ca25f` receipt count units.

Revalidate; do not copy release-local implementations.

### v0.17.2 / v0.17.3

Generic mount/config-v13 bundle:

- `21de8b3`
- `e7db299`
- `349a7d0`
- `cb7418a`
- `c71f426`
- `6f48703`
- `ba947c6`
- `ae4d623`
- `d855672`
- `0983d39`

Disposition: **designed in shared AW sandbox sibling plan; implementation
deferred until explicit user promotion.**

Release/version commits `e74d753` and `acb4044` are bookkeeping only.

### v0.17.4 canonical stop/provenance

`c185558` is the final implementation authority. Its portable semantics split
as follows:

AW-owned:

- exact signal identity;
- minimal semantic process stop reason;
- `StopEvidence::{Stopped,NoTarget,Unknown}` and `StopRecord`;
- evidence-preserving Guard outcome;
- retained process terminal stop fact;
- typed failed-Task terminal detail;
- execution lifecycle event value/sink;
- signal-name utility;
- generic Job ref/receipt/budget/result/progress projection infrastructure.

Explicit caller/parent/runtime/authority provenance and cleanup/Drop path identity are
higher-layer observer/product context, not canonical AW process-stop state.

Dotlink-owned:

- exact `stopped_by` compatibility spelling;
- outside-dotlink prose;
- exact `LOG job stop ...` text and logging sink;
- transport response windows;
- OpenAI outer replay ledger.

### v0.17.4 hardening commits

`74820ff` — caller-controlled Job diagnostics:

- P5 implements AW typed bounded flat `JobRef` plus shared UTF-8-safe bounded diagnostics;
- finite AW receipts no longer accept raw ref strings;
- dotlink retains only transport-boundary defense in depth.

`06d5548` — page vs response envelope:

- old `job_output` name is superseded;
- the same semantic bug applied to current `jobs_read`;
- P4 now separates raw `JobProfile` page size from authenticated serialized
  result ceilings and carries them through distinct zero-cost types.

`30099aa` — Nix environment ownership:

- dotlink-only deployment hardening;
- port after shared runtime repin;
- unrelated to AW runtime semantics.

`5b27784` — runtime-health/backport docs:

- dotlink-only operational documentation;
- adapt to current `jobs_*`, shared receipts and 30-minute guidance.

`fd1cdb6` — release bookkeeping only; never port as behavior.

## Things intentionally staying dotlink-owned

- profile schema/versioning and environment/CLI precedence;
- NixOS module/systemd credentials;
- exact wire wording and signal explanation prose;
- exact operator log format;
- OpenAI/HTTP/stdio delivery windows;
- outer OpenAI request replay body/lease/tombstone ledger;
- runtime-health deployment baselines;
- release/tag/version actions.

## Deferred sandbox ownership map

Designed but frozen:

AW:

- generic mount spec/access/required-vs-optional semantics;
- target-layer merge;
- target/env/nesting/host-conflict validation;
- pinned source capabilities;
- Bubblewrap RO/RW/env bind planning.

Dotlink:

- profile `mounts`;
- `DOTLINK_MOUNTS`;
- `--mount`;
- developer-cache discovery;
- config v13 migration;
- dotlink sandbox layout and host-integration selection.

See sibling sandbox plan.
