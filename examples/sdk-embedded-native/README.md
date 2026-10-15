# E4 — independently linked native Linux Agent without libc or CRT

This is an intentionally small **native x86-64 Linux host** for the existing
shared aw no_std + alloc embedded Agent. It links the **same**
aw-embedded-example::run_demo() and actual_portable_trace() as the E3
zero-import WebAssembly and hosted-vs-embedded parity tests. It does **not**
duplicate the Agent state machine or implement a different tool registry.

Reproduce both debug/release proofs:

~~~bash
cargo test --locked -p aw-embedded-example
~~~

The former native-smoke helper was removed; build and execute the host directly. It used the **checked-in Cargo.lock**, compiled the real independent
E3 Agent host library and its dependencies with panic=abort, link the
standalone no_std/no_main executable using ld.lld -static with
a genuine _start entrypoint and no CRT, then **execute** it on x86-64 Linux.
The host.rs supplies an aligned 4MiB bump allocator, panic-abort handler,
compiler memory primitives (memcpy, memmove, memset, memcmp, bcmp),
an unreachable rust_eh_personality abort shim needed by target-prebuilt
core/alloc metadata, and only direct Linux x86-64 syscalls for write and exit.

Runtime must return:

~~~text
E4_NATIVE_AGENT_OK code=0xA80E0001 events=10
~~~

That value is emitted after the actual shared Agent's model→source-granted
tool→journal-intent-before-effect→tool-receipt→final answer path, with two
model calls, one granted tool call and ten kernel event checks. A separate
native memory-primitive self-test verifies overlapping memmove and byte
compare/copy correctness before model execution. The debug suite also links
and executes two deliberately invalid static ELFs: one rejecting an otherwise
valid witness (exit 1), and one forcing a host panic to **abort** without
unwinding into a CRT/libunwind (exit 127).

The proof script uses readelf/nm on the **final executable** to require
ELF64 native x86-64 EXEC, a real entrypoint and shared Agent symbols,
**NO** PT_INTERP, **NO** PT_DYNAMIC/DT_NEEDED, **NO** undefined symbols,
**NO** dynamic relocations, and explicit native host memory/entry/panic
symbols. A static musl build or no_std rlib alone would NOT satisfy it.
Audited outputs and SHA256 are written beneath ignored
.agents/plans/evidence/host/e4/native-{debug,release}.json; binaries under
ignored target/e4/. The full hosted aw CLI is unchanged and std-based.

**Precise scope:** Target is x86_64-unknown-linux-gnu with a custom, actually
executed static **libc-free** ELF. The Rust source uses no standard library
and the final image loads no libc/CRT. Linux **kernel syscalls** still
provide OS entry/exit; NOT standalone bare-metal x86_64-unknown-none,
ARM/thumb or RISC-V firmware. The installed Rust 1.95.0 lacks those native
bare-metal core/alloc targets and complete rust-src; pinned Rust 1.98.1,
other native OS builds and native board runtimes have not run here.
No C ABI SDK, firmware driver, live inference/network transport, production
credentials, pointer-reentrancy ABI or signed release claims are made.

There is no Cargo package for this host because rustc direct final-link
flags and an independent host allocator/panic policy are integral to the
reproducible libc-free **final ELF** proof. The aw-embedded and
aw-agent-kernel libraries retain safe Rust and their existing additive Cargo
features; runtime-specific unsafe code is confined to the independent host.
