# AW — live programmable harness

Author: Prasanna Venkateswaran Loganathar
Co-authors: ChatGPT GPT-6 Astra Pro
Owner: @chatgpt-aw-harness-design (plan coordination)
Date: 2026-11-28 (Asia/Singapore)
Base: `a47492f45813f1e85f85e706e7f33292f0c3e6f0`
Status: **Foundation-first plan: HP-1–3 establish the shared contracts and working SDK/MCP substrate; runtime implementation pending.**

## Goal

Make AW a live programmable agent harness and environment. A developer, user or
authorized agent can inspect, compose, revise, evaluate and activate behavior
through the same reusable native operations.

- **ZeroGravity:** live programmable inference.
- **AW:** live programmable harness, coordination and agentic environment.
- **Products above:** autonomous applications composed from those independent layers.

The user switched this work fully to AW; ZeroGravity is separately owned. This
planning change is confined to this directory and authored through L5. The reserved
UX plan and frozen physical Sandbox implementation remain outside this work.

## Start here

Read [IMPLEMENTATION.md](IMPLEMENTATION.md) for the phase map, dependencies,
milestones, Rust engineering rules and focused verification strategy.
Read [API_AND_PROTOCOLS.md](API_AND_PROTOCOLS.md) for canonical proposed public
contracts. All new APIs are target design, not claims about the shipped version.

| File | Contents |
|---|---|
| [DESIGN.md](DESIGN.md) | Product boundary, architecture, current source evidence and design rationale |
| [IMPLEMENTATION.md](IMPLEMENTATION.md) | Implementation order, cross-plan mapping, milestones and qualification |
| [FOUNDATIONS.md](FOUNDATIONS.md) | HP-1 public contracts/host; HP-2 composition/component interfaces; HP-3 integrated runtime and SDK/MCP foundation |
| [API_AND_PROTOCOLS.md](API_AND_PROTOCOLS.md) | Native contracts, methods, receipts/errors, AW control v2, MCP and Process Extension ABI 3 |
| [AUTHORING.md](AUTHORING.md) | HP-4 client/transport expansion; HP-5 richer context/strategies; HP-6 TS/JS extensions |
| [LEARNING_AND_DELIVERY.md](LEARNING_AND_DELIVERY.md) | HP-7 experiments; HP-8 improvement; HP-9 inference integration; HP-10 migration/delivery |
| [TODO.md](TODO.md) | One phase checklist and seven focused verification scenarios |

## Foundation first

The first three phases establish the whole shared boundary for the declared design.
They include all planned public operation/data contracts, component/host-port and
state/lifecycle interfaces, protocol/version rules, and a working baseline through
embedded Rust, attached Rust and a minimal MCP profile.

A default/custom native harness must work end to end through those public interfaces
before the foundation is complete. Later components consume the established contracts:
HP-4 clients/transports, HP-5 behaviors, HP-6 dynamic authoring, HP-7 experiments and
HP-9 richer providers can evolve independently. HP-8 improvement depends on HP-7.

Known optional APIs have compiled contracts and honest capability discovery; their
engines and algorithms can arrive later. No placeholder service or unsupported
implementation is advertised as working. Component versions and migrations stay
local unless their actual shared contract changes.

## Core decisions

1. Establish the full declared public contracts and a typed host Service from the
   current application composition, keeping the original Agent, TurnMachine,
   journals, Store, Input, Job and Process owners.
2. Represent a harness as immutable composition over existing context, pipeline,
   routing and registration references. Keep native prepared handles distinct
   from portable revision and preparation references.
3. Complete the planned atomic ExtensionRegistry root using the current leaf
   owners and publication primitive. Resolve once and retain exact custody.
4. Activate a desired binding through expected-binding CAS. The next eligible
   admitted Turn captures its exact revision; admitted pipeline/controller pins
   retain precedence. Permission revocation remains current.
5. Deliver the common embedded/attached Rust and explicitly started MCP path in
   the foundation; later client expansion uses those same semantic operations.
6. Use ordinary bounded Agents/Jobs/pipelines for comparisons and improvement.
   Promotion uses the same activation path, with independently configured policy.

The initial candidate lifecycle is deliberately small: edit a local definition,
prepare an immutable revision, inspect/diff it, then activate. There is no
separate persistent draft lifecycle. Execution and unknown-outcome recovery stay
with existing owners rather than a new general ledger.

Same-name tool rebinding waits for affected Turns, Jobs and continuations unless
distinct live registration identities have been implemented under the original
authority. Driver/Vault/physical-environment maintenance preserves its existing
quiescent compatibility boundary.

## Engineering priorities

Keep primitives simple, reusable and composable across Rust libraries. Native
calls use typed values and borrowed data; wire serialization stays at adapters.
Prefer concrete functions/enums/generics for fixed behavior and dynamic dispatch
only at actual runtime-selected extension/provider boundaries.

Prepare immutable composition outside hot execution paths. Reuse existing
publication handles, indexes, queues and bounded pages. Avoid per-token harness
metadata, global scans, duplicate validation engines and speculative caches.
Choose a clean high-performance implementation when available; otherwise keep
the design simple and optimize measured hot areas.

Verification concentrates on seven coherent scenarios in IMPLEMENTATION.md.
Reuse existing owner tests; add tests for concrete changed failure boundaries,
not a fixture/check/evidence artifact per field or accessor.

## Integration and status

HP-1 through HP-10 are local work packages, not additional global AW phases.
The plan maps into DL-200 Phases 4–6 and their existing foundations/qualification;
the coordinator's queue and ownership remain authoritative.

Source was inspected at Base. Product version is 0.15.0; actual Store source
reports schema v62. Some overview prose still says v33; allocate the next schema
number only after checking source at implementation time. Canonical Agent-facing
adaptation of existing Session-oriented routing/pipeline internals is explicit.

This turn changes planning documents only. No runtime/schema change, implementation
commit, push, release, deployment or physical-containment qualification is claimed.
Documentation checks are recorded in TODO.md; product tests remain unexecuted.
