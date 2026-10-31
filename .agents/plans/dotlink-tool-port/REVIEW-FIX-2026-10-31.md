# aw-tools-core correctness review and remediation — 2026-10-31

Author: @pvl
Co-author: ChatGPT GPT-6

Baseline: AW main 11bea90 plus owned uncommitted shared-toolset port.
Read/write scope: AW tooling crates and owned tooling plan. Dotlink is
read-only through l5. No independent Agent, TaskJournal, ToolPolicy,
process manager, version change, release, push, or model permission.

## Sequential fresh review/fix passes

1. **Original spool regressions (P1).** Quota exhaustion previously let
   a truncated stream retain later bytes under earlier source offsets.
   Dropping a spool also leaked its retained session quota. Fixed with
   irreversible per-stream truncation and RAII retained-capacity charge.
   Verified quota churn, exact pages, compaction and drop accounting.
2. **Capability, lifetime, platform review.** Unix spool preflight
   requires O_RDWR; Windows checks opened-handle nlink, identity and R/W
   before spawn. Each stdout/stderr identity must be distinct on both
   platforms. File cursors are reset before any writes. Writer quiescence
   is separate from successful persistence; Failed and Uncertain can
   settle without falsely certifying Completed. Replaced per-4KiB task
   dispatch with one bounded blocking writer per stream, 64KiB batches,
   four-batch backpressure queue.
3. **Search, sandbox, binary review.** Directory grep marks oversized
   text files as incomplete instead of returning a false clean miss.
   Oversized context falls back to its matching line/location.
   Linux sandbox policies retain pinned authorized root/write handles
   into bwrap preparation; path replacement cannot substitute a new
   writable mount. Child drop includes eventual reaping after kill.
   Binary no-op rechecks complete original source identity/digest.
4. **Patch complexity review.** Replaced repeated forward removal
   lookahead with one reverse pass, avoiding quadratic work on large
   all-addition hunks. Added a deterministic 12,000-line patch test.
5. **Error accounting review.** Poisoned quota admission previously
   omitted the first failed chunk from total observed bytes; now every
   failed/discarded chunk remains counted, without claiming retention.
6. **Panic/poison settlement review.** Dedicated blocking writer catches
   unwinds, marks spool output failed and releases leases. Failure
   terminal handling recovers poisoned locks only to verify writer
   quiescence; it never certifies Completed. Regressions poison quota
   and spool mutex after Process spawn.
7. **Final performance/platform review.** Default unspooled execution
   allocates no queue/batch buffer. Repeated sandbox reaping tests,
   multi-megabyte output, original-registry tool/Agent tests, Linux
   and Windows compilation verified. No additional confirmed tool
   correctness regressions in the final targeted pass.

8. **Post-matrix concurrency review.** The final parallel test run
   revealed a flaky writer-sealing test: a bounded number of yields
   did not prove that the background thread had actually executed.
   Replaced it with a bounded channel handshake. The test passes ten
   repeated runs plus the full 60-test shared suite; no production
   writer-lock redesign was required.

## Design decisions

Shared aw-tools-core owns planning, bounded memory/receipts, stream
accounting, file identity, exact-source fencing and sandbox policy.
AW retains the sole model ToolRegistry/ToolPolicy/TurnJournal/Process
owner. The platform backends retain pinned/no-follow filesystem
capabilities and cross-process mutation locking. Persistent physical
spool-file cleanup across restart is host-owned; the host must retain
the quota owner until the output file is disposed, or explicitly
transfer that responsibility to a durable retention owner.

## Executed gates with original pinned Cargo.lock

- aw-tools-core all features: 60 tests passed
- aw-tools: 78 tests passed
- aw-platform-unix: 15 tests passed
- aw-agent-core: 70 passed, 1 ignored
- AW CLI binary: 383 passed, 1 ignored
- AW CLI library: 226 passed, 2 ignored
- Workspace all-targets compilation: passed
- Strict -D warnings Clippy on core/tools/Agent/platform: passed
- Windows GNU cross-target core/tool all targets: passed, compile only
- no_std+alloc kernel, rustfmt and git diff check: passed

The entire monolithic workspace test suite did not return a prior
terminal result; do not treat it as a completed test run. Native
Windows/macOS runtime, bwrap mount namespaces (l5 host reports
EPERM on /proc), live dotlink consumer, model-facing persistent job
APIs and typed media remain separate pending gates.

## Coordinator handoff

Owned AW changes remain uncommitted and unstaged for reconciliation.
No push/version/tag and no writes to dotlink or reserved UX files.
