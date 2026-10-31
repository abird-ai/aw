# aw-tools-core

Transport-neutral tool algorithms extracted from AW and dotlink, ready for
future single-source reuse. The default build is **synchronous and read-only**: no Tokio,
Agent, MCP server, provider, persistent task database, shell, or filesystem
permission grant is needed. AW explicitly enables the async and durable
interfaces; a future dotlink consumer may use sync alone and selectively
enable the other features.

## Feature switches and authority

| Cargo feature | Default | Compiled surface |
| --- | --- | --- |
| read-only | yes | Common filesystem metadata, discovery and binary/text reads |
| rw | no | read-only plus write planning, patch, lifecycle cp/mv/rm |
| full | no | rw plus opt-in async, durable task and sandbox modules |
| read / write / binary / lifecycle | indirect | Fine-grained compiling of tool families |
| async | no | Tokio adapter only, no second execution runtime |
| durable | no | Task fencing, spool, checkpoints and retention; host journal remains owner |
| sandbox | no | Host-selected isolation plan, no model shell grant |

These are build features ONLY. The runtime host separately authenticates
its effective Off/ReadOnly/ReadWrite/Full permission ceiling and the
original source/site/actor/epoch/effect ToolPolicy and journal. Compiling
full is never a grant to run shell or to access paths.

AW enables rw, async and durable in aw-tools. Its original platform backend
enforces pinned descriptor traversal, cross-process mutation coordination,
staged files, and no-replace publication. The same backend must be reused
or absorbed into this engine before dotlink switches its native tools;
duplicating platform security decisions would defeat this architecture.



## DL-150 current read/edit/status contract (2026-10-31)

A canonical filesystem schema registry provides 13 orthogonal tools,
including edit(path,edits=[[old,new,all?],...]) with original-snapshot
matching, overlap rejection and one atomic publication. No old aliases or
transport-only names appear in core schemas. AW's adapter may accept
legacy edits[], read.offset/limit and tool-mode aliases solely at the
wire boundary. Shared read::page consumes an already-opened BufRead,
requires a hard operator-supplied byte budget, validates skipped UTF-8,
and produces exact line/byte cursors through long Unicode source lines.
Mid-read I/O faults carry valid data as an explicitly incomplete/error
page. When status and exact data require independent content blocks,
AW currently encodes partial unnumbered results in JSON because the
existing Agent ToolOutput remains text-only.

Shared lifecycle::RemoveOutcome preserves caller input order and
distinguishes complete, known partial and committed cleanup-pending
namespace mutations. Platform backends enforce no-follow identities,
cooperative mutation locks, private staged copies and tombstones.
Build features never widen the original AW Actor/ToolPolicy grant.

The sealed batch graph compiler is available under the opt-in batch/full
feature: compact 1-based tuples, status predicates, backward-only
references and bounded nested-argument validation. It does not execute
effects or register a model-visible batch. That requires replay-safe
per-node journal identities in AW's original TurnJournal. Full current implementation evidence and outstanding platform
runtime gates are in the owned AI-NATIVE-FS-IMPLEMENTATION-2026-10-31.md.


## Final sealed DL-150 v1 and V2 boundary

Canonical public grep has one symmetric context field; patch is strict
and cp(from,to) copies complete directory trees without a public recursive
knob. Discovery query markers use more offset=N and incomplete reasons,
which may coexist; read paging uses more line=N byte=M. The shared
status module separates primary outcome from paging/coverage modifiers.
The model can provide advisory shell writes scopes, normalized and
checked by the shared filesystem layer; hints are never grants.

AW's existing CLI/UI and durable child-store use separate naming codecs:
rw/full are user-facing, but existing child SQLite rows remain workspace/
trusted as originally constrained. Future dotlink reuse must provide
its own exact authorization/journal/transport adapter, and may not
silently remove AW's platform filesystem guarantees.


### Verification and shared design checkpoint

Original AW Cargo.lock validation: 41 read-only, 64 rw and 120
full-feature core tests passed. Strict Clippy passes. The pure
batch graph compiler remains opt-in and non-executing; it has
one compact nested-schema-free model definition and no unjournaled
mutation path. Shared status and progress modules supply canonical
outcome/modifier formatting and cumulative known/unknown snapshots.
Separate secured filesystem backend extraction is still required
before replacing dotlink's complete native tool implementation.


### Batch predicate safety and durable-job v2 boundary

With the latest dotlink sealed design (8a3e7bb), .err means only a
known fail or partial result. A genuinely uncertain side effect must
be matched explicitly with .uncertain. The pure batch compiler now
enforces this distinction. V2's shared durable job namespace, shell
runner reattachment and explicit replay-protected job_restart remain
host integrations, not an independent executor inside aw-tools-core.


Sealed dotlink update a156142 defines V2 as canonical Job(Graph[Node])
rather than independent shell/batch durable runners. The shared batch
compiler stays pure and V1-restricted to native filesystem tools;
AW's existing journal/Process host must own real Job execution and
recovery. The Windows model-visible standalone shell name is pwsh.

## Current shared engine boundary — 2026-10-31

The shared filesystem operations module owns canonical write, snapshot
edit, copy, move and remove. Each operation acquires the original
SecuredFilesystem lock and delegates publication to the one pinned
Unix/Windows backend; AW no longer duplicates lifecycle orchestration.
Legacy conditional-write preimages retain their original adapter path.

Pure Job(Graph) state supports original-journal transition replay
without executing effects, immutable graph and exact actor/source/
epoch/site/session/branch/turn/call/effect bindings, and checkpoint
before runner start and terminal before dependent dispatch. A claimed
node cannot be known successful without Running; Claimed -> Skip
requires a trusted no-launch proof and a separate journal record.
No model-facing JobManager or second TurnJournal has been created.

Current complete original-lock tests and open host integrations:
owned plan CORE-ENGINE-RESUME-2026-10-31.md.
