# dotlink-tool-port

Base AW: `53206b2`; dotlink: `fc53fc9` plus uncommitted tool-hardening tree.

Design/decisions: [README.md](README.md).

## Committed

### `7ce39d9` — native dotlink-style tooling under original AW authority

- [x] Native `ls,grep,read,write,edit,patch,bread,bwrite,bedit` surface; retire
  model-facing `find`, leave human Find untouched
- [x] Keep existing AW exact-source/effect policy and model-directed
  parallelism; do not duplicate the tool executor
- [x] Concurrent scoped read/list paths; no global read lock
- [x] Platform staged writes; old create/edit APIs use the same backend
- [x] Cooperative Unix advisory cross-process directory mutation locks,
  root-local native write mutex and snapshot checks
- [x] Dotlink patch grammar, multi-edit, binary range operations and bounded
  output/error receipts
- [x] New cross-process, read/list race, symlink, CRLF, preflight and staging
  regression tests
- [x] Update tool-facing reference documentation

## Uncommitted

- [ ] Native Windows compile/tests including DACL preservation/recovery (no
  Windows target in current environment)
- [ ] Typed media `bread` transport/ToolOutput support (requires separate
  kernel-safe media contract; not part of native text tools)
- [ ] Dotlink wire interoperability fixtures through AW's `aw-mcp` client
  (separate external-source authorization remains mandatory)

## Tests and verification

- [x] `cargo fmt --all` (Linux)
- [x] `AW_BASH=/absolute/bash cargo test --offline --locked -p aw-tools --lib`
  (36 passed)
- [x] `cargo check --offline --locked --workspace --all-targets` (passed)
- [x] `cargo clippy --offline --locked -p aw-tools --all-targets -- -D warnings`
- [x] AW CLI binary: `AW_GIT=... AW_BASH=... cargo test --offline --locked
  --bin aw -q` — 383 passed, 1 ignored, zero failed
- [x] AW CLI library: 226 passed, 2 ignored with pinned `AW_GIT`/`AW_BASH`
  in the earlier workspace run
- [x] `git diff --check` (all modified tracked paths)
- [!] Whole-workspace strict Clippy: blocked by unchanged
  `crates/aw-workspace/tests/{git_containers,files_containers}.rs` and
  `src/web/dock.rs` preexisting lint violations; do not alter other owner's files
- [ ] Full `cargo test --workspace` completion across remaining packages;
  connector command timeout interrupted later split runs
- [ ] Native Windows compile, file ACL/atomic-write/symlink tests: no Windows
  Rust target available; target check reports missing `core`


## Hardening parity follow-up (2026-10-26)

- [x] Compare live dotlink schemas, tool implementation and new hardening
  against AW (including shell and `bread` formats).
- [x] Top-N `ls`, ignore/hidden discovery, bounded `grep` search and
  aggregate ignore budgets, shared text classification, exact-file filters.
- [x] Conditional/idempotent writes, cancellable read/lock budgets, staged
  parent-directory cleanup, mutation lock protocol matching dotlink.
- [x] Shell parameter parity and trusted Windows PowerShell registration.
- [x] Linux mandatory no-symlink/no-cross-mount `openat2`; Windows named
  kernel mutex coordination and ACL-preserving staged publication source.
- [x] Additional source-level compatibility and adversarial regression tests.
- [ ] Typed `bread` MCP ContentBlock media, distinct from encoded native JSON
  (requires explicit Agent/provider data contract; preserve existing policy).
- [ ] Host-selected sandbox execution separate from `trusted` Process:
  Bubblewrap mounts/network/security require dedicated platform validation.
- [ ] Windows/macOS native compile/runtime, ACL/recovery, process/mutex and
  unsupported platform checks.
- [ ] Cross-repo live MCP wire/media interoperability under AW's exact-source
  authorization, not remote read-only hints.

## Parity validation

- [x] `cargo fmt --all --check`
- [x] `AW_BASH=/absolute/bash cargo test -p aw-tools --offline --locked --lib`
  (53 tests passed, including unreadable-tree / missing-vs-inaccessible
  ignore-file regressions and full argument-schema fixtures)
- [x] `cargo clippy -p aw-tools --all-targets --offline --locked -- -D warnings`
- [x] AW CLI `--bin aw` 383 passed, 1 ignored; AW `--lib`
  226 passed, 2 ignored in an earlier full integration run
- [x] Platform/Agent/tools package group, MCP/providers/extension package
  group and workspace/editor/Git/terminal package group passed
- [x] `cargo check --workspace --all-targets --offline --locked` passed;
  strict tool/Agent Clippy and formatting passed
- [ ] Remaining complete workspace package groups (one monolithic runner
  invocation was interrupted by the connector, not a returned test failure)
- [ ] Native Windows/macOS runners and typed media integration checks
- [x] Review all dotlink `fc53fc9..9c771d2` commits and uncommitted
  native-tool hardening; record applicable vs non-applicable subsystems in
  README crosswalk
- [x] Integrate unreadable subtree/candidate and denied .gitignore reporting,
  bounded ignore loading and partial-result reasons
- [x] Windows managed PowerShell process contained in a Job Object before
  resuming suspended child; cleanup owned by existing Process service
- [x] AW Agent result budget now preserves valid JSON truncation envelopes
- [x] Windows Process service owns suspended native PowerShell children
  through a private Job Object before resume; no PID-only termination
  for those Process instances. Windows-native execution still pending.
- [x] Source-checked Windows Job Object and ToolHelp APIs in the pinned
  windows-sys 0.61.2 dependency; no claim of Windows target compilation.
- [x] Relevant `aw-store` exact-source tool-event regression passed (1 test);
  full 201-test store unit suite exceeded connector command lifetime.
- [x] Remaining pipeline/inference/identity/collaboration, store API/session
  format, workspace/editor/Git/terminal, platform/Agent/tool, MCP/provider
  and extension package groups passed in separate bounded runs.
- [x] No-std `aw-agent-kernel --no-default-features --features alloc`
  compiled; `aw-cli --bin aw` 383/383 active tests passed, 1 ignored;
  `aw-cli --lib` 226/226 active tests passed, 2 ignored.


## Audit-remediation validation — 2026-10-27

- [x] Reproduce oversized-grep match, >50 hits/file, read line marker,
  dotlink temporary leakage, invalid model-visible JSON, metadata loss,
  unsafe parent rollback, doubly linked create, shell/schema mismatch and
  Windows cross-target build failures
- [x] Fix search, result, schema, native publication and metadata behavior
  at shared tool/Agent/platform layers without a second executor
- [x] Native `aw-platform-unix` publication tests: atomic one-link create,
  create-only conflict, replaced-parent identity and owner/mode/xattr
- [x] `aw-tools` 59 unit/regression tests passed
- [x] `aw-platform-unix` 14 tests passed
- [x] `aw-agent-core` 70 active tests passed, 1 ignored
- [x] Windows GNU cross-compile `aw-platform-win` and `aw-tools`
  with `--all-targets` (Rust 1.98.1 and installed std target) passed
- [ ] Windows native Job Object, ACL, reparse-point, DACL-initial-create,
  metadata and rollback tests (requires actual Windows runtime)
- [ ] Full Windows CLI target build (requires MinGW C compiler for SQLite)
- [ ] macOS native ACL/xattr and filesystem publication tests
- [ ] Typed MCP media and sandboxed Process (separate authorized design)
