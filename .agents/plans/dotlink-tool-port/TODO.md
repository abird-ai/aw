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
