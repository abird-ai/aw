# Independent no_std+alloc embedded Agent — E2/E3 real turn

This is a real embeddable Agent turn adapter, **not** a second std CLI, second tool loop, Rust dylib or C ABI. The exact same aw-agent-kernel TurnMachine transitions are invoked inside standard aw-agent-core and this embedded adapter. The embedded consumer is a no_std library with alloc, JSON-compatible typed messages and no std/network/SQLite/libc dependency.

## Build and run the real proof

From the root:

    cargo test --locked -p aw-agent-kernel
    cargo test --locked -p aw-embedded
    cargo test --locked -p aw-embedded-example
    cargo test --locked -p aw-agent-core --test portable_embedded_parity
    cargo check --locked -p aw-embedded --no-default-features --features alloc --target wasm32v1-none

The `--target wasm32v1-none` check builds the module; the former Node helper that linked it, validated zero imports and executed the turn was removed. The test's temporary host supplies an intentionally simple test-only allocator and panic handler; no unsafe, allocator, libc or OS runtime is introduced into the reusable Rust libraries. Actual compiler/target and module digest are reported. Node and wasm-ld are required for this proof, not for embedding the library itself.

The independent fake device implements HostPorts: monotonic clock, secure nonce, explicitly acknowledged journal appends, typed model transport, exact host-owned source/epoch/grant, schema validation and one tool executor. It drives start, model_step, tool_step and model_step explicitly and verifies full intent/receipt ordering. No operation happens on new() or on import.

### Integrating your real embedded host

Use aw-embedded with --no-default-features --features alloc. Supply a production allocator and panic policy in your own final image. Implement HostPorts with real verified model transport, authenticated account, durable journal commit+flush, current exact tool grant, secure hardware RNG and monotonic deadline clock. An unsupported or unavailable host service must fail closed. A host callback that blocks must enforce its own deadline; this adapter cannot preempt your firmware callback.

No old tool intent may be reexecuted from a persisted receipt/journal without inspection. Requesting a model response is an explicit host step and may incur external cost. Current E2/E3 handles one bounded text+typed-tool turn and does not restore old Session credentials, automatically reconnect, do streaming/MCP transport, implement a native C ABI, run subagents, or certify a specific hardware/ELF target.

Refer to [accepted history](../../.agents/plans/accepted-history/TODO.md) and [accepted history](../../.agents/plans/accepted-history/TODO.md). Do not confuse static-musl (libc included) with a libc-free final image.
