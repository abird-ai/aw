# Audit and ownership map

Parent: [README.md](README.md)

Authoritative release: dotlink `v0.17.4 @ fd1cdb6`.

## Shared-first capability map

| # | Capability | Owner | Phase | Status |
|---|---|---|---|---|
| 1 | Physical `Code/Signal(n)/Unknown` | AW core | P0 | done in `796225e` |
| 2 | Cancellation origin | AW core | P1 | open |
| 3 | Stop cause/attempt/outcome | AW core | P1 | open |
| 4 | Evidence-preserving managed process-tree guard | AW platform/adapter | P2 | open |
| 5 | Stop evidence in `ProcessTerminal` / JobManager | AW core | P3 | open |
| 6 | Typed Task terminal/failure detail | AW core | P3 | open |
| 7 | Execution lifecycle event sink | AW core/adapter | P3 | open |
| 8 | Generic sandbox mount semantics | AW sandbox | S | **deferred** |
| 9 | Linux pinned generic mount/Bubblewrap binding | AW sandbox_linux | S | **deferred** |
| 10 | Page vs serialized response budget types | AW core + aw-mcp | P4 | open |
| 11 | Complete Job handoff/control receipts | AW receipt | P5 | open |
| 12 | Typed bounded flat Job refs + diagnostics | AW execution_ref/Job API | P5 | open |
| 13 | MCP receipt metadata decorator | aw-mcp | P6 | open |
| 14 | MCP serialized result fitter | aw-mcp | P6 | open |
| 15 | MCP heartbeat/progress adapter | aw-mcp | P7 | open |
| 16 | Signal-name utility | aw-platform | P2/P7 | open |

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
- cancellation origin;
- stop cause/attempt/outcome;
- evidence-preserving Guard outcome;
- retained process terminal stop fact;
- typed failed-Task terminal detail;
- lifecycle event value/sink;
- signal-name utility;
- generic result/receipt projection infrastructure.

Dotlink-owned:

- exact `stopped_by` compatibility spelling;
- outside-dotlink prose;
- exact `LOG job stop ...` text and logging sink;
- transport response windows;
- OpenAI outer replay ledger.

### v0.17.4 hardening commits

`74820ff` — caller-controlled Job diagnostics:

- primary fix: AW typed bounded flat Job refs and shared bounded diagnostics;
- dotlink retains transport-boundary defense in depth.

`06d5548` — page vs response envelope:

- old `job_output` name is superseded;
- semantic bug remains relevant to current `jobs_read`;
- implement shared raw-page vs serialized-result budget types and consume them.

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
