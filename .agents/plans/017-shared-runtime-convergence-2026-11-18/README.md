# 0.17 shared runtime convergence

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-5.6 Sol
Owner: @pvl

Shared-first convergence of the reusable execution/runtime semantics exposed by
dotlink's finalized 0.17.x lane into AW. The goal is not release-line code
parity. The goal is that AW owns portable process, Job, Task, receipt, budget
and MCP semantics, while dotlink becomes a thin host/transport/deployment
consumer.

Authoritative dotlink reference:

- `release/0.17 = v0.17.4 = fd1cdb6429a341a4e3d27805cae0bec288fc26df`
- canonical v0.17.4 stop/provenance implementation: `c185558`
- v0.17.4 hardening: `74820ff`, `06d5548`, `30099aa`, `5b27784`

AW base for this master plan is the signed local checkpoint:

- `796225e9b50e824606586859d7bcc88722b4919e`
  `Separate process signal identity from lifecycle`

That checkpoint is the completed first slice: physical process exit is now
`Code(i32) | Signal(i32) | Unknown`.

Detailed API/ownership design: [DESIGN.md](DESIGN.md)
Release/shared mapping: [AUDIT.md](AUDIT.md)
Implementation phases: [TODO.md](TODO.md)

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
2. typed cancellation origin;
3. typed stop cause/attempt/outcome;
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

### Stop attempt and terminal claim are different facts

A runtime can attempt a stop without proving that it stopped the waited
command. Cleanup and fail-safe Drop are also important forensic events but do
not automatically justify a terminal stop claim.

Therefore AW owns both:

- typed stop-attempt evidence;
- optional claimable stop cause retained on the process terminal.

### Cancellation origin is portable

Caller cancellation, runtime shutdown and authority withdrawal have different
meaning. AW owns the typed origin. Dotlink may collapse richer AW origins into
its v0.17.4 compatibility spellings `cancel` and `shutdown`.

### Execution lifecycle events are not Job wake events

The existing `job_event` module remains for Result/Settled/Attention and
Agent wake semantics. Process stop attempts are lower-level execution evidence
and use a separate optional lifecycle-event sink.

### Task failures retain factual terminal detail

A later internal/spool/projection failure must not erase an already observed
process exit, stop cause or prior deadline. AW Task state owns typed terminal
detail; MCP transports project it.

### Page and response budgets are different types

A one-byte raw page must not imply a one-byte serialized MCP envelope. AW owns
the semantic page budget; AW MCP owns the serialized result envelope.

### Receipts are typed shared facts

Handoff, ACK/Finish, cancellation and Job status receipts are shared durable
execution semantics. Hosts provide facts; AW renders one bounded grammar.

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
  `ExecutionRef`, `Receipt`, `Toolset` and `aw-mcp`; do not create
  parallel registries.
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
