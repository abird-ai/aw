# 0.17 stop provenance — completed first slice

Base: `792f0b38d104659264b70c31736655df1a46b60f`

## Committed

### `796225e` — separate physical signal identity from lifecycle

- [x] `ProcessExit::Signal(i32)`.
- [x] Remove `ProcessExit::{Cancelled,DeadlineExceeded}`.
- [x] Unix reaped signal identity reaches shared `ProcessTerminal`.
- [x] Job cancellation/revocation lifecycle remains separate.
- [x] Requested AW termination does not fabricate a signal.
- [x] SIGKILL and SIGSEGV remain distinct.

## Uncommitted

None. Remaining shared-first work moved to
[017-shared-runtime-convergence-2026-11-18](../017-shared-runtime-convergence-2026-11-18/TODO.md).

## Tests and verification

- [x] `aw-tools` full suite — `796225e`.
- [x] `aw-tools-adapter` **253/253** — `796225e`.
- [x] strict affected Clippy — `796225e`.
- [x] downstream aw-mcp/aw-host/aw-cli checks — `796225e`.
- [x] rustfmt + diff check — `796225e`.
