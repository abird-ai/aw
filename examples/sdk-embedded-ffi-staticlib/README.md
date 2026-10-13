# E5 — independent native C11 consumer of the portable aw Agent

The portable no_std+alloc ABI is in crates/aw-embedded-ffi.
The independent C program here compiles with strict C11 -Werror and imports
ONLY the stable aw_embedded_ffi.h declarations (not Rust Agent internals).

Run BOTH fresh native packaging/build/link/execute checks:

~~~bash
cargo test --locked -p aw-embedded-ffi
~~~

The former C-ABI smoke helper was removed; build and run the C consumer directly. It first performed an explicit wasm32v1-none no_std+alloc library
check, verifies the Cargo normal dependency tree has no SQLite/Tokio/HTTP
service crates, builds the portable ABI into a native Rust staticlib wrapper
using panic=abort, and compiles the actual C host using:

~~~bash
cc -std=c11 -Wall -Wextra -Wpedantic -Werror -O2 -fno-common \
  -I crates/aw-embedded-ffi/include \
  examples/sdk-embedded-ffi-staticlib/c/fixture.c \
  target/e5/panic-abort/debug/libaw_embedded_ffi_staticlib_example.a \
  -o target/e5/aw-e5-c-native-debug -ldl -lpthread -lm
~~~

The wrapper's own dependency on Rust std provides native C-host staticlib
runtime and panic policy; the actual FFI crate remains no_std. This means
the FINAL C executable is libc-using and **not** an E4-style libc-free ELF.
Do NOT conflate a portable no_std Rust library with a standalone bare-metal
C host. The native test executable is an ordinary Linux x86-64 process.

The C program:
- checks ABI version, every struct size, status/event fields and 11 symbols
- proves create does not call a model, journal, tool or host callback
- mutates/discards stack-owned config/source strings after create, then
  checks the provider and authorization callbacks still receive original
  safe copies from Rust-owned storage
- executes one exact source-epoch/actor/site-granted tool-bearing Agent turn
  through two real mock model callback invocations and ten shared kernel
  events; verifies intent-before-effect, durable receipt and final answer
- tests strict caller-owned answer/trace copy, required lengths, no NUL
- rejects overlapping old/new effects via the shared kernel's exact grant
- attempts callback reentry into state and destructor; both return BUSY
- rejects missing callbacks, bad ABI sizes/versions, duplicate tool grants,
  invalid source kinds/epochs, malformed UTF8, excessive prompt lengths,
  callback output overflow and malformed model replies
- verifies authorization revocation AFTER intent prevents execution, a
  failure writing the ToolIntent prevents any tool effect, and a failed
  ToolReceipt or tool callback prevents silent replay
- verifies uncertain paid model callbacks, incomplete ModelSettled, and
  failure to append Assistant AFTER ModelSettled force explicit reconciliation
  before destroy, even if no tool ran
- verifies cancellation cannot mutate a completed turn or replay failed work

The test emits one strict machine-readable summary:
E5_C_ABI_NATIVE_OK checks=N model_calls=2 tool_calls=1 events=10

Host-local, ignored SHA256 and source/head provenance reports are under
.agents/plans/evidence/host/e5/native-{debug,release}.json; binaries under target/e5.
All receipt counts refer to an isolated fake provider/tool, NOT real paid
LLM accounts or a certified production C embedder. The underlying aw Agent
and std CLI remain unchanged, and future E6 board/firmware is NOT claimed.
