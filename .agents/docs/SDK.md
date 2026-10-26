# Rust SDK

## CLI subprocess integration

SDK clients spawning aw now supply canonical long options only, including --provider-profile, --model-id, --provider-base-url, --state-memory-only, --tools-mode and RPC subcommand --agent-stdio. The standalone Rust and Python RPC examples were updated to use those exact aw arguments; their own example-client parameter naming is independent. All previous CLI long aliases have been dropped. The offline aw cli schema v2 is the supported machine-readable flag inventory, with NO old-name migration fields. No compatibility switch or argv rewrite helper is shipped.

The schema preserves typed Arg IDs, short flags, environment names (never values), defaults, accepted values, requiredness and headings for the compiled feature cut. CLI naming does not create tool, account or Session authority. Pin a repository revision for these pre-1.0 source libraries; current validation is listed in [release readiness](../plans/release-readiness/TODO.md).

---

## E5 — public versioned C ABI and independent native C11 consumer

New portable package aw-embedded-ffi (no_std+alloc; wasm32v1-none check) exports the 11 C ABI v1 functions declared by include/aw_embedded_ffi.h: ABI/layout inspection; create/start/model_step/tool_step/cancel; state and bounded answer/trace copies; explicit destroy. No C strings or mutable Rust references cross the boundary, and all host input labels/tool/source grants and the callback table are copied on construction. Future structs must honor exact tool-array stride and prefix-compatible config/callback sizes. No implicit allocator/panic, std provider, network, credential fallback or background effects in this portable rlib.

The independent C11 test under examples/sdk-embedded-ffi-staticlib proves ABI layout, read/write buffer bounds, two genuine model callback steps, one source-authorized tool effect, journal order and final ten events. It tests negative ABI shapes, callback reentry BUSY without violating Rust mutable aliasing, dynamic grant revocation, refused tool intents and uncertain tool receipts, and also conservatively blocks disposal of an ambiguous paid model callback after ModelStarted but before ModelSettled. The native C staticlib wrapper intentionally uses a standard C runtime and panic=abort; **not** an E4 libc-free final C executable or E6 firmware. Future true embedded C host owns allocator/panic and hardware/transport callbacks. See [E5 ABI README](../../crates/aw-embedded-ffi/README.md), [independent C link fixture](../../examples/sdk-embedded-ffi-staticlib/README.md).

---

## E4 — independently embeddable safe Rust Agent, native ELF libc-free host

E4 adds ONLY an independent example consumer, not a new SDK Engine. A real no_std+alloc aw-embedded Agent backed by the SAME aw-agent-kernel transitions as hosted aw-agent-core is built with a target-local panic=abort closure; the separate native Linux host supplies its own bounded allocator, panic handling, memcpy/memmove/memset/memcmp/bcmp compiler primitives and raw write/exit syscalls. Its ELF is linked without startup CRT, libc, dynamic loader or imports, audited and actually executes the independent E3 fake model/tool/journal path. Debug and release native executions and a fail-closed negative/panic witness pass. The safe aw-embedded/aw-agent-kernel public API and the full standard std-first hosted Agent are unchanged.

The example is under [sdk-embedded-native](../../examples/sdk-embedded-native/README.md) and its reproducible test is the removed aw_e4_native_libc_free_smoke helper. This is **Linux x86_64 native libc-free**, NOT x86_64-unknown-none firmware, an ARM board runtime, or an E5 C ABI SDK. A Rust extern C _start/host compiler symbol is not a public opaque aw ABI. The C ABI is documented separately above; firmware and additional native targets remain in [embedded platforms](../plans/embedded-platforms/TODO.md).

---

## R12 external RPC clients and bounded public Agent events

The std-only aw::headless::Driver calls the ordinary hosted Agent, never constructs a second provider/tool loop. Output and stdio RPC serialize bounded public event projections only, retaining actual provider-account custody and tool-origin restrictions in the host. A request ID is unique within the process but does not certify global durable idempotency. Separate accepted, delta/receipt and settled events make network or pipe ambiguity explicit; the exact saved Session must be inspected rather than automatically resending an uncertain prompt after reconnect.

The standalone Rust SDK package aw-rpc-client-example in examples/sdk-rpc-client depends only on serde/serde_json and standard IO, not aw-agent-core, provider or Session internals. Its typed client reports TransportUncertain on pipe loss and rejects bad version or nonmonotonic messages. A Python standard-library-only client independently implements the same protocol. Both are tested as external processes talking to real full and minimal aw binaries and isolated mock SSE providers.

Bounded recent-message and tool-result projections omit raw signed provider-private continuation, OAuth/SecretStore credentials and headers. --diagnostics-protocol only logs event names and frame lengths to stderr; no raw wire capture.

---

## R11 — host-owned project resource admission and existing Agent boundary

One aw::project_resources::Catalog exposes bounded source-pinned scan/verify/trust/revoke, ancestor instructions, named prompt templates, SKILL.md and strictly typed named themes; no new provider/tool loop, remote service, script engine or secret store is instantiated. Source discovery is confined to explicit canonical UTF-8 project root and current descendant working directory via platform scoped_fs Root, with incomplete directory/symlink/hardlink rejection and max 96 files, 512KiB aggregate, 64KiB per source and 24 ancestor levels. Secret-free ProjectResourceTrust metadata is merged by existing locked FileConfig CAS; no authority can be derived from a project file. A host must validate trust at admission and revalidate before each new prompt.

The full and JSONL-minimal CLI share ResourcesArgs (--resources-*), and all model-visible user prompts still run through the established Agent with current account/effect journaling. Templates interpolate only user-supplied input and up to 16 explicit NAME=VALUE variables. SKILL.md accepts bounded name/description frontmatter only. Native UI UiConfig.theme_accent optionally overlays one validated #RRGGBB accent on the established System/Light/Dark semantic palette; no second theme renderer. Full SQLite SecretStore/OAuth and minimal explicitly selected env API secrets retain existing custody; no homemade JSONL secret vault or subscription OAuth is claimed. Named resources are intentionally disabled for browser/owner/Workspace hosts pending a separate authenticated trust interface.


---

## R10 durable compaction and conservative provider-attempt SDK (2026-10-10)

CompactionPolicy (default Ask, Auto requires explicit paid authorization and a verified known destination window) produces a pure CompactionPlan whose complete public evidence is bounded and SHA256-bound to the current Session head, never provider-private thinking or a restored tool grant. Actual tool-free generation reuses the existing SummaryExecutor and a trusted SummaryJournal. SessionCursor optional backend-neutral compaction_journal/apply_compaction/compaction_ready returns fail-closed for unsupported backends. JSONL append-only begin/settlement/CAS preserves all raw TurnFinished data; successful existing summary can apply after restart without another provider call, unknown paid outcomes block inspection. Full SQLite retains existing ContextFlow. Both native full and JSONL-minimal CLI call the same aw::jsonl_compaction helper for JSONL mode.

AgentOptions.retry uses the distinctly exported PreDispatchRetryPolicy (0..=2 extra attempts; default 0), separate from the full SQLite store's own RetryPolicy. Every provider attempt has its own durable InvocationStarted/Finished ordinal. Error::BeforeDispatch from a TRUSTED Rust adapter positively attests no dispatch bytes, permitting an explicit bounded retry with exact same actor/model/account; generic Transport/Timeout/HTTP or streamed fragments NEVER satisfy that predicate. Current standard HTTP transports do not promise no-dispatch retries; the option is primarily for adapters with a real preflight guarantee. No auto-replay of tool effects or hidden model/account rerouting.


---

## Model-directed read and trusted Bash waves (R9 follow-up)

AgentBuilder::tool_policy(ToolPolicy) now defaults to resource ceilings of 16 concurrent reads/Bash Process effects and model-driven selection, with an absolute bounded maximum of 128. Model scheduling is signaled by optional per-tool boolean parallel:true, never an authority grant; omitted/false means serial. The same host-owned ToolPolicy and portable TurnMachine enforce absolute source grants, durable intent-before-effect and ordered factual receipts. Agent::set_max_parallel_reads and Agent::set_parallel_bash remain trusted SDK-level constraints; default Bash concurrency does **not** grant Bash execution, which still requires an explicit trusted host tool grant.

Source grant/actor/site/epoch and provider binding are rechecked after durable intent and before results are disclosed. Results and after_tool hooks are processed in proposal order. The canonical full/minimal native CLI uses --tools-parallel model|off, --tools-parallel-limit 16 by default and optional stricter read/Bash limits. Current native read schemas expose parallel:boolean for read, ls, grep and bread, and the trusted Bash/PowerShell schema also exposes it; mutations are serial
barriers. Current native schemas additionally expose dotlink's shell input
options, conditional atomic write preconditions and bounded discovery knobs. Earlier builds also advertised a separate find tool; recursive ls supersedes it. The alloc-only kernel still has zero Tokio/OS dependencies; aw-embedded stays explicitly host-driven and sequential. Result bytes are apportioned across the existing 1MiB context/trace cap and are explicitly marked truncated rather than rejecting all 16 small calls because of imaginary maximum-size outputs. See [accepted history](../plans/accepted-history/TODO.md).

---

## R8 real shared portable Agent turn — std and embedded (2026-10-10)

The full and JSONL-minimal standard aw Agent now enforces the exact bounded aw-agent-kernel::turn::TurnMachine for model-round ordering, duplicate tool-call IDs, whole-batch admission, journal-before-effect, current authorization after journal, receipt-before-continuation and unsafe-effect reconciliation. Its Agent::portable_trace() exposes the last turn's data-free ephemeral Event sequence for diagnostics/conformance; it never restores or persists permissions or model context.

A separate aw-embedded crate (--no-default-features --features alloc) is a real host-driven no_std+alloc adapter using **the same** TurnMachine, not a clone of the hosted loop or another ToolPolicy/credential engine. The host supplies exact actor/site/source/effect Grant, schema validation, authenticated model transport, synchronous tool callbacks, secure nonce, monotonic clock and durable append+flush; it explicitly advances start/model_step/tool_step and receives typed Progress, errors and bounded messages. Model/typed-tool work cannot begin on construction. Missing clock, entropy, journal or tool grant fails closed; unknown/write/process intent requires inspection after an ambiguous failure. The host must enforce timeouts inside its blocking port callbacks.

Independent aw-embedded-example is a no_std fake firmware consumer that executes an actual provider→tool→receipt→answer turn. the removed aw_r8_embedded_smoke helper compiles both core-only and alloc cuts for wasm32v1-none, links the final no_std module with a **temporary host-owned allocator/panic handler**, checks that it imports **zero functions**, and runs that module in Node WebAssembly. crates/aw-agent-core/tests/portable_embedded_parity.rs compares an independently driven embedded event trace with an actual standard Agent provider/tool/journal invocation and checks failed receipt no-replay. Tested compiler/target and module digest are reported separately; neither these tests nor static musl establish a production native ELF/C-ABI/cross-toolchain firmware certification. No std/Tokio/HTTP, SecretStore, SQLite or libc import is added to the kernel or embedded crate.

See [accepted history](../plans/accepted-history/TODO.md) and [independent no_std host example](../../examples/sdk-embedded/README.md). Hosted MCP/provider/UI/Session capabilities remain standard Rust; optional firmware work is tracked separately.

---

## Current R1 portable capability kernel (2026-10-09)

aw-agent-kernel is a core-only no_std library with an optional alloc tier and std as its default. It defines bounded borrowed capability descriptors (name/source kind/host-issued epoch/effect), immutable catalog validation and exact actor/site/source/revision/effect grants. The existing std aw-agent-core ToolRegistry and ToolPolicy consume **the same kernel predicate** for trusted-host and externally sourced Tool registration. This does **not** make aw-agent-core no_std; Tokio remains the standard async host.

Trusted Rust hosts may call Agent::register_source_tool(tool, ToolSource) without granting it. Agent::permit_registered_source_tool(actor, name, expected_source) grants only the exact registered origin/epoch/effect and current site; Agent::revoke_registered_source_tool removes that grant for subsequent work. Regular AgentBuilder::tool and register_host_tool remain explicit trusted-host actions. Source names and metadata never grant permission. ToolSpec is frozen when registered and used for both model disclosure and pre-effect receipts.

The independent `aw-kernel-embed-example` is a core-only capability consumer; `aw-embedded-example` separately drives the shared alloc TurnMachine. Neither example grants production host capabilities.

## R5a MCP SDK integration (2026-10-09)

Standard Rust consumers can depend on aw-mcp as an optional external-tool host layer. The official rmcp 3.5.0 client handles stdio/Streamable HTTP 2026/2025 negotiation, while aw-agent-core remains the only Agent ToolRegistry and effect/journal executor. aw_mcp::Options::parsed validates explicit server/grant selection; aw_mcp::inspect returns bounded ungranted names; aw_mcp::attach registers remote Tool objects through Agent::register_source_tool and permits only exact host-selected grants through Agent::permit_registered_source_tool. Local JSON Schema validation occurs before every tools/call; non-text/interactivity require future media adapters. All rmcp/transport/Reqwest crates remain std-only, never dependencies of core-only aw-agent-kernel. See [accepted history](../plans/accepted-history/TODO.md).

## R5b deprecated SSE transport SDK boundary (2026-10-09)

aw-mcp::Options adds --mcp-sse for explicit old MCP 2024-11-05 two-endpoint compatibility. The transport-only bridge feeds standard rmcp AsyncRwTransport and ClientLifecycleMode::Initialize with protocol version V_2024_11_05. The canonical Agent ToolRegistry/authorization/journal remain unchanged. Untrusted server-announced POST URIs must match scheme/host/port; transport/HTTP/TLS/stream tasks stay entirely in standard Rust aw-mcp. R5a modern HTTP and stdio remain independent and neither retries uncertain effects. See [accepted history](../plans/accepted-history/TODO.md).

## Hosted Agent and journal boundary

The core owns tool-free summary execution and ordered context; the store owns source plans, immutable artifacts and exact application. Account/identity and personal-device boundaries remain explicit. The core contains no HTTP, SQLite, filesystem or renderer implementation.

## Phase 11 — same API shape, tighter boundaries

Current code preserves the same headless Agent/provider/tool/journal architecture and wire/schema versions. The [accepted history](../plans/accepted-history/TODO.md) adds sticky sink errors, post-intent account/source revalidation, summary invocation consistency checks and post-hook publication revalidation. A callback rejection now remains a failure even if a custom Provider incorrectly ignores it. Chat invocation records can report `stream_rejected`; do not interpret that as completion or verified billing. Summary failures expose no partial candidate.

Standalone SummaryExecutor registrations now obey the same extension name/count/uniqueness rules as AgentBuilder. Invalid/duplicate names that previously slipped through the standalone path now fail at construction. Compiled hooks/providers/journals remain trusted code, not a sandbox; denying output cannot undo external effects.

No SQLite, HTTP, filesystem, IPC, browser, new model loop or credentials are added to aw-agent-core. The new validation/CI utilities are development tooling, not native startup dependencies. Run `cargo test --locked -p aw-agent-core --test hardening` on a real host. [Source-bound acceptance](../plans/TODO-SPEC.md) records precisely which compiler/consumer/fixture steps actually pass.

## Phase 10 / 0.11.0 — summary and ordered-context SDK

Core context dependencies provide SHA-256 utilities, not a database/HTTP/UI implementation.

`ContextBlock` identifies Import or Compaction and its insertion index, source node/artifact, text and exact dependency refs. `HistoryState` version2 retains completed/unsuccessful attempts and ordered blocks; legacy version1 must contain no blocks. The host obtains an authorized projection from the store, never from client-authored grant metadata. `TurnJournal` adds defaultable `validate_context`, `validate_context_as`, `additional_context` and `requires_output_gate` hooks; the SQLite journal implements source/account checks, one-shot consumption and derived-output fencing.

`SummaryInput` describes an immutable store plan, focus, materialized evidence, complete dependency manifest and bounds. `SummaryExecutor::execute(selection, input, journal, cancellation)` uses the **requester's** permitted account; it invokes no tools and no main Agent prompt. `Agent::summary_executor()` carries the existing trusted extensions. `Extension::before_summary` defaults to the before-prompt gate on the focus; `after_summary` can deny candidate publication. Native hooks are trusted and cooperative, not isolated arbitrary plugins.

`SummaryJournal::{authorize,begin,finish}` commits intent and outcome separately from conversation nodes. `InvocationContext.purpose` defaults to Chat and has a Summary variant binding the exact message digest and sources. Embedders adding explicit struct literals must set purpose; use the source examples and pre-1.0 migration notes. Provider and journal implementations must not infer cross-source permission from the presence of a destination ID.

Store APIs: `context_target`, `context_prepare`, `context_plan`, `summary_journal`, `context_generation_status`, `context_operation`, `context_manual`, `context_artifact`, `context_apply`, `context_application`, `context_pending`, `context_discard_pending`, and owner-only `context_grant_read`. Plan/operation/artifact types and `PrepareContext`, `Selection`, `ApplyContext`, `ApplyMode` are re-exported from `aw-store`. Applications perform SQLite management off the async renderer. After a summary, reload current history/next ID before another ordinary Agent prompt.

```sh
cargo run --locked -p aw-context-example
cargo test --locked -p aw-agent-core --test context_flow
cargo test --locked -p aw-store --test context_flow
```

The [independent consumer source](../../examples/sdk-context/src/main.rs) is a complete fake-provider/in-memory-SQLite example without the CLI/server. [Detailed contract](CONTEXT-FLOW-IMPLEMENTATION.md) and real-binary host harness define the required real-host evidence. None of these Rust commands were executed in the authoring environment.

## Libraries and boundaries

| Crate | Responsibility | Excludes |
|---|---|---|
| `aw-identity` | Bounded validated internal roster and qualified principal names; not a login or permission check. | Agent, HTTP, SQLite, filesystem, Tokio and UI. |
| `aw-agent-core` | Sole model/tool loop; accounts and extensions; journal/events; InvocationContext and delegated-adapter declaration. | HTTP, files/processes, SQLite, IPC, UI and provider keys. |
| `aw-providers` | Concrete Ollama/OpenAI-compatible local HTTP/config/secret resolution. | UI/store/remote key upload. |
| `aw-inference` | Explicit Bridge/DeviceWorker, mutual link auth, finite grants, private ledger and stream schema. | Concrete HTTP providers, model tool execution, SQLite, UI. Includes deliberate local file/Unix I/O. |
| `aw-client` | Typed same-UID owner client and bounded framing. | Agent loop, provider config, tool execution, database and renderer. |
| `aw-tools`, `aw-store` | Existing explicit effects and durable owner history. | Secret restoration from transcript. |
| `aw-workspace` | Pure layout/editor model. | Execution/transport implementation. |
| `aw-git`, `aw-terminal`, `aw-surfaces` | Existing human review/terminal services and controlled surface composition. | Another model loop or inference account resolver. |

`aw-cli` composes these into the same binary. The `device` feature is compiled by default but starts no listener/worker for ordinary inline usage. A headless embedding application can link core without HTTP/device crates, or explicitly supply its own provider and independent journal.

## Identity-aware provider invocation

```rust
use aw_agent_core::{InvocationContext, DelegatedBinding};

// InvocationContext supplied by the native loop, not an untrusted UI field:
// scope: Option<InvocationScope { session_id, branch_id }>
// turn_id, ordinal, identity: InvocationIdentity
// DelegatedBinding { account_id, executor_id }
```

`Provider::invoke(context, messages, model, tools, sink)` defaults to the prior `stream_tools` implementation. Local providers need not implement a new transport. A delegated Bridge overrides it and refuses unscoped legacy `stream` calls. The actual session journal's `invocation_scope()` defaults to None for old/custom memory hosts; the SQLite handle returns its stable session and branch. The Agent records the existing model-start intent before polling the provider.

`ProviderAccounts::register_delegated(descriptor, adapter)` is a trusted host API: it validates a personal/user-device descriptor, verifies the adapter's declared account/executor match and refuses local-executor labels or replacement accounts. `Provider::delegated_binding()` is a declared native contract, not cryptographic proof. The ordinary register path rejects remote or delegated handles rather than silently treating them as locally held secrets.

Each submitted prompt still uses `ProviderAccounts::select(actor, account, model)` and `Agent::prompt_as`. A host must authenticate and authorize the actor/session/context first. Shared history and peer co-editing confer no provider authority. Existing normal registration/revocation and before-prompt/tool hooks remain.

## Owner embedding

Inside a running Tokio context, a trusted host can explicitly load a private link, bind the private listener and register its adapter:

```rust,no_run
# async fn attach_personal(mut_agent: &aw_agent_core::Agent, path: &std::path::Path) -> aw_agent_core::Result<()> {
let link = aw_inference::LinkConfig::load(path)?;
let (bridge, listener) = aw_inference::Bridge::bind(&link, mut_agent.actor())?;
mut_agent.provider_accounts().register_delegated(bridge.descriptor()?, bridge)?;
let stop = aw_agent_core::CancellationToken::new();
// A production host retains the stop token and JoinHandle and settles shutdown.
let _task = tokio::spawn(async move { listener.run(stop).await });
# Ok(()) }
```

The adapter has no provider key or endpoint. It enqueues only within a live authenticated device session. A missing/disconnected worker is a failure, not another account. Model/tool effects remain in the existing Agent; the Bridge returns completed proposals, not privileged instructions.

## Device embedding

```rust,no_run
# async fn device(provider: std::sync::Arc<dyn aw_agent_core::Provider>,
#     grant: &std::path::Path, receipts: &std::path::Path,
#     local_profile_digest: &str, socket: &std::path::Path,
#     key_file: &std::path::Path) -> aw_agent_core::Result<()> {
let mut worker = aw_inference::DeviceWorker::open(
    provider, grant, receipts, local_profile_digest,
)?;
let key = std::sync::Arc::new(aw_inference::private::key(key_file)?);
worker.connect(socket, key, aw_agent_core::CancellationToken::new()).await
# }
```

The application injects a locally configured provider, including the requested output-token cap. A custom Provider is trusted code; DeviceWorker cannot prove it honored that cap or where it physically ran. The CLI performs local profile/grant consistency checks and never constructs a provider from incoming link JSON.

A private ledger owns its lock for the worker lifetime. Receipt metadata can be inspected without executing anything. The optional ready observer runs after reciprocal handshake and supplies no prompt, secret or authority; it cannot serve as a policy gate or attest billing. `run_stream` is an advanced confidential-transport/test seam, not permission to put this protocol on unencrypted public TCP.

## Continuation, failure and consumer compatibility

Private provider continuation stays on the device behind a bounded opaque same-turn/public-block handle. It is neither stored as normal history nor leaked as a model explanation. A missing/restarted continuation fails rather than recreating one under another model/account. Context size is rechecked with the restored private data before dispatch.

`InvocationContext` and `TurnJournal::invocation_scope` are additive source APIs. New strict serialization fields/types and new default features are pre-1.0 changes; pin the repository revision. Existing `Turn` usage/outcome remains provider-reported and can be unknown. A Result::Ok Turn can still be Failed/Cancelled/Incomplete. Process/client disconnection does not imply remote vendor cancellation or external rollback.

The independent `aw-inference-example` links core + inference but not app/provider/tools/UI. No arguments initialize no transport. `--run-fixture` explicitly runs an in-memory fake provider plus a private test ledger, never an LLM or an external model endpoint.

```sh
cargo test --locked --workspace
cargo run --locked -p aw-inference-example -- --run-fixture
python3 the removed check_sdk_boundary helper
cargo doc --locked --workspace --no-deps
```

Consult the release-readiness TODO for executed and unfinished gates. [Device contract](DEVICE-INFERENCE.md) · [Protocol](DEVICE-PROTOCOL.md) · [Local owner/client](LOCAL-OWNER.md) · [State](STATE.md) · [Git workflow](GIT-WORKFLOW.md).

## Phase 9 / 0.10.0 — identity SDK and private participant controllers

`aw-identity` adds validated `Roster`/`Participant` names and `internal:<realm>:<id>`. It depends only on Serde/thiserror; no Tokio, Agent, authentication transport, HTTP, filesystem, subprocess or database. `Roster::principal` maps a trusted directory entry; it does NOT authenticate a user. The independent `aw-identity-example` prints a labeled fixture and opens nothing.

The optional application API `aw::web::team::serve(roster, Vec<ParticipantRuntime>, address, stop)` owns the actual invite/cookie gateway. Each supplied runtime must match its roster principal and owned persistent session and have no exposed tools. The stock CLI factory supplies no arbitrary extensions or surfaces. Native embedding code remains trusted and is not sandboxed by a roster type.

The core now supports `AgentBuilder::delegated_account(descriptor, actor, owner_executor)` for an explicitly nonlocal personal default. It uses `register_delegated` validation, not an arbitrary execution-location label or fallback provider. `set_model` on that path selects only pregranted models and cannot widen or revive a revoked account. Existing `account()`/local-provider behavior and the one shared model/tool loop remain.

Per-person controllers have separate `Agent` state and registries while sharing the existing SQLite owner and, where explicitly configured, common stateless HTTP provider clients. Request cookies select the participant before any actor-bound handler; no prompt field chooses identity. History/default account remains private in the first team mode.

```rust
use aw_identity::{Roster, Participant};
let roster = Roster::new("engineering", vec![
    Participant { id: "alice".into(), display_name: "Alice".into() },
    Participant { id: "bob".into(), display_name: "Bob".into() },
])?;
assert_eq!(roster.principal("alice")?, "internal:engineering:alice");
// A name lookup is not authorization to act as Alice.
# Ok::<(), aw_identity::IdentityError>(())
```

```sh
cargo test --locked -p aw-identity
cargo run --locked -p aw-identity-example
cargo test --locked -p aw-cli --no-default-features --features team
python3 the removed phase9_smoke helper --binary target/release/aw
```

These Rust/real-binary commands require host execution. [Team identity](TEAM-IDENTITY.md) · [Protocol](TEAM-PROTOCOL.md) · [Build status](BUILD-STATUS.md).
