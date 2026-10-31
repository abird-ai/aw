# Unified native toolset design and post-sync port, 2026-10-30

Author: @pvl
Co-author: ChatGPT GPT-6

## Provenance and rules

Read-only source: sibling `src/dotlink`, base `9c771d2` (2026-10-26), head
`70939fb` (2026-10-30). Exactly 16 intervening commits; no dotlink writes.
AW starting HEAD `11bea90` (2026-10-30); existing original Agent, exact
ToolPolicy, durable TurnJournal, model-directed tool scheduler, and
`aw-platform` pinned/no-follow filesystem are authoritative. No migration
support or duplication of durable ledgers. The shared library cannot create
permissions, alter a tool identity or start a background task implicitly.

## Every dotlink commit since the last sync

| Commit | Audit and relevant changes | AW treatment |
|---|---|---|
| `08ca60d` | v0.10.0: serialized-result budgets, glob/grep limits, MCP/OpenAI HTTP and OAuth hardening, mutation accounting, profile ceilings | Tool result/scan/mutation budgets and schemas: adapt; transport/OAuth: not applicable |
| `439098a` | OpenAI poll batch control-plane maximum | Skip: AW not dotlink's tunnel |
| `aa8d817` | v0.11.0 supervised recovery, private child env, launcher authority, shell lifecycle | Adapt process ownership where needed; AW already owns journal/process cleanup; no second supervisor |
| `ebddfc5` | Clean stop when service unconfigured | Skip: dotlink daemon lifecycle |
| `c1633b2` | v0.12.0 release metadata | Skip |
| `3f38427` | Real macOS/Windows build gates, IO portability, mutation lock identity | Cross-target checks; adapt platform-safe native algorithms |
| `fe375df` | Platform-safe grep fixture names | Test portability: adapt |
| `28bcd5a` | v0.13.0 release metadata | Skip |
| `b6aa9de` | New centralized mutation planner/staging, bounded receipts, bounded array schema, reduced duplicated edit/patch/binary logic | Port shared algorithms and adapters |
| `f3309ad` | Borrowed request/source bytes, in-place shrinking replacement, streaming binary splices, borrowed patch lines, no-op checks | Port to new sync core and native adapters; no unbounded retained buffers |
| `bb8a917` | v0.13.1 release metadata | Skip |
| `3a775e2` | Durable jobs/tasks and shell stream spools, KMP matching, denser search paging, runtime authority, task CRUD | Shared task interface + budget contract; AW uses existing durable Session/Turn ownership, no competing jobs DB; match/search/edit optimizations adapted |
| `9e36979` | Durable job fencing, bounded spool, sandbox checks, replay/transport controls | Adapt bounds & optional sandbox host interface; transport replay belongs to dotlink, not AW native tools |
| `594b57e` | Transport replay lifecycle, job cleanup/retention and protected launcher identity | Adapt execution-token/claim semantics at interface; don't copy OpenAI tunnel replay ledger |
| `567ae18` | v0.14.0 plus jobs/task finalization, shell and transport hardening | Port bounded task presentation contracts, source protections as appropriate; no automatic daemonization |
| `70939fb` | v0.14.1 jobs retention/partial receipt fixes | Adapt job/result budgets; keep runtime source of truth AW; no duplicate background workers |

## Target crate split

```
crates/aw-tools-core/  # prospective shared library, std + sync default
    limits.rs        # checked work/capacity budgets, typed result ceilings
    matching.rs      # KMP streaming sequence matching, no hit-position Vec
    mutation.rs      # source/working plan budget, compact bounded receipts
    text.rs          # memory-bounded literal editing + text guards
    binary.rs        # original-relative byte splices, stream hashing/staging
    task.rs          # opt-in task/journal host contract; no owned ledger
    sandbox.rs       # opt-in host sandbox contract; no implicit OS privileges
crates/aw-tools/     # thin AW ToolSpec/ToolPolicy adapters + AW Process hooks
crates/aw-platform/  # AW scoped pinned OS filesystem and locks
```

Synchronous native tool work is default and may run without Tokio. `async`
adds opt-in async hosting, `durable` admits a host-provided TaskJournal but
never includes hidden persistence, `sandbox` admits a host-selected isolated
executor. These are independent features; none silently changes source grants
or the original invocation's effect type. Dotlink later imports the shared
library with sync as its default and may opt into async/durable. AW always
adapts into its existing durable journal/Task runtime; the core library never
owns user sessions, tokens, a replay namespace or MCP transport.

**Authority and lifetime:** file authority is obtained from the host's pinned
backend; `Root` and lock/commit guards never leave the host adapter. Synchronous
pure planning returns bounded records. Host journals intent before effect,
controls cancellation/deadline, serializes binary/mutation outputs, and settles
unknown effects. The shared crate can calculate a plan, not independently
publish an unjournaled write.

## Safety/compatibility acceptance

- Plans cap sum of retained original/working bytes (64 MiB) *before* growth.
- Text edit limits 512 entries/64 paths only where per-call payload permits;
  retain AW source-bound path semantics and byte envelopes.
- Multi-file preflight and receipt sizing before staging. No all-files-atomic
  claim; partial publication yields an inspect-required error, not replay.
- UTF-8, escaped search paths, CRLF/EOF exactness, bounded deep-hit paging,
  bounded serialized JSON/tool results, incomplete-reasons array.
- Binary edits hash original, stream unchanged spans and verify even deleted
  data; no full-file copy required on the streaming backend. No-op preserves
  metadata/inode.
- Long-running task handoff is *host authorized*. AW native `bash` stays
  journaled Process and cannot implicitly opt into dotlink background jobs.
- Sandbox feature requires a host-supplied executable/mount/network policy;
  `trusted` stays explicitly unsandboxed and must not be relabeled secure.
- No edit to sibling dotlink repository during this task.

## Validation gates

Native unit/property/regression tests; AW Agent/ToolRegistry/TurnJournal tests;
Unix metadata/no-follow and multi-process tests; strict Clippy; no_std kernel;
AW CLI binary/lib; cross-target compile when toolchain available. Native
Windows/macOS runtime, typed MCP media, or reused dotlink deployment must not
be claimed verified by source checks alone.

## Implementation result and source of truth

The shared library now lives at crates/aw-tools-core with matching, patch,
mutation, text, binary, read, search, task and sandbox modules. AW explicitly
selects async + durable features, while sync is the default and the sandbox
feature remains opt-in. The existing aw-tools/aw-platform adapters use shared
algorithms without changing Agent ToolPolicy, journal or source grants.

See PORT-RESULTS-2026-10-30.md for the exhaustive fully/partially/skipped
inventory, commit applicability, exact validation evidence and unfinished
native/runtime gates. Never infer source, credential or process authority from
a shared library feature alone.


### Live uncommitted spool-writer follow-up

Current dotlink jobs/shell working tree extends the 16-commit baseline with
atomic writer-count sealing, wait-for-idle on runner finalization/shutdown,
panic settlement, retained-prefix compaction and per-stream release. The
shared library now contains the generic writer lease/quiescence invariant and
task bounds; AW still requires an exact durable host integration before
these can protect real background job spools. No dotlink files were touched.
