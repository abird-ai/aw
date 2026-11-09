# aw-tools-adapter — AW native host integration

Native adapters for AW's existing `aw-agent-core::Tool`. There is **one**
Agent, ToolRegistry, ToolPolicy, TurnMachine and durable intent/receipt journal;
no filesystem library may grant its own model/tool authority.

The transport-neutral filesystem engine is in
crates/aw-tools. AW selects rw+async+durable; the shared default
is synchronous read-only for future dotlink reuse. Durable replay authority still belongs to AW's original Session/Turn journal;
`aw-tools::JobManager` is bounded live-session state, not a second durable
database. Direct filesystem tools remain synchronous/foreground; `batch`,
`job_start` and trusted shells may expose the same live Job for explicit `job_*` control.

`LocalToolOptions` is the single trusted runtime configuration boundary for the AW
adapter. `LocalToolLimits` may only narrow the adapter's reviewed defaults and feeds
tool schemas, direct execution, Graph NodePlans/result budgets, shell retention, Job
quotas and job-control rendering. `LocalToolConcurrency` feeds both the Agent scheduler
and nested Graph width; adapter composition is narrow-only against an existing host
`ToolPolicy`, and dynamic Agent scheduler changes are transactional/refused while a
live Job still depends on the previous policy. Session replacement stays on
`InvocationScopeLifecycle`; bounded scheduler updates use the separate
`ToolPolicyLifecycle`, so neither hook becomes a second authority source. These are
runtime/SDK authority inputs, not values restored from persisted application config.

## DL-150 AI-native filesystem surface (2026-10-31)

The canonical shared crate is `aw-tools`; this crate is the AW-specific adapter to
AW Agent/ToolRegistry/ToolPolicy/TurnJournal and the AW platform backend.
Rooted filesystem operations are synchronous and feature-selected; the
AW adapter offloads bounded work with its existing Tokio runtime.
Dotlink adoption comes only after AW validation and will use the same
shared engine, not duplicate selector, copy, or mutation rules.

| Tool | Purpose |
| --- | --- |
| ls | Immediate directory children; compact kind + basename rows |
| find | Recursive pathname and metadata discovery; glob, regex, kind, depth, time filters |
| grep | Content regex/literal with a path glob prefilter and bounded context |
| read | line/lines/byte UTF-8 cursor paging; numbered=false is exact on complete reads |
| write | Create or replace complete UTF-8 text; model receives ok/unchanged |
| edit | Independent exact substitutions in one file: path + compact [old,new,all?] tuples, all from one snapshot |
| patch | Strict Codex Update/Add hunk grammar; partial publication failures explicit |
| cp | Exact source to new exact destination; complete bounded directory trees by default |
| mv | Same-volume exclusive rename; no overwrite or copy+delete fallback |
| rm | Exact paths; bounded preflight, recursive opt-in and durable exact tombstone cleanup; omitted when the host has only volatile private state |
| bread/bwrite/bedit | Specialized bounded binary inspection, creation and range editing |
| bash/pwsh | Separate explicitly trusted UNSANDBOXED Process grant; optional advisory writes scopes |

Public discovery vocabulary: path, glob, regex, depth, hidden, gitignore,
size, mtime (content modified), ctime (status changed), btime (created),
mode, sort, meta, offset, limit. Hidden=false and gitignore=false by
default; metadata is opt-in. Missing native timestamps are shown as -,
never synthesized. Discovery results are deterministic compact text,
not per-entry JSON envelopes; partial results include explicit footers.
Actual source/target mutation authority is verified by the pinned
backend and existing per-actor ToolPolicy.

Copy stages entire files (64 KiB scratch memory) and directory trees
privately before no-replace publication. Symlinks are copied as links and
never followed. Rename never silently becomes copy+delete. Remove
preflights all requested roots, atomically moves each root to a private
tombstone, and reports known partial commits and post-rename cleanup pending
distinctly from preflight failures.
The original Agent never automatically replays uncertain mutations.

AW's text-only ToolOutput cannot yet provide separate MCP data/status
content blocks. Complete numbered=false reads remain byte-exact; partial
reads return JSON with exact data and line/byte continuation. A read that
obtains useful bytes before an I/O error reports an error-marked prefix.

Canonical edit uses `path` plus `edits=[[old,new,all?],...]` against one
original snapshot. Legacy multi-file edit objects and the temporary singular
old/new/all shorthand are accepted only in AW's compatibility deserializer.
The sealed Graph compiler is provided by `aw-tools`; this adapter registers it
through one shared `JobRuntime`, with current native-tool eligibility and prepared
shell interpreters resolved by the host. Old AW CLI mode spellings
workspace/trusted remain accepted but the canonical user-facing modes are
rw/full. Old recursive ls fields are no longer model-visible.

## Authorization and scheduling

- off (default): no tools.
- read-only: ls, find, grep, read, bread.
- rw (workspace accepted for compatibility): read-only plus write, edit,
  patch, cp, mv, rm, bwrite, bedit.
- full (trusted accepted for compatibility): rw plus an explicitly selected
  UNSANDBOXED host native bash or PowerShell.
- `--tools-allow` intersects, never widens, those tool sets. A selected Job-backed producer may also expose the bounded `job_*` controls required to observe/cancel/restart that same execution; this does not add another node/tool capability. Child agents
  retain their original narrower read/read-write-edit subsets.
- AW's TurnMachine admits concurrent calls **only** for adjacent, explicitly
  model-selected `parallel:true` reads or already-granted trusted Process
  calls. Writes remain barriers. No execution in tool metadata or discoverers.
- `LocalToolConcurrency` is the one host-owned concurrency snapshot for both
  top-level Agent scheduling and nested Graph execution. Runtime SDK narrowing
  is transactional and is refused while live Jobs still depend on the old
  ceiling.
- `LocalToolLimits` owns AW's product defaults and may only narrow them. Shared
  `aw-tools` exposes larger engine-safety maxima so other hosts such as dotlink
  can choose their own bounded transfer/result policy without forking semantics.
  AW currently defaults to 48 KiB text/patch/encoded-binary input, 64 KiB binary
  read windows, 96 KiB semantic native results, 48 KiB per shell stream, and a
  512 MiB aggregate live-Job spool budget. Job-producing configurations must be
  large enough to retain one maximum-size Graph's exact result reservations; this
  is checked by normal configuration, public JobRuntime construction and shell
  registration before the capability is published rather than surfacing later as
  `skip`.

## Filesystem guarantees

- Descriptor-pinned, component-by-component, no-follow reads/writes. Ordinary
  user hard links are valid content; atomic replacement changes only the named
  entry, while private control/recovery files keep single-link integrity. Linux uses `openat2` with
  `RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_XDEV`; no fallback that
  silently drops required resolution guarantees.
- File content is validated as UTF-8 with no binary control bytes for
  `read`/`grep`/`write`/`edit`/`patch`. Arbitrary bytes use binary tools.
- Shared bounded globally ranked discovery; compact footers distinguish
  incomplete scans (budget, walk, gitignore) from normal pagination.
  Detailed reasons remain in host-internal accounting. The shared work budget caps
  actual file bytes and scan time, and the 4,096-file candidate set avoids
  retaining a vector of every match. Opt-in Gitignore/hidden filtering is
  bounded by both per-file and aggregate work ceilings.
- Snapshot verification, sibling staging, per-file atomic install, no-op
  detection, optional exact SHA-256 write preconditions. Existing Unix file
  owner/mode and supported ACLs/xattrs are preserved; setuid/setgid files and
  Linux `security.capability` require separate privileged host operations.
  Linux create-only publication uses atomic no-replace rename, never a
  link-then-unlink sequence that could publish a doubly linked file.
  Windows existing-target DACL is supplied at CreateFileW time before any
  staging bytes are written, avoiding a transient permissive ACL.
- Aborted stages remove temporary files. Empty parent directory cleanup uses
  recorded file identity to avoid removing a directory already replaced by
  another actor. Non-cooperating same-user processes can still race directory
  name resolution; no filesystem capability model is claimed to sandbox them.
- Unix mutations use the same sorted ancestor-intention/nearest-parent
  exclusive advisory `flock` convention as dotlink; Windows mutations use
  the same canonical-path-derived named kernel mutex convention. Read tools
  never acquire the mutation mutex. External programs ignoring the protocol
  can still race, and a multi-file commit is **not** a transaction.
- Cancellation, tool timeout, failed staging, and uncertain Process/write
  effects remain subject to AW's durable journal and inspection/no-replay.
  Shared planning/receipt code reserves aggregate memory and serializes a
  truthful bounded result before any commit. Changed-file receipts are
  preflighted; multi-file publication is still not a transaction.
- Windows native Process children are started suspended, assigned to an
  invocation-owned Job Object, and only then resumed. Killing/dropping a
  Process cleans its job rather than signalling a possibly reused PID.
  AW Git/terminal launch paths are unaffected by this added managed mode.
- Both PipeProcess reader tasks hold shared spool-writer leases. Terminal
  capture seals new writers and requires quiescence before a complete result.
  Inert ProcessOutputCursor offers exact head/tail pages, never fills lost
  middle bytes and ACKs only the contiguous prefix actually delivered.
- Trusted hosts may opt into PipeProcess::spawn_with_host_spools, passing
  pre-opened distinct stdout/stderr files and a shared bounded session quota.
  The host-only FencedProcessSpools adapter rechecks exact actor/source
  epoch/site/session/branch/call/effect and generation before output/ACK.
  Every terminal transition requires both spool writers to be quiescent.
  ShellNode streams and native results now attach to the one shared
  `aw-tools::JobManager`; no second process supervisor or persistent Graph
  database is installed.
- The Agent's shared output-budget projection and final model-visible tool
  message preserve parseable JSON, including for structured errors and
  truncated output. Human notices are appended only to plain-text results.
- `grep` preserves the location of oversized matching lines via explicit
  abbreviated-line markers, honors the requested hit limit across files
  (without an undocumented 50-hit/file cap), and distinguishes line
  abbreviation from result-page truncation. `read` reports the last returned
  line in its truncation marker.
- Shared walker excludes exact AW and dotlink staging/tombstone names, not
  ordinary user files with similar names. Shell argument validators enforce
  the same limits advertised in their JSON schemas; Windows advertises the
  canonical prepared executable as exactly `pwsh` or `powershell`, never `bash`
  and never a silent alias/fallback.

## Deliberate compatibility boundaries

Dotlink's `bread(format=mcp)` returns true MCP typed image/audio/blob
`ContentBlock`. AW's native `ToolOutput` is presently **text-only**, so
the same `format=mcp` input returns a complete-file, bounded, base64-encoded
JSON envelope with `transport:"encoded_json"`, a source SHA-256 and
signature-derived MIME classification. Spoofed extensions are not trusted;
unrecognized or potentially active content stays a resource. The signature
is not a codec-security attestation. True typed output requires an
explicitly designed Agent/provider media adapter; never bypass tool grants.

Dotlink's Bubblewrap shell is a separately sandboxed *machine execution
facility*. AW retains its existing explicitly trusted, **unsandboxed**
host-selected Process boundary. The shared sandbox feature has a separately
host-approved FD-pinned Linux Bubblewrap plan and RAII child, but no
model-facing grant, automatic job durability, or trusted-shell replacement.
Native l5 runtime mount verification failed with Operation not permitted; the
Linux builder is code-tested, not runtime-certified.

Windows GNU cross-compilation of `aw-platform-win` and `aw-tools`,
including test targets, is verified against AW's original pinned Cargo.lock
using the available matching Windows standard library. Native Windows and macOS runtime
hardening tests remain separate gates: cross-compilation cannot attest ACL,
job-object or filesystem recovery behavior. Full Windows CLI cross-compilation
requires a MinGW C compiler for the bundled SQLite dependency.

See [current tool contract](../../.agents/docs/TOOLS.md) and
[tool-port plan](../../.agents/plans/dotlink-tool-port/README.md).

### Process and search correctness (2026-10-31)

The opt-in durable Process pipeline runs one bounded blocking writer
per stdout/stderr stream. It batches 64KiB chunks through a four-item
queue instead of spawning a worker per 4KiB read. Foreground Process
without spooling allocates no additional batch or queue.

Host-supplied spool files are checked for readable/writable access,
single-link identity and distinct stream identity on Unix and Windows.
Failure/Uncertain settlement requires stopped writers, not successful
persistence; Completed additionally requires successful output sync.
Truncation is irreversible for that stream, with quota reclaimed on
drop or explicit compaction. ToolPolicy and TurnJournal are unchanged.

Grep now reports skipped oversized files as incomplete and retains
matching lines when verbose context will not fit the output cap.

### Host TaskOps integration (TS-N11..N20)

BoundProcessTaskHost exposes bounded, exact-source host Inspect/List/Page/
ACK and a host-supplied cancellation callback. It neither registers
model-facing job tools nor creates a second executor or registry. Fenced
Process transitions use the original host's journal-first CAS callback;
non-journaled production transition shortcuts were removed. Liveness
callbacks are invoked outside task-fence mutexes to avoid reentrancy
deadlocks, while every actual output/ACK is generation-fenced.


### Current sealed-regression checkpoint

AW's original Agent/ToolPolicy/TurnJournal and host filesystem roots
remain authoritative. Core/adapter tests pass (41 read-only, 64 rw,
119 full-core, 96 AW tools). AW CLI binary (383 passed, 1 ignored)
and library (226 passed, 2 ignored) also pass. Windows GNU builds
cross-compile; native Windows/macOS and successful Bubblewrap runtime
testing remain separate acceptance gates. Model-visible batch is NOT
registered until per-node durable replay is implemented by the
existing host journal. Dotlink has not been modified.


On Windows the model-facing shell tool is pwsh under sealed DL-150.
The previous powershell selected-tool spelling remains input-only
compatibility; the trusted executable CLI flag remains --tools-powershell.
No PowerShell binary is auto-installed or implicitly granted.

## Shared operation consolidation — 2026-10-31

Canonical AW write/edit/copy/mv use aw-tools operations under
the original Root's pinned filesystem locks and staged publication. `rm` uses the
same shared operation only when the configured Root has a durable private-artifact
Store; memory-only sessions and platforms without verified private mutation-state
custody simply do not advertise `rm` rather than weakening crash recovery. Volatile
roots can still stage ordinary mutations safely during the live process, but cannot
promise restart cleanup of an in-flight Stage/Backup after process death; callers
requiring crash-clean mutation artifacts must provide durable private state.
AW handles original Agent/ToolPolicy/TurnJournal, tool invocation,
compatibility and provider-oriented error rendering.

The canonical `JobManager` is now bound to AW through one live `JobRuntime`.
Model-facing `batch`, `job_start`, `job_list/get/output/ack/cancel/restart` and
direct trusted shells all use that same runtime, the original ToolPolicy/live
binding generations, outer TurnJournal replay fence, and existing `PipeProcess`
custody. Per-node Graph state remains intentionally live-session-only rather
than a second persistent workflow database.
