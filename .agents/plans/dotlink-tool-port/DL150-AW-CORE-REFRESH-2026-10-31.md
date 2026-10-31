# AW core-tools convergence — historical 81a54ce checkpoint

Author: @pvl
Co-authors: ChatGPT GPT-6
Owner: @pvl
Status: Live AW implementation, uncommitted

## Current canonical source (updated review)

The fully sealed current spec is dotlink c34ea5a:
reference/DL150-SEALED-c34ea5a-README.md and
reference/DL150-SEALED-c34ea5a-TODO.md.
For current implementation status, confirmed fixes and remaining gates,
read PARITY-AUDIT-2026-10-31.md and NEXT-AGENT-HANDOFF-2026-10-31.md.
The following 81a54ce snapshot records a prior design checkpoint, not
the final authoritative spec.

## Authoritative complete specification

The **complete, unabridged** sealed dotlink design and roadmap, copied
read-only from dotlink commit 81a54ce, are:

- reference/DL150-SEALED-81a54ce-README.md
- reference/DL150-SEALED-81a54ce-TODO.md

Read both first. This AW delta is not a competing or abbreviated spec.
If dotlink updates the design, diff new commits and snapshot them
explicitly before making new model-facing choices.

## Architecture

One original AW Agent/ToolRegistry/ToolPolicy/TurnJournal and Process
supervisor. aw-tools-core owns transport-neutral synchronous engine,
schemas, progress, results, planning and bounded execution state.
aw-tools is a thin AW authority/journal/transport adapter. AW platform
backends own pinned/no-follow handles, secure path traversal, cross-
process mutation locks, staging, and native syscalls. Extract reusable
OS-backend interfaces, not a weaker second filesystem implementation.

Cargo features select compiled availability, not model grants:
read-only, rw, full; optional read/write/binary/lifecycle/async/durable/
sandbox/batch. Runtime intersects actor, source, epoch, site, tool
effect and actual prepared interpreter. Shell writes are advisory hints.
Old CLI spellings parse at the adapter boundary; SQLite retains its
original constrained child-session labels.

## Sealed deltas relative to the AW a156142 integration

- Outcomes: ok, unchanged, fail, mixed, unknown, skip, not
  partial/uncertain. More and incomplete are separate modifiers.
  Mixed aggregates may contain unknown outcomes.
- Graph predicates N.* means any terminal outcome; remove .err/.done.
  Require exact N.mixed/N.unknown for ambiguous effects.
- write and cp create missing parent suffixes only inside one atomic
  staged namespace publication; failed work creates no visible parents.
- mv requires the destination parent already exist. Rename-or-no-
  change; never mkdir or copy/delete fallback.
- rm preflights duplicates, aliases and ancestor/descendant roots.
- batch(ops) and job_start(ops) share an immutable Graph grammar;
  batch optimistically waits, job_start exposes Job immediately.
- Canonical Job(Graph[Node]) unifies shell, native batch, job_* control,
  Task projection and one ID namespace. Journal-claim before executing
  a node and terminal-checkpoint before dispatching dependents.
  Resume/reattach is never restart; restart explicit and replay fenced.
- Runtime-session jobs do not promise process-crash resurrection.
  Cross-process request replay uses a minimal bounded protected
  tombstone/claim, not a second persisted graph.
- Three distinct prepared interpreters bash, pwsh, powershell. No
  implicit fallback or alias between interpreter capabilities.
- Status, paging and progress are canonical; exact data and status
  need distinct typed blocks when the transport supports them.
- Model-visible batch/jobs remain OFF until original AW host provides
  per-node journal claims, replay-safe identity, revocation and
  retention; do not add a second execution runtime.

## Implementation order

A. Update v1 core status/DSL/shell and strict contracts with tests.
B. Extract shared filesystem backend contract, reuse original secure
   Unix and Windows handles and staged mutation layers.
C. Implement missing-parent atomic staging, mv no-mkdir, rm alias
   preflight, failure-injection and race tests.
D. Separate exact read data from control/status metadata in a
   backwards-compatible AW tool-result representation.
E. Build pure reusable Job/Graph/Node checkpoint semantics, then bind
   to original AW TurnJournal/Process before model registration.
F. Run feature, Agent/CLI, no_std, Windows GNU, native-runtime and
   replay/revocation gates. Record actual evidence.

No staging, commit, push, tag or changes to dotlink/reserved UX.
