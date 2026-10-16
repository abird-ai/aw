# Optional embedded targets and independent certification

Accepted/provenance checkpoint: `47ee8a03abecbb3aee84a49c0a590096d76ed1a7`.

Parked optional work; desktop/WASM/native ABI evidence does not certify hardware, hostile-network security or live billing. Reopen only when separately authorized.

## Completed

- [x] Portable kernel/Agent remains no_std+alloc with host-injected capabilities; hosted HTTP/SQLite/process/renderer/publication dependencies do not enter the kernel graph.
- [x] Actual zero-import WASM, Linux libc/CRT-free execution and independent C11 ABI/negative controls exist at the accepted checkpoint; they are separate platform claims.

## Uncommitted

- [ ] E6 firmware: choose actual board/target, allocator/memory/stack budgets, clock/interrupt/transport capability boundaries and a minimal witness before implementation; prove real hardware boot/execution/recovery rather than substituting desktop tests.
- [ ] RISC-V or additional E4/E5 OS/architecture variants remain optional; specify linker/ABI/import/export and execution witnesses per target before claiming portability.
- [ ] Optional R2b: consider external rg/fd acceleration only after measured need and explicit safe executable/root policy; Windows trusted-shell selection/Job Object cancellation remains unimplemented and separately gated by native authority/cancellation tests. No new default shell permission.
- [ ] Preserve one Agent/TurnMachine/ToolPolicy/journal and four Runtime primitives/two Hook verbs; no new execution authority, autonomous replay, implicit listener or hidden indexer from an embedded adapter.
- [ ] Independent hostile-network/security or live-account billing certification, if required, needs a separately approved scope, real evidence and privacy/custody controls; fake providers and local paired transports cannot supply it.
