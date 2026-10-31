# Core-tools latest resume and validation — 2026-10-31

Author: @pvl
Co-author: ChatGPT GPT-6
Owner: @pvl
Status: Live AW worktree, uncommitted and unstaged

## Authoritative source and version

- AW main HEAD 11bea90, plus existing owned, uncommitted source.
- Dotlink local main HEAD 81a54ce, latest sealed AI-native filesystem
  and Job/Graph design, fully snapshotted in the owned reference files
  DL150-SEALED-81a54ce-README.md and DL150-SEALED-81a54ce-TODO.md.
- Dotlink local main is ahead of remote tracking. Authenticated GitHub
  fetch_commit(main) returned 84cd9b7, older than the sealed local
  design. Git HTTPS ls-remote failed TLS certificate-chain validation;
  no TLS bypass, remote mutation or unverified pull was attempted.
- Another agent exclusively owns the reserved UX plan directory and
  staging; it has not been listed, read or modified.

## Implemented: one shared filesystem engine

- aw-tools-core now orchestrates canonical write, snapshot edit,
  exact copy, same-device move and bounded multi-root remove through
  its SecuredFilesystem trait. The AW lifecycle adapter parses and
  maps outcomes, but does not duplicate lock/effect orchestration.
- aw-tools-fs-unix and aw-tools-fs-win are the original hardened
  pinned/no-follow OS backends, reexported by AW platform crates.
  No second ambient-path filesystem authority was introduced.
- On Unix, failure immediately after a private stage mkdir but before
  acquiring its first identity handle now removes only that empty,
  unpublished stage (not an unverified recursive path). Exact inode
  identity still fences subsequent recursive cleanup.
- The original AW conditional write preimage options (create_only
  and expected_sha256) remain a narrow adapter compatibility path;
  their original checks were not weakened.
- Mock SecuredFilesystem regression tests cover true no-op (no stage),
  text rejection, missing/ambiguous replacements, cancellation,
  source/destination validation, duplicate remove and a commit error
  after publication classified Unknown rather than retryable failure.

## Implemented: reusable pure Job(Graph) state, not a second executor

- Journal-first legal transition validation and application share one
  allocation-free state machine, including authenticated reconstruction
  of original host journal records; no replay executes tools.
- Job identities bind graph hash plus exact task Actor/source kind,
  source ID/epoch, site, Session/branch, turn, call and effect.
  Malformed task identities reject zero epoch or unknown source kind.
- Node path: Pending -> Claimed -> Running -> Terminal. Known completion
  is refused from a mere Claim. Claimed -> Skip requires an independent
  trusted host no-launch proof and its own durable UnlaunchedSkip event.
  Unknown effects must remain explicit.
- Runner-start verifies source liveness and launch permit again after
  claim; cancellation may escalate to revocation and stops new claims.
- Journal event ordering, recorded predecessor, graph, source binding
  and generation are all validated on recovery. Claimed/Running nodes
  are reattachment candidates, never automatically re-executed.
- This is intentionally not a registered model-facing batch/job_* tool
  or independent JobManager/TaskJournal/Process executor.

## Executed original-lockfile validation

- aw-tools-core read-only: 43 tests passed
- aw-tools-core rw: 71 tests passed
- aw-tools-core full: 141 tests passed
- strict shared Clippy all features -D warnings: passed
- aw-tools: 98 tests passed
- aw-tools-fs-unix: 10 tests passed
- aw-platform-unix: 7 tests passed
- aw-agent-core: 70 passed, 1 ignored
- strict aw-tools, aw-tools-fs-unix, aw-platform-unix and aw-agent-core
  Clippy all targets -D warnings: passed
- workspace --all-targets compilation: passed
- aw-agent-kernel no_std+alloc: passed
- Windows GNU cross compilation of shared core, Windows backend and
  AW adapter all targets: passed (compile, not native runtime).
  Unrelated aw-terminal emits two Windows dead-code warnings.
- AW CLI binary: 383 passed, 1 ignored
- AW CLI library: 226 passed, 2 ignored
- cargo fmt --all --check, git diff --check: passed

## Remaining host/runtime acceptance gates

1. Original immutable TurnJournal must commit per-node Job transitions
   with exact grant/source/actor/site/scope and replay proof before
   model-visible batch or job_* can be enabled.
2. Bind the pure graph projection to the existing AW Session/Process
   owner, not a second runtime, database, executor or scheduler.
   Require checkpoint before effect and terminal before dependents.
3. Nested graph operations must independently recheck their exact
   registered tool grants and current Actor Envelope.
4. Finish runner reattachment, output paging/ACK, cancellation,
   retention and explicit replay-protected job_restart within one
   original Job/Task identity namespace.
5. Provider-native exact data/status content blocks; current partial
   unnumbered read uses explicit JSON because AW ToolOutput is text.
6. Native Windows/macOS ACL/reparse/race tests, successful Bubblewrap
   namespace execution (l5 previously denied proc mount), power-loss
   and crash-injection tests, and full monolithic workspace run.

Coordinator owns signed atomic commits and reconciliation. No staging,
push, tag, version/release, database migration, dotlink modifications
or reserved UX access has occurred.
