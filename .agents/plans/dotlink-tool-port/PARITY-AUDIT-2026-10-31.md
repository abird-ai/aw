# DL-150 implementation parity audit — 2026-10-31

Author: @pvl
Co-author: ChatGPT GPT-6
Owner: @pvl
Status: reviewed, confirmed fixes applied; remaining gaps explicitly open

## Source of truth and review boundary

- Live AW repository: main at 11bea90 before this signed commit; all source changes in the owned tooling worktree.
- Latest local dotlink canonical design: c34ea5a (second sealed correctness/security review). Dotlink was read-only.
- Immutable exact copies: reference/DL150-SEALED-c34ea5a-README.md and reference/DL150-SEALED-c34ea5a-TODO.md. SHA-256 README bfcfeec5d4034fad82ab6a759af7f0b4d17cf2e1b4158d133a5116ea602a86b1; TODO 0216a108d5d9a0a4bb066b86750f38e6a350a7ccd95d948a59e20d269c55e0d1.
- Older reference snapshots (81a54ce) are historical, not canonical; do not treat dated handoff claims as stronger than live source or executed tests.
- Source reviewed: crates/aw-tools-core, crates/aw-tools, aw-tools-fs-unix, aw-tools-fs-win, AW Agent ToolRegistry/ToolPolicy/TurnJournal and CLI tests. Windows/macOS native runtime behavior was NOT exercised here.

## What is structurally implemented

- Canonical shared package aw-tools-core, sync/read-only default, individual read/write/lifecycle/binary and optional async/durable/sandbox/batch features. Feature selection grants no model authority.
- One capability-backed SecuredFilesystem contract, implemented by descriptor-rooted Unix and handle-pinned Windows backends and reexported by AW platform crates. No ambient-path alternative or duplicate mutable namespace implementation.
- AW adapter exposes one original Agent, ToolRegistry, ToolPolicy, TurnJournal, Envelope and Process owner. New core does not grant shell, launch a second executor, or create an independent durable database.
- Shared discovery and metadata selection, time filters, bounded traversal and paging, compact status/result grammar, and canonical 13-tool schema registry. AW adapter provides host-specific limits, cancellation and error translation.
- Core snapshot edit, write, copy, move and remove orchestration uses the original root mutation lock; native publication remains in secure platform backends.
- Pure immutable Graph compiler and guarded Job/Node transition projection, source/actor/turn/call/site-bound identity and original-journal replay validation. Not a registered model-facing batch or Job runtime.

## Current public v1 tool parity

| Tool | Status | Live implementation evidence | Remaining gap / caution |
| --- | --- | --- | --- |
| ls | Substantial | One-directory selection, explicit metadata, sort, compact rows, limited bounded pages | Named-root symlink authorization and native-unaddressable-path details require more runtime tests |
| find | Substantial | Recursive pathname/glob/regex/kind/time filter, globally ranked paths and continuation | Exact symlink-root dual authorization; case-insensitive path identity acceptance on Windows |
| grep | Partial | Logical-line hits, files/count/content, context, scan limits and compact statuses | Overlapping context-group coalescing, clipping at page boundaries and exact per-primary pagination need an independent adversarial pass |
| read | Partial | Bounded UTF-8 line/byte cursor, valid Unicode boundaries, useful-prefix I/O fault, full unnumbered exact data | Provider-native separate data/status blocks, final symlink target admission, tight transport ceilings |
| bread | Partial | Binary byte windows, encoded content; typed media policy core | Provider-native typed ContentBlocks and target symlink reauthorization |
| write | Substantial | Shared staged atomic replace; write-only path does not probe old byte equality; parent suffix published as one namespace operation | Final symlink content mutation is currently fail-closed rather than followed/re-authorized |
| bwrite | Substantial | Shared publication and bounded decoding, no equality oracle | Same resolved symlink authorization and native provider contract |
| edit | Substantial | Independent original-snapshot compact replacement tuples, ambiguity and overlap rejection, one atomic publication | Safe final symlink target authorization; legacy multi-file adapter remains until coordinated removal |
| bedit | Substantial | Original-relative bounded binary ranges, exact preimage/digest and atomic staging | Final symlink target authorization and native fault-injection |
| patch | Partial | Strict Codex Update/Add preflight with atomic per-file commits; mixed outcomes marked as errors | Nontransactional multi-file publication, provider presentation, per-child protected-path checks and fault injection |
| cp | Substantial | Exact bounded file/symlink/tree copy with atomic missing-parent suffix publication, no overwrite | Descendant deny/protected authorization, native NTFS/macOS failure testing, link-tree adversarial cases |
| mv | Partial | Same-device no-replace rename, never mkdir or copy+delete; failed move leaves destination parent absent | Resolved identity/case-only aliases on case-insensitive volumes; protected descendants policy |
| rm | Partial | Nonempty recursive explicit, multi-root preflight, inode identity/alias rejection, private tombstone, known mixed/cleanup outcome | Race/rollback fault-injection, denied subtree policy, crash recovery, NTFS ACL/reparse native tests |

Substantial means implemented with passing Linux regressions, not declared fully compliant. Any gap above remains an open work item.

## Fresh pass: confirmed bugs fixed in shared layers

1. .gitignore scope leak: scoped discovery previously loaded parent ignore files above the caller-named root. Shared walk now starts with a fresh policy at the requested directory; no global/ancestor rules are imported. Regression: requested_root_does_not_read_parent_ignore_files.
2. Incomplete ignore input: malformed UTF-8, overlong and exhausted .gitignore files previously failed the whole search or applied an incomplete rule set. Shared ignore handling now leaves safe results usable and marks incomplete gitignore; parser never applies partial contents. Regression: invalid_or_oversized_ignore_is_incomplete_not_a_hard_error, plus native grep work budgets.
3. Glob grammar ambiguity: matched path/basename choice was inferred from compiled regex text. Shared Filter now retains whether the original source glob contained a slash. Regression: glob_path_separator_is_decided_from_original_pattern.
4. Non-progressing discovery: a record exceeding the effective result budget could produce only more offset=0 forever. Shared discovery now rejects an oversized first semantic row with an actionable error. Regression: oversized_first_record_is_actionable_error_not_stuck_cursor.
5. Invalid filename round-trip: invalid-UTF-8 Unix names were omitted but indistinguishable from ordinary traversal limits. Platform listing now reports a distinct path_encoding flag, carried through shared walk/discovery/grep to incomplete path_encoding. Reusable paths are never lossy-replaced. Regression: unix_non_utf8_filenames_are_omitted_with_exact_path_encoding_warning.
6. Grep truncation violation: matching lines longer than 2048 bytes were silently replaced with placeholders, and requested context could be discarded while returning a primary match as though complete. Grep now treats one primary plus context as an indivisible block; it emits complete content when it fits, errors when the first record cannot fit, and emits a progressing continuation when a later record cannot fit. Tests cover 70 KiB matching line, over-ceiling first hit and huge context/path.
7. Strict lint: replaced an unnecessary fallible DateTime to SystemTime conversion; strict Clippy all targets passes again.
8. Write-only permission audit: existing core write_bytes/write_text already stages without reading original content and reports ok; verified direct tests, no new equality oracle added.

## Important open correctness and architecture gaps

### P0: security, authority and replay

- Final-component symlink semantics from c34ea5a remain NOT implemented: content tools currently reject no-follow final symlinks. Correct implementation requires lexical deny/protected validation, pinned resolution, reauthorization of the target under the exact same actor/source/site/effect grant, safe target-only staging (preserving link), dangling-link denial and race tests on Unix and Windows. Do not simply remove O_NOFOLLOW or trust canonicalize().
- Recursive cp/mv/rm descendant deny/protected checks and resolved namespace ancestry/alias behavior need a host-policy-aware test suite. The backend root itself cannot silently assume the ToolPolicy granted all descendant effects. Preserve atomic no-replace, identity and cancellation.
- Runtime Job(Graph[Node]) host integration is absent. The pure core may compile/validate/reconstruct Job transitions but does not register model-facing batch, job_start/list/get/output/ack/cancel/restart. Bind to AW's existing Session/TurnJournal/Process, durable claim-before-effect, terminal-before-dependent dispatch, source/grant liveness, runner reattachment, ACK/retention and replay tombstones. No second manager or database.
- Cross-process crash/power-loss tests, unknown-commit handling and staged directory cleanup under identity races need hostile fault injection before calling it production-complete.

### P1: exact query and output semantics

- Grep context windows across overlapping matches and page boundary clipping; logical primary offset must not leak later hits as context. Require exact labels and separators, and prove no 0-progress page.
- Gitignore semantic coverage beyond current tests: nested negative rules/anchored patterns, unreadable rules and exhausted loader while preserving safe results; follow requested-root boundary.
- Full native path canonicalization on Windows and symlink-root admission after double authorization; invalid path encoding support is now explicit but needs platform-native tests.
- Partial unnumbered read is encoded JSON in AW's single text ToolOutput, not provider-native independent data/status ContentBlocks. Support separate channel/blocks before replacing the dotlink MCP media implementation.
- Native metadata availability/precision (mtime/ctime/btime, link payload, mode) needs Windows/macOS runtime test fixtures.

### P2: portability, maintenance and measurement

- Native Windows NTFS reparse/ACL/share-mode and case-only rename tests, macOS renameatx_np support and sandbox runtime (Bubblewrap /proc EPERM on l5).
- Full monolithic workspace test and full aw-store suite terminal result are not yet evidenced. Do not convert previous timeouts to passes.
- Source policy plan remains long; keep one canonical spec snapshot and one owned parity/handoff record rather than parallel conflicting TODOs. Reconcile only the owned dotlink-tool-port plan; coordinator owns shared plans/TODO.md.
- Once AW host integration passes, port dotlink by depending directly on aw-tools-core and aw-tools-fs-{unix,win} with distinct transport adapters. Retire duplicated implementations only after behavioral parity/security regression. Dotlink remains untouched.

## Verified gates on the original AW Cargo.lock

- Shared core no-default/read-only: 51 passed.
- Shared core no-default/rw: 83 passed.
- Shared core no-default/full: 153 passed.
- AW native tools: 105 passed.
- Secured Unix filesystem backend: 10 passed.
- Unix platform: 7 passed.
- Agent-core: 71 passed, 1 ignored.
- AW CLI binary: 383 passed, 1 ignored.
- AW CLI library: 226 passed, 2 ignored.
- Workspace --all-targets cargo check: passed.
- Shared core strict Clippy all-features/all-targets -D warnings: passed.
- AW tooling/platform/Agent strict Clippy: passed.
- Windows GNU cross-check of shared core, Windows backend/platform and adapter: passed (compile only; two unrelated aw-terminal dead-code warnings).
- aw-agent-kernel no_std+alloc: passed.
- Cargo fmt --all --check and git diff --check: passed.
- Real Windows/macOS runtime, provider ContentBlocks, full monolithic workspace suite and complete Job host integration: not verified.

## Ownership / commit rules

- Commit signed; never disable signing. Do not stage/read/alter the reserved .agents/plans/ux subtree or another agent's index entries.
- Changes belong to AW owned tooling source, its needed AW adapter/Agent integration, Cargo workspace/lock and owned docs/plan. Dotlink was read-only.
- No push, tag, release, staging of other agents' files or runtime grants.
- See NEXT-AGENT-HANDOFF-2026-10-31.md for the exact independent review and implementation prompt.
