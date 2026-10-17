# Build status

Current work and verified completion belong in the [plan TODO hierarchy](../plans/TODO.md). Git checkpoints preserve completed implementation; this document describes capabilities and validation boundaries rather than a history of test runs.

## Current capabilities

| Area | Available implementation | Boundary |
|---|---|---|
| Application | One `aw` CLI, inline/scrollback Agent UI, fullscreen Workspace and browser UI; shared palette and explicit focus handling. | Native read-only Agent tabs pin exact authorized branches; scoped Files and Git containers retain independent human roots and exact review/index admission. Browser read-only Agent tabs use the same exact authorized branch reader; scoped browser Files/Git and docking remain planned. |
| Agent and providers | Shared Agent/tool loop, provider/model catalog, bounded tool concurrency, project resources/skills, MCP, process extensions, JSONL output and stdio RPC. | Local mocks validate protocol behavior; they do not certify current live vendors, prices or billing. |
| State and authority | Domain → Workspace → Session, immutable accepted history, exact branch/CAS and writer fences, durable effect receipts, SQLite and bounded JSONL stores. | Browsing does not grant execution. Unknown external effects are not automatically replayed; account, payer, custody and executor remain distinct. |
| Feature cuts | Default Full, JSONL Minimal and stateless CLI; separately tested Workspace and optional feature combinations. | Passing one cut does not establish another. Workspace package versions, schema and wire/ABI versions evolve independently. |
| Portable components | Core-only kernel and shared `no_std + alloc` embedded Agent; WASM, Linux libc-free host and C ABI examples. | These are bounded SDK/example targets, not a fully `no_std` CLI, native firmware or general board certification. |
| Packaging | Nix native and five-target release definitions, release manifest/checksums and platform installers. CI runs the product gate and a separate native build/version job. | Linux/macOS packages use Full; Windows packages use Minimal. Builds, native execution, signing and publication are distinct gates. |

The architecture and security contracts remain in the live [State](STATE.md), [Database architecture](DATABASE-ARCHITECTURE.md), [SDK](SDK.md), [CLI](CLI.md) and [retry/recovery](LLM-RETRY-RECOVERY.md) documents. Historical phase names do not authorize a new phase, schema change, tag move or parity claim.

## Validate the current candidate

The plan TODO is the record; tick an item only after running its named command.
The only shared gate is the product build and test:

```sh
bash scripts/bootstrap.sh
cargo fmt --all -- --check
cargo test --workspace --locked
node --test tests/web_model.test.mjs tests/web_workspace.test.mjs \
  tests/web_context_flow.test.mjs tests/web_multiplayer.test.mjs \
  tests/web_agent_parity.test.mjs tests/web_rich_markdown.test.mjs
```

A source check is not a compile result. A cross-built executable is not a native
execution result, and a local run cannot establish missing Windows/macOS hosts,
permitted live-provider use, signing or publication. Record the exact command
and result on the plan TODO line and leave unexecuted work open.

## Completion and remaining work

Keep one current checkbox per outcome in the appropriate plan TODO, linked from the [root TODO](../plans/TODO.md). After coordinator verification, record the test command/result and implementation commit; remove resolved duplicates. Temporary raw reports and logs are useful while diagnosing a run, but no historical evidence archive or byte ledger is required for an accepted checkpoint.

Keep blocked, failed, cancelled and unexecuted work open. In particular, release readiness must distinguish successful target builds from canonical assembly, actual execution of the shipped assets, permitted live-provider use, signing and publication. Do not replace those gates with a source-only report or a remembered earlier pass.
