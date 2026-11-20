# 0.17 shared runtime convergence

Authority-gated publish/repin procedure:
[PUBLISH-REPIN-PREFLIGHT.md](PUBLISH-REPIN-PREFLIGHT.md).

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-5.6 Sol
Owner: @pvl

Shared-first convergence of the reusable execution/runtime semantics exposed by
dotlink's finalized 0.17.x lane into AW. The goal is not release-line code
parity. The goal is that AW owns portable process, Job, Task, receipt, budget
and MCP semantics, while dotlink becomes a thin host/transport/deployment
consumer.

Authoritative current dotlink release reference:

- `release/0.17 = origin/release/0.17 = v0.17.5 =`
  `3995448d7bf33794ef84c7bb51150d77cc260ada`;
- `v0.17.4 = fd1cdb6429a341a4e3d27805cae0bec288fc26df`;
- canonical v0.17.4 stop/provenance implementation: `c185558`;
- v0.17.4 hardening: `74820ff`, `06d5548`, `30099aa`, `5b27784`;
- `8874013` delegates signal labels to the platform helper;
- v0.17.5 adds outside-signal lifecycle projection `f9537bb` and hard execution
  deadline wording `a1f8505`.

AW base for this master plan is the signed local checkpoint:

- `796225e9b50e824606586859d7bcc88722b4919e`
  `Separate process signal identity from lifecycle`

That checkpoint is the completed first slice: physical process exit is now
`Code(i32) | Signal(i32) | Unknown`.

Detailed API/ownership design: [DESIGN.md](DESIGN.md)
Release/shared mapping: [AUDIT.md](AUDIT.md)
Implementation phases: [TODO.md](TODO.md)
Current comprehensive handoff: [../../docs/AW-DOTLINK-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md](../../docs/AW-DOTLINK-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md)
Current next-agent entrypoint: [../../docs/AW-DOTLINK-NEXT-AGENT-2026-11-20.md](../../docs/AW-DOTLINK-NEXT-AGENT-2026-11-20.md)
The 2026-11-19 and 2026-11-18 handoffs remain available for historical chronology.

## Goal

For every portable fact, AW should be the one canonical semantic owner. A host
may produce evidence and a transport may project it, but neither should invent a
parallel lifecycle model.

Target architecture:

```text
OS / host evidence
        |
        v
AW platform/process primitives
        |
        v
AW typed Process / Job / Task facts
        |
        +--> AW receipts / MCP projection
        |
        +--> optional lifecycle event sink
        |
        v
dotlink
  config + transport + wire wording + operator log + deployment
```

Dotlink should not permanently own a second stop model, Task failure model,
Job receipt grammar, result-budget model, MCP receipt decorator or progress
heartbeat implementation.

## Ownership rule

Move a concept into AW when all are true:

1. it is meaningful for more than one execution host;
2. another AW host would otherwise have to rediscover the same invariant;
3. it can be represented without importing dotlink transport/deployment policy;
4. the type can remain factual and authority-neutral.

Keep a concept in dotlink when it is specifically:

- OpenAI/HTTP/stdio delivery policy;
- dotlink profile/CLI/environment syntax;
- NixOS/systemd deployment;
- dotlink compatibility wording;
- operator logging destination/format;
- release/version bookkeeping.

## Active shared-first scope

The active non-sandbox scope is:

1. physical exit identity;
2. one minimal semantic stop reason;
3. `StopEvidence::{Stopped,NoTarget,Unknown}` + `StopRecord` with optional reason;
4. evidence-preserving cross-platform managed process-tree stop primitive;
5. stop evidence retained in `ProcessTerminal` and canonical Job state;
6. typed Task terminal/failure detail;
7. typed execution lifecycle event sink;
8. raw page budget vs serialized response-envelope budget;
9. complete shared Job handoff/control receipt grammar;
10. canonical bounded flat Job/Node refs and bounded diagnostics;
11. shared MCP receipt metadata decoration;
12. shared serialized MCP result fitting;
13. shared MCP heartbeat/progress adapter;
14. shared signal-name projection utility;
15. dotlink thin-consumer conversion and v0.17.4 equivalence.

## Explicitly deferred sandbox scope

Generic mount/sandbox convergence is designed, but **must not be implemented
until the user explicitly promotes/pulls that plan**.

The frozen sibling plan is:

- [017-sandbox-mount-convergence-2026-11-18](../017-sandbox-mount-convergence-2026-11-18/README.md)

No active phase in this master plan may change:

- `aw-tools::sandbox` mount APIs;
- `aw-tools::sandbox_linux` bind planning for generic mounts;
- dotlink config v13;
- `DOTLINK_MOUNTS`;
- dotlink `--mount`;
- developer-cache-to-generic-mount convergence.

Existing sandbox code may be read as context while implementing process
provenance, but its behavior is frozen for this effort.

## Core decisions

### Physical exit and lifecycle remain orthogonal

`ProcessExit` is physical evidence only:

```rust
Code(i32)
Signal(i32)
Unknown
```

Cancellation, deadline, revocation and failure are not exit shapes.

### Stop record retains only shared process semantics

A runtime can attempt a stop without proving that it stopped the waited
command. The shared record therefore separates semantic intent from platform
evidence:

```rust
StopRecord {
    reason: Option<StopReason>,
    evidence: StopEvidence,
}
```

`StopReason` contains only terminal-capable semantic causes: Timeout, Cancelled
and RuntimeError. `reason: None` means housekeeping with no semantic terminal
cause. Explicit cleanup, Drop fallback, caller/parent/runtime/authority
provenance and product wording are higher-layer observer context, not canonical
process state.

Only `Some(reason) + Stopped` contributes to the optional terminal stop reason.
Shared zero-cost `StopTracker` retains the existing semantic reason across an
Unknown retry; a later Drop retry does not relabel Cancelled as "Dropped".

`NoTarget` is positive knowledge that no live target remains; `Unknown` is
unresolved and the target may still be alive. They remain distinct because the
former disarms a Guard while the latter may require a Drop retry.

## Latest design correction — P1 vocabulary

The committed `021b8ce` P1 checkpoint was an invariant-proving transitional
checkpoint, **not the final public API**. The final 2026-11-18 P1 refinement
uses exactly three stop concepts:

- `StopReason::{Timeout,Cancelled,RuntimeError}`;
- `StopEvidence::{Stopped,NoTarget,Unknown}`;
- `StopRecord { reason: Option<StopReason>, evidence }`.

The obsolete `CancellationOrigin / StopOrigin / ProcessStopCause /
StopAttemptReason / StopOutcome / StopAttempt` public names are removed.
Cleanup and Drop are execution paths, not stop reasons. P2/P3 now build on
this minimal vocabulary and expose richer provenance through optional hooks
rather than growing canonical state speculatively.

### Execution lifecycle events are not Job wake events

The existing `job_event` module remains for Result/Settled/Attention and
Agent wake semantics. Process stop attempts are lower-level execution evidence
and use a separate optional lifecycle-event sink. The adapter also retains one
final factual `ProcessReceipt::stop_record()` for the logical stop sequence;
products use that for one final projection rather than treating each attempt
event as a terminal result.

### Task failures retain factual terminal detail

A later internal/spool/projection failure must not erase an already observed
process exit, stop cause or prior deadline. AW Task state owns typed terminal
detail; MCP transports project it.

### Page and response budgets are different types

A one-byte raw page must not imply a one-byte serialized MCP envelope. AW owns
the semantic page budget; `aw-mcp-protocol` owns generic serialized result
projection.

### Receipts and finite Job refs are typed shared facts

P5 implements bounded `JobRef` values for the finite model/product surface:
`j:<handle>[/n:<coordinate>]`. Hierarchical `ExecutionRef` remains the internal
ownership/Agent-routing identity.

Handoff, ACK/Finish, cancellation and Job/node status receipts consume typed
`JobRef` values and are shared durable execution semantics. Hosts provide
facts; AW renders one bounded grammar. `render_handoff` may reduce a rich
status to the same Job + `no-rerun` continuation under a tighter envelope, but
never changes execution identity or result semantics.

### MCP receipt metadata and fitting preserve result identity

P6 keeps raw client `McpToolResult` ownership in `aw-mcp`, while generic
result/receipt projection lives in lightweight `aw-mcp-protocol`. Shared receipt
time is stored under `io.abird/receipt.ts`; hosts supply the fresh timestamp,
an already stored timestamp wins on replay, and the exact visible `ts=` line is
added only when absent. `aw-mcp` re-exports the protocol helpers for AW clients.

The complete-result fitter measures actual serialized `CallToolResult` bytes.
Only text may shrink. Structured JSON, typed media/resources, result metadata,
error class, result type, block order and block metadata remain unchanged; if
those immutable facts cannot fit, fitting fails explicitly.

### MCP progress is advisory and cannot own completion

P7 adds token-gated `ProgressHeartbeat` /
`await_with_progress_heartbeat` plus bounded semantic-progress projection.
A heartbeat emits only monotonic unknown-total ticks; it never invents a total,
message or final completion state.

The product supplies its rmcp request peer as a send closure. The shared
`aw-mcp-protocol` crate has no client/server transport feature, enables no
`aw-tools` feature, and therefore adds no Agent, HTTP client or MCP transport
stack to server consumers. Receipt selection is a feature-neutral copyable
`ReceiptProfile`, not a Toolset/catalog dependency. Each
notification send is independently bounded, and the operation future remains
polled while delivery is pending, so progress backpressure cannot delay
completion. No detached task/queue exists. An optional RAII observer receives
only attempted/delivered counts, including when the wrapper itself is
cancelled.

Real semantic progress remains owned by the transport-independent AW progress
type and is projected through its existing known/unknown total semantics.

### Transport replay remains product-owned for now

AW already owns semantic Job replay, output ownership and Remote Start
authorization. Dotlink's outer OpenAI request replay ledger still owns hosted
response bodies, HTTP delivery leases and tunnel horizons. Do not move it into
AW in this effort.

## Performance and maintenance constraints

- Prefer `Copy` enums and zero-cost newtypes for hot semantic values.
- Keep clocks, I/O, logging and transport allocation out of `aw-tools` core.
- Optional event sinks must be no-op when absent.
- Do not add heap allocation merely to classify a process stop.
- Reuse existing `JobManager`, `ProcessTerminal`, `TaskDisposition`,
  `ExecutionRef`, `Receipt`, `Toolset`, `aw-mcp-protocol` and the `aw-mcp`
  client adapter; do not create parallel registries or projection engines.
- Preserve no_std/alloc boundaries where existing modules support them.
- Platform proof belongs in `aw-platform*`/std adapters, not the pure kernel.
- Product prose and logging strings remain outside shared semantic types.

## References

- dotlink `v0.17.4 @ fd1cdb6`
- dotlink `c185558`: final stop/signal/cause/log implementation
- dotlink `74820ff`: caller-controlled diagnostic bounding
- dotlink `06d5548`: page bound vs response-envelope separation
- dotlink `30099aa`: Nix environment ownership
- dotlink `5b27784`: final runtime-health/backport documentation
- AW `796225e`: physical signal identity/lifecycle separation
- sibling deferred sandbox plan linked above
