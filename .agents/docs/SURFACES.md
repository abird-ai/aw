# Workspace surface SDK — Phase 6

**Public Rust source API, pre-1.0.** `aw-surfaces` is a separate library composing the existing read-only file, Git-review and PTY services. It contains no HTTP server, browser renderer, credential transport, model routing or SQLite store. It transitively uses shared tool/core types; it is not claimed to have zero Tokio transitive dependencies. The headless agent and pure layout crates do not depend on it.

## Embed in another Rust host

```toml
[dependencies]
aw-surfaces = { path = "../aw/crates/aw-surfaces", version = "=0.15.0" }
```

```rust,no_run
use std::path::PathBuf;
use aw_surfaces::{Command, Options, SurfaceHost};

fn browse() -> Result<(), Box<dyn std::error::Error>> {
    let host = SurfaceHost::new(Options {
        root: PathBuf::from("/absolute/checkout"),
        git: PathBuf::from("/trusted/path/to/git"),
        bash: PathBuf::from("/trusted/path/to/bash"),
        actor: "host-authenticated-user".into(),
        allow_terminals: false,
        sites: vec![],
    })?;
    let listing = host.execute("host-authenticated-user", Command::Files { path: "".into() })?;
    println!("{listing}");
    Ok(())
}
```

Replace all example paths and authenticate the caller in the embedding host. Constructing an actor string is not authentication. The initial host supports one trusted principal. `Options` is deliberately not a browser-deserializable grant. Operations are blocking; invoke from a bounded worker. One internal resource mutex serializes operations without locking the Agent or UI renderer.

`execute_if(actor, command, authorize)` rechecks a quick host-supplied predicate **after acquiring the operation lock**, so a queued operation can notice revoked/expired authority. It does not cancel an effect already admitted/dispatched. Do not perform blocking I/O or call back into the same SurfaceHost from that predicate.

## Typed commands, opaque handles

`Command` is a `serde` tagged `op` enum with unknown fields rejected. It includes capabilities, files/file, Git inventory/open/review/feedback, terminal list/start/claim/release/screen/input/resize/stop/forget. `GitMode` is Changes/Worktrees/Branches. A completed directory/Git request returns only authorized bounded data; raw Git identity stays behind opaque cached item handles. There is no browser-supplied root, Git argument list, program/env, endpoint fetch, account, credential, principal or arbitrary JSON command interpreter.

`Site::new` validates a conservative registered HTTPS descriptor. It is not a generic browser or network gateway. `input_bytes` encodes a complete typed text/key/paste message using the native terminal contract; it does not execute it. The browser HTTP layer is in `src/web/workspace.rs`, not this library.

## Exact browser read scopes

Capabilities add a random owner-lifetime `source_id` and an opaque `view_root`
for the granted launch directory. The existing `root` descriptor and legacy
commands remain compatible. Files operations reuse one retained root and check
its physical identity before and after reads; replacing its pathname grants no
access to the replacement directory.

`Command::View { source_id, scope_id, request }` accepts only `ViewRequest`
Files/list/search, Git inventory/open and immutable Review reads. The result is
`{source_id, scope_id, result}`. A Git inventory returns its own `inventory_id`;
open requires that exact inventory and item under the originating scope.
Verified worktree selection returns another immutable Git-only scope. It never
grants Files access, rewrites an old scope by pathname or starts a terminal.
Retained Repository bindings and listed worktree directory identities reject
replacement before disclosure. Inventories and reviews expire under the existing
bounded caches; expired handles require an explicit navigator refresh.

The HTTP adapter distinguishes a changed source, invalid scope and an individual
read denial through typed error codes. Browser views clear all state for a
changed source, the affected scope for a confirmed binding denial, and only the
failed resource for a read denial. Cached File/Diff bytes are explicit snapshots;
refresh revalidates the original binding.

## Resource lifetime

The host retains known checkout contexts, cached reviews, native Terminal resources, transient control leases and launch receipts. A tab/view is not one of those resources. Moving/hiding/reopening views does not launch/terminate a process. `shutdown` revokes the surface host and requests termination of all retained terminals. Dropping it has the same cleanup intent; escaped processes are outside a promise of perfect containment.

Terminals remain unsandboxed. Lease ownership separates cooperating views of the same authenticated principal; a client-supplied tab ID is not a new user identity. Native extensions/embedding applications are trusted code. No security boundary can be inferred solely from metadata labels.

## Input and launch receipts

Launch request IDs bind context/confirmation/dimensions for the owner lifetime. Reuse with a different body conflicts; prior failed/uncertain launch is not repeated. A terminated/forgotten ID is not permission to resurrect it. The log is memory-only, bounded to64 entries.

Terminal input uses a random lease, clientID, expected sequence and complete typed input. The last exact duplicate is acknowledged without requeue; any different/older/skipped sequence conflicts. A receipt means accepted into the native input queue, not executed or paid/side-effect settled. Dropping a browser socket does not revoke all its OS effects. Authority/lease expiry blocks later operations, not previously dispatched commands.

The HTTP adapter adds same-origin/cookie checks, body limits, four bounded workers and post-operation read-disclosure checks. Those are not part of the `SurfaceHost` string API; an independent embedder supplies equivalent authentication and grant enforcement before exposing it remotely.

## Consumer and tests

`examples/sdk-surfaces` depends on this library and JSON only. No argument performs no filesystem/process action; an explicit directory lists its human-view contents. It does not start an Agent or server.

```sh
cargo run --locked -p aw-surfaces-example
cargo run --locked -p aw-surfaces-example -- /absolute/checkout
cargo test --locked -p aw-surfaces
cargo test --locked -p aw-cli --no-default-features --features web --lib
```

Executed checks and remaining platform/runtime gates are recorded in the [workspace checklist](../plans/workspace-views/TODO.md). A source package and local test pass do not imply a published stable crate/ABI or a security audit. [Browser behavior](BROWSER-WORKSPACE.md) · [Same Agent SDK](SDK.md) · [HTTP boundary](WEB-PROTOCOL.md).
