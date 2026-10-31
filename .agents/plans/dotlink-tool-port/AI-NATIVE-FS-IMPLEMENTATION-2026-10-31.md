> Superseded design reference. Current dotlink c34ea5a sealed spec,
> latest source parity and next-agent plan are in
> reference/DL150-SEALED-c34ea5a-README.md,
> PARITY-AUDIT-2026-10-31.md and
> NEXT-AGENT-HANDOFF-2026-10-31.md. Historical phases below remain.

# AW / DL-150 AI-native filesystem implementation

Author: @pvl
Co-authors: ChatGPT GPT-6
Owner: @pvl
Status: Implementation in AW worktree, uncommitted
Base: 11bea90 plus existing owned, uncommitted shared-toolset port

Source of truth: dotlink's approved
.agents/plans/dotlink-ai-native-filesystem-2026-10-31/README.md,
through sealed commit a156142 (including 8a3e7bb, 2a255f3,
5942393, 6c1cb16, f28ce2c, 60b348d, f09790d and ebb5b84),
consulted read-only through dotlink-l5.
Sealed model-facing semantics were not modified in dotlink.

## Decisions

- Shared package = aw-tools-core, Rust import aw_tools_core.
- aw-tools is ONLY an Agent/ToolPolicy/ToolContext/Process adapter.
- Reuse existing AW pinned Unix and Windows roots; no path-based fallback.
- Runtime permissions are separate from Cargo selection. User-facing
  off/read-only/rw/full accepts workspace/trusted input aliases.
  Durable child-session SQLite labels remain read-only/workspace; CLI
  aliases never silently migrate existing storage or expand grants.
- Cargo feature presets may remove unused code, but enabling a feature
  NEVER issues a model grant. No second Agent/TurnJournal/task registry.
- DL-150's model-visible vocabulary, compact outputs and exact path rules
  are authoritative. AW legacy aliases accepted only at adapters where
  required by existing callers; never advertised to new model clients.
- Cross-process mutation locks and journal/replay guarantees apply to all
  new lifecycle operations; no silent cross-volume copy+delete.

## Sequential implementation checkpoints

- [x] Rename aw-toolset -> aw-tools-core; Cargo, lock and imports updated.
- [x] Shared metadata (size/mtime/ctime/btime/mode), filters, ordering,
      rendering, bounded globally ranked paging.
- [x] Unix/Windows descriptor-rooted metadata and exact symlink kind.
      Native Windows/macOS runtime verification remains open.
- [x] One reusable scoped walker for ls/find/grep; hidden/gitignore,
      incomplete reasons, stage/tombstone exclusion and bounded traversal.
- [x] DL-150 read-only model surface: direct ls, recursive find,
      grep(context), compact globally ordered paging, status grammar
      more offset=N/incomplete=causes and operator-enforced scan budgets.
- [x] Sealed edit(path,edits=[[old,new,all?],...]) with one original
      snapshot, overlap rejection and one staged atomic publication.
      Legacy singular/multi-file shapes remain input-only AW compatibility.
- [x] Read(line,lines,byte,numbered) with UTF-8-safe same-line cursors,
      valid prefix + I/O-fault signaling and input-only offset/limit alias.
      Complete numbered=false reads are byte exact. Partial numbered=false
      uses a JSON data/status envelope because AW ToolOutput is text-only.
- [x] cp(from,to) recursively copies complete file/tree/link by default,
      same-volume no-replace mv, bounded preflight-and-tombstone rm, all
      through the existing scoped root and mutation guards.
      Mac/Windows cross-compile != native race/ACL verification.
- [x] Shared status grammar and 1-based rm failure coordinates: known
      partial commit vs post-rename cleanup pending; no generic retry of
      uncertain mutations. Mixed cleanup counts are not double-counted.
- [x] Build features read-only/rw/full; runtime permission intersection
      is independent, shell requires separate explicit Process grant.
- [x] Shared canonical 13-tool schema registry; public grep context,
      strict patch and recursive-by-default cp remove old advertised knobs.
      Shell accepts optional advisory writes hints (not runtime grants).
- [x] Pure full-feature batch graph compiler and tiny model schema:
      1-based compact tuples, status predicates, parallel roots, joins,
      skip conditions, backward-only references and host-supplied nested
      schema validation. Not registered to models until the original
      TurnJournal supports durable per-node replay identities.
- [x] Shared cumulative progress snapshots for known/unknown totals,
      monotonic counters, phase changes and bounded output labels.
- [x] Corrected cleanup-pending rm counts: an unpublished cleanup is
      not double-counted as a fully complete operation.
- [x] Added exact paging/coverage-combination regressions for
      find/grep and final-length edit planner regressions.
- [x] Compact schema/result measurement and docs.
- [x] Original pinned workspace compilation, native tests, Unix tests,
      Windows GNU cross-target, strict Clippy and no_std checks.
- [ ] Per-tool provider-native multiple content blocks for exact partial
      unnumbered read data plus status; AW's current ToolOutput is one text
      string, so JSON is the explicit boundary compromise.
- [ ] Full failure-injection coverage of every stage/commit/durability
      boundary for write, edit, patch, cp, mv and rm, and durable per-item
      journal state for exceptional multi-file partial effects.
- [ ] Model-visible sealed batch(ops) graph execution with deterministic
      per-node journal/replay identities, cancellation and bounded result
      allocation; parser/compiler is ready, but no unsafe second executor
      or mutation-capable model tool was installed.
- [ ] Shell write-hint scheduling integration and durable job_start hints;
      validated hint inputs are advisory in the current AW shell adapter.
- [ ] Sealed V2 canonical Job(Graph[Node...]) runtime: bash/pwsh are
      one-node Jobs, batch is a multi-node Job, job_start exposes a one-node
      shell Job immediately. One namespace/Task projection, persisted
      pending/claimed/running/terminal node checkpoints, automatic resume,
      explicit replay-protected job_restart, retention and bounded output.
      No second JobManager, Agent, or TurnJournal. Shell graph nodes must
      not be enabled merely by marking them batchable.
- [x] Windows model-facing shell tool renamed to pwsh; old powershell
      selection remains input-only compatibility while --tools-powershell
      continues to select its trusted executable. GNU cross-compiles.
- [ ] Native Windows/macOS filesystem race/ACL verification, successful
      Bubblewrap namespace runtime, full Nix/CI and monolithic workspace
      integration suite; test timeouts are not passes.
- [ ] Live dotlink adoption after AW acceptance; do not modify dotlink
      during this implementation phase.

## Hard limits

- Maximum directory traversal 100k visited, max depth 128.
- Max 4096 grep candidates, 32 MiB/4096 files actual scan bytes.
- Scoped root, no symlink traversal, no hardlink reads/writes.
- Exact lifecycle paths with root-wide mutation coordination.
- No implicit overwrites, no shell fallback, no unbounded recursion.
- If native OS features cannot guarantee no-replace/no-follow behavior,
  return an explicit unsupported result rather than weakening semantics.

## Verification evidence — 2026-10-31

Executed against AW's original pinned Cargo.lock:

| Gate | Result |
| --- | --- |
| aw-tools-core read-only | 41 passed |
| aw-tools-core rw | 64 passed |
| aw-tools-core full | 120 passed |
| aw-tools native | 96 passed |
| Unix platform | 15 passed |
| Agent core | 70 passed, 1 ignored |
| AW CLI binary | 383 passed, 1 ignored |
| AW CLI library | 226 passed, 2 ignored |
| Workspace --all-targets check | passed |
| Strict -D warnings Clippy on shared/native | passed |
| Windows GNU core/backend/aw-tools --all-targets | passed (compile) |
| no_std+alloc kernel | passed |
| cargo fmt --all --check; git diff --check | passed |

The aw-store full library suite was attempted with a 55-second bound,
timed out while running tests (not a pass). Previous monolithic workspace
test attempts also lacked a terminal result. The SQLite child-mode
integration is specifically validated by the passing 383-test AW CLI
binary suite.

13 canonical filesystem tool schemas total 6,177 serialized bytes
in AW after the sealed compact-tuple edit and read-cursor migration,
versus 7,111 bytes at the earlier schema checkpoint.
The separately compiled batch(ops) schema does not duplicate nested tool
schemas and is NOT registered without host-owned replay safety. Representative 100-entry compact listing:
2,000 bytes versus 5,504 bytes legacy-style JSON (~64% smaller).

## Further convergence before dotlink adoption

The sealed public AW semantics are in place for its 13 filesystem tools,
and the shared batch graph compiler is available but not executing.
Actual dotlink replacement requires first moving the platform-specific
pinned-root, mutation-lock and stage/rename/delete implementations behind a
reusable core/backend boundary, avoiding a second filesystem authority.
After AW cross-platform acceptance, dotlink may link the shared core and
retire duplicated native tools only once conformance and security/replay
tests cover all existing operations. Dotlink remains read-only today.

The original AW host must retain immutable journal-before-mutation
semantics; no generic task executor, new model grant, or database migration
was introduced by this work.

## Coordinator handoff

All changes are uncommitted and unstaged. Original AW HEAD 11bea90.
The dotlink repo through a156142 was consulted read-only through l5.
No pushes, tags, releases, source schema migration, new Agent/journal,
unapproved shell capability or edits to reserved UX files occurred.
