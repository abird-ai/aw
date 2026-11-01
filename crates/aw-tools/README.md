# aw-tools

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
host integrations, not an independent executor inside aw-tools.


Sealed dotlink update a156142 defines V2 as canonical Job(Graph[Node])
rather than independent shell/batch durable runners. The shared Graph
compiler stays pure/non-executing and now carries transport-neutral
`NodeTool` identities for native filesystem tools plus host-resolved
bash/pwsh/powershell nodes. Interpreter preparation, authority and real
process execution remain host responsibilities. AW binds these nodes to its
existing journal/`PipeProcess` owner; the Windows shell names remain exact
`pwsh` or `powershell`, never an alias/fallback inferred by the shared crate.


## Differential consumer conformance

`aw-tools` exposes a test-only `conformance` feature for temporary differential
consumers such as dotlink during cutover. It enables only the pure Graph compiler
and host-neutral serde vectors; it does **not** grant authority, open files, start
processes, enable durable jobs, or depend on AW Agent types.

The fixture covers the canonical 13 filesystem requests, exact schema projection,
Graph acceptance/rejection and condition semantics, primary outcome aggregation,
and paging/status markers. Consumers should call `aw_tools::conformance::validate()`
before comparing product behavior, then use `fixture()`, `schemas(...)`, or the
individual vector functions. Filesystem/security differential execution remains a
host test responsibility because only the host owns prepared roots and authority.

A standalone consumer must pin the AW Git revision rather than commit a sibling
path dependency. After the reviewed implementation commit exists, the intended
dev/test dependency shape is:

```toml
[dev-dependencies]
aw-tools = { git = "https://github.com/abird-ai/aw", rev = "<PINNED_AW_REV>", default-features = false, features = ["read-only", "conformance"] }
aw-tools-fs-unix = { git = "https://github.com/abird-ai/aw", rev = "<PINNED_AW_REV>" }
```

The same revision must be used for `aw-tools` and every secured backend. Local
path patching is acceptable only as an uncommitted differential-development aid.

For multi-root hosts, the secured platform crates expose matching
`scoped_fs::capability` APIs. `CapabilitySet` accepts already-admitted
`GrantSpec`/`PreparedGrant` roots, independent read/write `ExclusionSpec` holes,
and host-owned private-artifact registries. It implements the same
`SecuredFilesystem` trait as a single `Root`, including cross-root copy/move/remove,
absolute or base-relative model paths, final-symlink target re-resolution, and
startup private-artifact reconciliation. Product policy remains outside the shared
crate: the host must supply both lexical/resolved exclusions and must reauthorize
a resolved final-symlink model path before payload access.

`CapabilitySet` deliberately does not model an unrestricted host policy. Unix
unrestricted hosts can prepare `/`; Windows unrestricted hosts should prepare the
drive roots involved in each already-authorized request. Windows UNC/network-share model paths use the canonical `//server/share/...`
spelling and route through the same exact prepared-grant model. This path is covered
by shared grammar tests and Windows cross-compilation; native Windows execution CI
remains a separate platform verification gate.

## Current shared engine boundary — 2026-10-31

The shared filesystem operations module owns canonical write, snapshot
edit, copy, move and remove. Each operation acquires the original
SecuredFilesystem lock and delegates publication to the one pinned
Unix/Windows backend; AW no longer duplicates lifecycle orchestration.
Legacy conditional-write preimages retain their original adapter path.

Host policy may further narrow the shared engine without changing its semantics.
AW does this through `LocalToolOptions::{limits,concurrency}`: schema/input ceilings,
direct/Graph result budgets, shell stream retention, Job quotas and scheduler width
all come from one validated trusted runtime snapshot. Existing `ToolPolicy` scheduler
ceilings compose narrow-only, dynamic Agent parallelism updates are transactional, and
internal Graph scheduling can never bypass `--tools-parallel off` or a host's tighter
read/shell ceiling. Product configuration may narrow these values but cannot widen the
shared 64 MiB per-stream/read/mutation and 8 GiB aggregate Job-spool safety maxima.

Direct host presentation budgets are deliberately separate from NodePlan semantic
budgets. A host adapter should render/page an ordinary direct call to the actual
per-invocation delivery ceiling before the effect/result leaves the tool. Graph
nodes keep the semantic ceiling captured at Job preparation; later transport or
context pressure may only page/hand off retained output.

A `private_artifact::Store` that reports volatile custody may own transient
stage/backup records, but `Registry::reserve` rejects `Tombstone` on such a store.
Recursive/exact remove therefore requires a host-provided durable store. Backends
reserve every requested tombstone before the first namespace detach and revalidate
the source identity immediately before each rename.

For hosts with wall-clock retention, `JobManager::expire_terminal(id)` deliberately
contains no clock: the product owns the deadline and may expire a quiescent terminal
Job (including unacknowledged payloads) once its policy says the Job is no longer
visible. Live/in-flight or shared-replay-owned Jobs fail closed with `Busy`. ShellNode
stdout/stderr may use either the memory-backed atomic attachment used by AW or
`attach_shell_output_files(...)` with host-opened secure spool files; result ownership,
quota, paging, ACK and terminal metadata remain identical.

The shared `JobManager` owns canonical live-session Graph lifecycle, immutable
NodePlans, deterministic conflict admission, retained result/stream paging and
ACK ownership. It still does not own a model, journal, process launcher or
cross-process workflow database. AW projects it through one model-facing
`JobRuntime`; other hosts may bind the same manager to their own authority and
transport replay boundary.

Shared constants are engine-safety maxima rather than AW product defaults. The
canonical renderer and raw retained-output pager permit up to 64 MiB pages,
Graph request JSON is bounded at 64 MiB/64 nodes, and aggregate Job spool policy
may be configured up to 8 GiB. AW deliberately chooses smaller defaults in its
adapter; these larger bounds exist so a consumer such as dotlink can use the same
implementation without semantic narrowing or duplicated render/spool logic.
