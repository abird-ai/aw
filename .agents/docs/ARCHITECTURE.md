# Architecture

Source 0.15.0; SQLite v33 and JSONL v1. Current completion/pending work is owned by the [plan TODO index](../plans/TODO.md).

## Typed CLI

The same source-derived Clap CommandFactory and FromArgMatches is authoritative in full/minimal/stateless. The recursive typed Arg metadata projection sets category-prefixed long names but registers NO previous long names as aliases; the six older tool alias spellings were removed from both CLI derives. No feature flag selects compatibility mode and no migration tool exists. The public offline aw cli schema v2 exposes ONLY supported names, typed ID, env name, defaults, possible values and nested paths. One-shot CLI positional disambiguation uses canonical Clap metadata only, preserving plain command words as prompts without silently launching a subcommand.

CLI metadata does not create a second Agent/tool loop, provider custody, Session authority, source grant, effect or replay path. Full tree construction and parsing use a bounded 32MiB stack worker; reduced cuts parse directly. Current unfinished validation is in [release readiness](../plans/release-readiness/TODO.md).

---

## E5 — versioned C ABI for the SAME portable embedded Agent, no alternate state machine

E5 adds crates/aw-embedded-ffi, a portable no_std+alloc rlib containing isolated carefully documented unsafe C pointer glue around the exact original aw-embedded::EmbeddedTurn. It does not modify aw-agent-kernel, aw-embedded or the standard hosted Agent. The same TurnMachine enforces grant/source epoch, journal intent-before-effect, exact tool receipt and uncertain-effect no-replay. A source-bound multi-tool registration vector (up to 16 explicit host grants) is copied and validated at creation; the host tool_authorize callback must approve exact actor/site/name/source-kind/source-epoch/effect on every registration and is checked AGAIN after journal intent before effect. No untrusted model callback authorizes a shell/network effect.

ABI v1 public header crates/aw-embedded-ffi/include/aw_embedded_ffi.h uses version+size fields, uint32 status/event codes, pointer+length borrowed bytes, opaque single-owner handles and a separate disjoint Atomic BUSY gate to reject reentrancy before mutable EngineState aliasing. All config/source/identity strings copied into immutable Rust-owned storage. Callbacks are synchronous, never retain pointers, and fill borrowed output buffers with explicit cap/written; answer and trace APIs copy into host-owned buffers and return exact required sizes. C host owns its context/clock/secure entropy/durable append+flush/paid model transport and tool execution. If a committed ModelStarted is not committed ModelSettled after an error, E5 marks reconciliation required conservatively; existing kernel separately tracks unknown tool intent/receipt. Destructor refuses to discard either ambiguity without explicit host choice. No implicit retries or provider grants.

A separate examples/sdk-embedded-ffi-staticlib std packaging wrapper builds a native C-test-linkable staticlib with panic=abort. The actual independent C11 executable passes positive and fail-closed tests and dynamically links libc; this does NOT change E4's independent native Linux libc-free ELF proof, nor produce a native bare-metal C firmware library. The portable C ABI rlib passes wasm32v1-none no_std+alloc compile. E6 firmware/real board/target transport and non-Linux C consumer certification still future. Complete decisions [accepted history](../plans/accepted-history/TODO.md), [portable ABI contract](../../crates/aw-embedded-ffi/README.md).

---

## E4 — actually executed native Linux libc-free aw shared Agent, NOT standalone firmware

E4 follows E1–E3: aw-agent-kernel::turn::TurnMachine is the ONLY transition/authority/effect state machine in both hosted aw-agent-core and independent no_std+alloc aw-embedded. Its existing independent aw-embedded-example fixture supplies exact model/tool/journal host callbacks and confirms two genuine fake model requests, source-bound grant, one tool, intent-before-effect/receipt, ten kernel events and final answer.

New examples/sdk-embedded-native/host.rs links this SAME E3 fixture as a native x86-64 Linux NO_STD/NO_MAIN static ELF using an explicit stack-aligned _start shim, host-owned 4MiB bounded allocator, full compiler memory intrinsics, fail-closed panic/personality handler and ONLY raw Linux write/exit syscalls. No crate or provider/Tokio/SQLite/network/tool implementation is changed; the independent host owns all unsafe code. Final linked ELF is structurally audited with readelf/nm (NO PT_INTERP, PT_DYNAMIC, DT_NEEDED, undefined imports or relocations) AND ACTUALLY EXECUTED under Linux. Debug and optimized release real images succeed and a separate deliberately denied witness + panic-abort image fail closed. The final ELF is libc/CRT-free; the normal full/minimal CLIs stay std and hosted.

The compiled Rust target is x86_64-unknown-linux-gnu with explicit static no-CRT linker flags and owned compiler-memory symbols; this demonstrates **libc-free final native ELF**, NOT no-kernel/firmware support. Linux syscalls/process entry still required. Additional freestanding/board targets remain separately gated in [embedded platforms](../plans/embedded-platforms/TODO.md). Static-musl is distinct from libc-free linking. See the [independent host](../../examples/sdk-embedded-native/README.md).

---

## Validation boundaries

R13 adds no second Agent, provider loop, permission/custody system, or schema. the removed aw_r13_certify helper executes actual full, minimal and stateless binaries against first-party independent mock-provider, signed Anthropic/Gemini, MCP, process extension, PTY and Rust/Python RPC fixtures; statuses distinguish passed_local_mock, failed, not_run and known_open_unverified and record binary SHA/source. the removed aw_r13_portability helper checks true Cargo normal-dependency exclusions, hosted ELF PT_INTERP, original release artifacts and zero-import, actually executed E3 embedded WASM. the removed aw_r13_benchmark helper measures warm-cache startup, latency distributions, stripped/gzip footprint, RSS and near-cap bounded rejection on local fake SSE, not live vendor or original aw timing.

Current outcomes and unfinished platform/security requirements belong in the [plan TODOs](../plans/TODO.md), not a second status list. Fake providers and structural audits certify only their tested boundaries.

---

## R12 — headless JSONL and typed RPC over the same hosted Agent

The std-only aw::headless emitter and RPC Driver adapter project existing aw-agent-core AgentEvent and Session state into versioned newline-delimited JSON. The full and JSONL-minimal hosts continue to own the exact same Actor, Provider, Model, Tool source/epoch/site permissions, durable intent-before-effect journal, cancellation and existing provider/tool loop. No parallel RPC-specific Agent implementation, network listener, implicit Shell grant or background effect replay is introduced.

A single-event JSONL --output-format jsonl --output-print session and aw rpc --agent-stdio both serialize typed started/delta/tool/attempt/finished events, with distinct accepted and settled RPC receipts. A missing settled result or lost process pipe is unknown; clients must inspect the exact Session ID before any new effect. An input thread only signals the existing Agent CancellationToken for a matching abort. Strict per-method parameter allowlists and bounded request/event/frame sizes reject untrusted authority injection. Credentials, Authorization headers, native signed thinking and raw provider-private continuation are never projected by the public protocol.

The independent Rust SDK and standard-library Python example under examples/sdk-rpc-client are external clients without aw Agent internals. Diagnostics are explicitly opt-in event-name/frame-size metadata on stderr only. Original SQLite ContextFlow, JSONL R10 compact and R11 project trust are reused, with direct privileged RPC Bash/extension operations deliberately unsupported.

---

## R11 — one explicitly approved project data resource boundary (2026-10-10)

The R11 local source trust workflow uses the same aw hosted Agent, FileConfig locked-CAS private storage, descriptor-scoped file browsing and TUI command registry; no project-resource discovery or template ever becomes permission to select a provider/account, execute Bash, enable external tools/MCP or rewrite existing Session custody. The user runs --resources-trust-project ROOT, binding canonical root to a bounded sorted SHA256 source snapshot. Only explicit --resources-project ROOT loads current matching resources, whose contents and provenance are checked again before subsequent prompts. Durable Session resumes compare exact resource-bearing system bindings rather than restoring a grant from saved conversation history. Underlying tool source/epoch and model provider-bound context checks remain unchanged.

New typed local resources are ancestor AGENTS.md/AGENTS.override.md/CLAUDE.md, named user-prompt templates, explicitly selected SKILL.md and semantic RGB named themes. The same native TUI semantic palette function projects these themes into Inline/Scrollback/Fullscreen and native Workspace; project-resource authorization is not available in browser/web owner/Workspace mode. Minimal env-key references and full SecretStore/OAuth retain prior credential-custody responsibilities.


---

## R10 compaction and no-dispatch retry architecture (2026-10-10)

The existing aw-agent-core Agent remains the only provider/tool loop. New pure CompactionPolicy and SHA256-bound CompactionPlan select complete old-turn evidence for the existing tool-free SummaryExecutor; the backend owns durable paid intent, summary receipt and CAS application. Both std full+JSONL-minimal use one shared aw::jsonl_compaction helper for the JSONL backend. The full SQLite backend still owns its existing authorized ContextFlow prepare/generate/apply workflow. JSONL retains every original append-only TurnFinished record but shows only bounded active complete recent Turns plus a labeled unverified model-derived summary with exact source digest and compacted-through cursor. JSONL retains finite raw file limits (32MiB). A successfully settled-but-unapplied summary is applied from journal on restart without regenerated billing; an uncertain paid attempt blocks until user inspection. The SummaryExecutor alone invokes a provider; compaction never imports credentials or grants from disk.

Provider attempts are separately journaled and bounded to 32 per turn; default automatic extra attempts ZERO. A host can allow 0–2 extra retries only for a trusted adapter's positive BeforeDispatch attestation (no vendor bytes dispatched, no partial stream). HTTP 429/503, Timeout, generic Transport and partial SSE are always ambiguous and never silently retried; no tool effect replay or changed actor/account/model. No std/Tokio or native DB is added to the portable no_std Agent kernel.


---

## R8 implementation — one portable Agent kernel, two host classes (2026-10-10)

The original E1 core-only Source/Catalog/Grant stays no_std without alloc. Its new alloc-only TurnMachine provides the shared deterministic provider/tool transition authority, and the existing std Agent now **uses that machine on real turns**. aw-embedded is the opt-in no_std+alloc host adapter; it uses the same machine with explicit clock, secure entropy, authenticated model, durable journal, exact source-bound grant, schema and tool ports, not a second independent policy/turn loop. A true WASM final image linked with a temporary host allocator and panic handler has zero imports and executes a fake tool-bearing turn. No std libraries, libc wrappers, TLS/database substitutes or negative no_std compile flag have been introduced in that embedded dependency path.

The normal aw CLI remains standard Rust: Tokio, Reqwest, standard filesystem/terminal and SQLite/JSONL are preferred for the full/minimal product. aw-agent-core::Agent::portable_trace returns only most recent data-free transition events and is cleared across restore/reset; it is not a grant or persisted replay plan. Optional firmware/transport work is listed in [embedded platforms](../plans/embedded-platforms/TODO.md).

## Hosted composition

The storage boundary has two explicit, mutually exclusive modes: owner/service uses an exclusive `owner.lock`; `SessionStore::open_trusted_local_direct` uses a shared `owner.lock` for trusted local peers on supported local filesystems. Trusted-direct is exact-current-schema, heartbeat/store-generation fenced, and never migrates or restores. Remote clients still use the owner/service protocol. See [DATABASE-ARCHITECTURE](DATABASE-ARCHITECTURE.md) and [STATE](STATE.md).

## Phase 8 composition

One Agent invokes a selected Provider with an immutable InvocationContext containing durable session/branch/turn/round and account identity. Local adapters use the default invoke behavior. A Bridge enqueues into a live authenticated device connection; only the device resolves its local profile and private model credential. The same owner continues to validate/execute its granted tools, journal, and publish the result. No per-view second Agent or automatic fallback is created.

`aw-inference` adds finite local consent, private atomic receipt storage, protocol/HMAC and private Unix client/listener plumbing with an injected Provider. It does not contain aw-providers, aw-tools, SQLite, HTTP, browser or native renderers. Device shutdown/revocation stops future admitted use; unknown effects and billing remain explicit. [Detailed source contract](DEVICE-INFERENCE.md). Git and full-history bundles now replace the archive workflow.


```text
CLI / private paired web owner
   current config: identity + provider handles + tool grants
   persistent host controller: selected session / branch / root / disclosure binding
                      ↓
              aw-agent-core Agent
       bounded completed context + optional checkpoint
       prompt/account/hooks → journal.begin
                      ↓
       model-start receipt → aw-providers HTTP
       model result and typed tool proposals
                      ↓
       grant/argument/hooks → tool-start receipt → aw-tools/custom Tool
       recorded tool result → result gate → next model round
                      ↓
       journal.finish → immutable turn + branch + matching web receipt
                      ↓
       memory projection / CLI text / browser snapshot and events
```

`aw-store` supplies SQLite and `TurnJournal`; the core contains only the trait and data structures. A memory-only consumer does not need to link the store. CLI is journaled by default on first real prompt, with explicit `--state-memory-only` preserving memory-only operation. Web starts a persistent owner unless deliberately disabled. No Git scan, model discovery, browser credential storage or provider call is needed for `--help` or listing saved metadata.

At the Phase-3 baseline, the process had one Store connection behind a mutex and an exclusive store-owner file lock. Journal writes run through a blocking worker; the web serializes admission/session switching with a separate short control mutex and does not hold the snapshot mutex across provider/SQLite waits. Individual views do not own model execution. Later Phase-35 work added the explicit trusted-local shared-lock mode described in the current ownership note above; ordinary owner/service mode remains exclusive.

Node ancestry is immutable; branch heads are revisioned references. Each native model/tool start is durable before effect; complete turns and web receipts settle atomically. Failed restoration or stale branch aborts rather than silently changing context. Private provider continuation is excluded from persistence. Current actor, account, payer, inference and tool execution locations remain separate.

Only the active ancestry is copied into bounded memory. A human checkpoint shortens future projection without deleting old nodes or invoking a summarizer. Session lifetime limits and view/export bounds are explicit; no hidden pruning or unbounded dashboard initialization.

[State and recovery](STATE.md) · [SDK](SDK.md) · [Web protocol](WEB-PROTOCOL.md) · [Credential placement](CREDENTIAL-PLACEMENT.md) · [Tools](TOOLS.md). All source/runtime validation limits are recorded in [BUILD-STATUS.md](BUILD-STATUS.md).


## Phase 4: optional native presentation and pure layout SDK

`src/workspace/` adds the Ratatui/crossterm renderer around the existing local Host/Agent. The only model/tool loop remains in aw-agent-core. Native events feed a bounded coalesced view snapshot; one owner thread handles current account and durable-session operations, and a separate lazy read-only worker serves explicit human file views. Layout save/load uses a blocking worker. No model/tool invocation is caused by rendering.

`aw-workspace` is an independently reusable, UI-neutral library with serialized layout/tab descriptors, atomic validation/reducer, screen rectangles and bounded grapheme editor. It has no Agent, Tokio, HTTP, filesystem, SQLite or Ratatui/crossterm dependency. A separate `aw-layout-example` consumer tests that intended embedding shape. Actual graph/build evidence requires Cargo.

The shell has one current conversation; additional tabs are views. Session/branch changes preserve current binding/grants and require an empty draft and idle run. Store access retains the explicit owner/service versus trusted-direct mode boundary. See [workspace contract](WORKSPACE.md), [SDK](SDK.md), [current plans](../plans/TODO.md).


## Phase 5: concrete Git and terminal services

The existing `aw-workspace` Target set adds Diff and Terminal opaque resource IDs. `aw-git` provides synchronous bounded Git operations; `src/workspace/git.rs` runs them on a lazy human-read worker, preserving each pane's mode/scope/filter. Whole-file index apply is sent to the same native owner command queue that serializes agent work. It is explicit human activity, not a model tool or newly durable journal.

`aw-terminal` owns a real PTY and bounded emulator behind a nonblocking reactor. A native Pool is retained in Host independently of layout tabs, created on workspace entry but launching no shell until requested. Each actual terminal has a root/actor, screen/input limits, cancellation/reap state and one focused local input owner. The renderer maps cells/styles instead of forwarding raw control streams. Resource IDs restored from a layout are not launch descriptors.

The current Agent/root remains fixed while a Git pane browses a different checkout; new terminals can use that selected review scope. Provider accounts, payer/custody, read/edit/bash model grants and durable history remain untouched. Human terminal commands have separate OS authority and may race with model work; no sandbox or distributed lock is inferred.

[Git](GIT.md) · [Terminals](TERMINALS.md) · [Workspace](WORKSPACE.md) · [SDK](SDK.md).

## Phase 6 — optional browser workspace and surface SDK

Browser groups and human service endpoints use the existing paired web owner; they do not introduce another Agent executor.

```text
browser conversation or explicit workspace UI
  existing Agent snapshot/prompt API → existing Agent / accounts / journal
  typed human surface API → bounded blocking worker → aw-surfaces
                                             ├─ aw-tools human read-only browsing
                                             ├─ aw-git cached review handles
                                             └─ aw-terminal canonical PTY resources
  registered Website iframe → browser-requested external site, not owner proxy
```

`--web-workspace` is an explicit host grant. The owner creates the capability registry without scanning Git/spawning shells; browser JS/CSS imports on entry, service work on demand. --web-allow-terminals adds a separate unsandboxed shell grant. Up to4 blocking tasks and one surface-resource mutex are independent of the Agent's provider loop. Edit/read authority is checked at the transport, after queue lock and before response disclosure.

The browser's generic groups use stable view identities and a stable surface DOM layer, preserving conversation input and iframe state while geometry changes. Only visible terminal views poll canonical snapshots; no persistent metrics/datasource timer is invented. Closing a view releases UI demand; the owner drains PTYs. Logout/expiry/shutdown requests resource termination. See [browser contract](BROWSER-WORKSPACE.md), [surface SDK](SURFACES.md) and [web protocol](WEB-PROTOCOL.md).


## Phase 7 source addition

`aw-client` supplies a typed Linux Unix-socket client and bounded codec without agent/provider/store/HTTP/UI dependencies. `src/web/native.rs` is a server adapter sharing the existing Session handlers with an explicit kernel-peer-validated local authority. `src/attach.rs` is a small inline client dispatched before local config/store access. `aw agent` holds the one existing Agent and journal; optional browser transport is not a second executor.

The first shared owner is same-UID, one active execution lane. Inspect is an independent authorized history read. Team identities and explicitly delegated device inference keep their separate authenticated boundaries; remote native fullscreen rendering is not implied by attach. Minimal local chat creates no socket or attach runtime.

[Owner](LOCAL-OWNER.md) · [Protocol](LOCAL-PROTOCOL.md) · [SDK](SDK.md).

## Phase9 composition

The existing per-controller web/Agent implementation is reused, not duplicated. A new invite-only gateway chooses the immutable participant AppState from a verified random session cookie. Every member has its own agent/persistent view/accounts/admission/stream projection. The shared `SessionStore` continues to have one owner process; common model transport clients can be reused without sharing conversation context.

`aw-identity` is a pure naming/directory library. `src/team.rs` is trusted CLI configuration, explicit default-account construction and fail-closed config watching. `src/web/team.rs` is authenticated bounded routing; `src/web/team/tests.rs` supplies router/Agent/store fixture source. Team startup is opt-in; ordinary inline mode performs no new account/watch work. No tools or human surfaces are exposed in team mode.

`AgentBuilder::delegated_account` permits an explicit checked personal-device default. Existing default-local registration and `prompt_as` retain their authority semantics. See [identity](TEAM-IDENTITY.md), [protocol](TEAM-PROTOCOL.md), [SDK](SDK.md) and current [build limitations](BUILD-STATUS.md).

## Phase 10 context flow composition

Core adds ordered context blocks and a tool-free SummaryExecutor with injected SummaryJournal; store adds pinned plans, artifacts, source-release dependency checks and atomic application. App context_flow composes both and is called by CLI, native workspace, shared-owner attach and browser. The same account registry, current grants and durable invocation identity apply; no second model/tool agent loop is introduced.

Preparation/application are bounded store transactions; provider calls are outside the branch transaction. Summary output is buffered until complete and authorized. Later ordinary runs with source-derived context use journal-specified output fencing and source/account checks. Device wire2 binds summary purpose/input/source digests to finite local consent. All native/database/network/front-end boundaries remain separated; [the SDK consumer](../../examples/sdk-context/src/main.rs) links no app/UI.

## Database, sync, identity and credential plane — adopted target

The product architecture now explicitly treats storage as immutable durable facts plus tiny mutable refs/policies, separate external-effect leases, a durable semantic `commit_log`, and a first-class identity/auth plane. One SQLite/WAL file initially backs most durable roles; logical Content/Ref/Policy/Identity/Auth/Audit/Sync/Receipt/Lease/Migration interfaces preserve a later physical split without forcing one now. Raw shared secrets are outside normal content tables behind a `SecretStore`; personal provider credentials remain on their user-controlled executor.

This section originally described trusted-local multi-process access as a future target. Phase 35 subsequently shipped that bounded mode after branch CAS, durable conflicted-result preservation, execution/process leases, migration coordination and adversarial WAL/recovery proof were in place. The outer `owner.lock` was deliberately retained as the ownership-mode fence: owner/service takes it exclusively; explicit trusted-local direct peers take it shared. Direct mode remains local-filesystem-only, exact-current-schema, no-migrate/no-restore, and remote clients continue through owner/service. See [DATABASE-ARCHITECTURE](DATABASE-ARCHITECTURE.md).

## Machine, Fleet and virtual workspace direction — design amendment

The workspace execution plane now has first-class **Machine** and **Fleet** targets. The main host is itself a Machine; remote machines run `aw agent` and establish an authenticated outbound MachineLink exposing only typed registered workspace capabilities. A fast workspace Target picker selects Local, a Machine, or a Fleet. Follow-target Files/Git/terminal/log/preview views rebind together so a remote machine feels like the local workspace, while pinned tabs and already-running sessions keep their explicit durable target.

A **FleetWorkspace** is a virtual workspace definition stored on the main host. It references a Fleet and resolves a registered MachineWorkspace for each member; member filesystem paths need not match. Selecting the Fleet shows aggregate machine-aware projections, while selecting one member drills the same virtual workspace down into that machine. Fleet operations pin the exact membership/binding snapshot they execute against, and durable per-machine `MachineRun` records preserve partial/unknown/retry state. Fleet grouping grants no credentials or tool authority and does not create a merged writable filesystem.

See [Machines/Fleets](FLEETS.md) and [adaptation matrix](FLEET-ADAPTATION-MATRIX.md). Current capability availability is defined by those contracts and the compiled feature cut.


## Current consolidated hierarchy and networking direction

The current accepted design uses **Domain → Workspace → Session** for durable ownership. Every AwNode has a reserved `local` HostedWorkspace. FleetWorkspaces are virtual Workspaces authoritative on one Workspace authority set; remote delegated work normally lives in durable PeerWorkspaces on the remote AwNode. See [DOMAIN-MODEL](DOMAIN-MODEL.md).

AwNet is a thin decentralized AwNode connectivity/data plane with federated Domain control. The peer protocol is QUIC-native with direct paths preferred, QUIC relay fallback, and TLS/TCP :443 relay fallback for restrictive networks. WireGuard is not required for core aw functionality. Managed AwNet and self-hosted `aw net serve` use the same protocol/semantics; Domain federation survives individual route/tunnel changes. See [AWNET](AWNET.md).

These additions do not create another agent engine, another Session tree or another distributable binary. `aw agent`, `aw net ...`, local/workspace/web/device modes are roles of the same `aw` executable.


## Phase 41 — bounded subagent child Sessions

Phase 41 keeps the existing Agent as the only model/tool loop and adds an application-owned `subagent` Tool/service for the native single-user owner. A child is not an in-memory pseudo-task: it is an ordinary durable Session in the same Workspace plus one secret-free `subagent_sessions` relation to the exact parent Session/branch/head/revision/turn. The relation persists the exact already-authorized `InvocationIdentity`, task digest, foreground/background mode, model-only tool mode, lifecycle and bounded error state.

`ToolContext` now carries the exact invocation identity and optional durable invocation scope already authorized by the Agent. This is evidence, not a new grant. The service reselects the same account/model and requires exact identity equality before child admission. `Agent::builder_from_selection` creates isolated child conversation state around that exact selected lease; child dispatch uses `prompt_as` with the pinned selection so a provider-account/grant revision change fails closed instead of silently changing payer/custody/model.

With host-selected child tools `off`, children receive no local file/shell tools. Recursive delegation is separately bounded by the root max-depth policy below. A model-only child may coexist with its active parent on the same physical root only through the reviewed synthetic internal run key after the store proves the durable relation is running, the parent run is exactly active on that physical root, identity matches, and no unrelated root hazard/run exists. Ordinary Sessions keep the physical root key and the historical root-execution fence remains unchanged.

Foreground children share parent cancellation and return bounded child answer data through the parent tool result. Background children are bounded in-process worker threads with their own current-thread Tokio runtime; the durable child Session is the source of truth. Owner restart marks admitted/running relations interrupted and never automatically replays the child provider call. Native Sessions Explorer and the floating Agent-side family switcher are projections of those durable relations. Web/shared/trusted-direct surfaces do not silently gain spawn authority.


## Phase 42 — isolated subagent execution roots

Phase 42 keeps Phase-41 child Sessions/identity/lifecycle unchanged and adds a host-owned execution-binding layer for optional child file tools. `--subagent-tools` is resolved by the native owner, never by model arguments. `off` preserves the Phase-41 model-only child. `read-only` and `workspace` first create a detached managed Git worktree at the parent's exact clean committed HEAD, then atomically bind the child Session working directory/tool root into that worktree before child inference. `trusted`/bash is rejected because the current shell tool is not a sandbox.

The new v26 `subagent_execution_bindings` state machine separates external Git resource effects from conversation state. `planned` is durable before `git worktree add`; verified allocation moves to `ready`; child creation consumes `ready -> bound`; explicit human cleanup records `cleanup_planned` before Git removal; uncertain creation/removal becomes `unknown` and is never automatically replayed. Child Session/history survives worktree cleanup.

Tool-enabled children use the ordinary physical root derived from their isolated SessionBinding and therefore the ordinary durable tool-effect hazard model. Only Phase-41 `off` children may use the tightly scoped same-root synthetic model-only run key. Git-management authority and shell are never child grants; the separately bounded recursive policy below governs delegation.


## Phase 43 — durable delegation root envelopes

Phase 43 keeps Phase-41 child Sessions and Phase-42 isolated execution roots unchanged, but makes their admission and child model calls consume one durable root envelope scoped to the exact parent Session/branch/turn. Host-selected limits bound total child allocations, active children, child model attempts and elapsed deadline; the default depth is 1 and the host-selected recursive ceiling is 1..4 as described below. No child can create its own budget authority.

`BudgetJournal` wraps the ordinary child Session journal. On child `InvocationStarted`, it reserves one exact root/child/turn/ordinal/account/model attempt **before** delegating to the ordinary journal/provider path. Exact duplicate reservation is idempotent; conflicting identity or exhausted deadline/counter fails closed before provider entry. Ordinary `InvocationFinished` commits first; only then is provider-reported token evidence settled into the root envelope. Missing usage remains unknown/incomplete and no price/currency is invented.

Child allocation is similarly reserved before Phase-42 managed-worktree creation. Allocation count remains consumed across preparation failure. Active-child count is acquired only when the child becomes running and released exactly once on terminal settlement/recovery. Owner restart interrupts linked allocations, repairs active counts from durable running rows, marks still-started model attempts unknown, and never replays model/Git effects.


## Phase 44 — bounded recursive subagents

Phase 44 activates recursive delegation on top of the Phase-43 root envelope without creating descendant-local budgets or routing authority. `--subagent-max-depth` defaults to 1 and is bounded to 1..4. The root Agent owns the one immutable root envelope; every descendant allocation stores direct-parent allocation lineage and authoritative recursive depth, while all descendants consume the same created/active/model-attempt/deadline counters.

The lineage-aware native `subagent` Service clone carries only root ID, direct-parent allocation ID and current depth. At each child tool call, aw revalidates the exact selected actor/account/model identity and direct-parent running relation/allocation. A child receives the `subagent` Tool only while its depth is below the root max; max-depth children do not advertise the tool. No child routing/model/account choice is introduced.

For non-off file tools, recursive descendants derive a **sibling** managed Git worktree from the direct parent's verified bound worktree and exact clean committed HEAD. Dirty direct-parent worktrees fail before descendant provider dispatch. Descendants retain the same read-only/workspace tool mode and never gain bash/Git-management authority. Model-only recursive children use the reviewed synthetic run-key lineage while the original root Agent remains the only physical-root owner.


## Phase 45 — governed child target routing

Phase 45 reuses Phase-40 immutable routing strategy revisions for child workers rather than adding a second routing engine. The native root host may select one exact actor-owned strategy revision with role `worker`; that revision is pinned immutably to the Phase-43 delegation root. With no child route configured, Phase-44 exact parent account/model inheritance remains unchanged.

Fixed, route-once and route-each child selection use the same bounded routing evidence and strict tool-free `RoutingExecutor` as main-target routing. The classifier sees only delegated task text plus candidate IDs/descriptions. Strategy-referenced provider profiles are registered into the existing shared `ProviderAccounts` registry and every chosen target is freshly selected/revalidated through the stable actor's current account/model authority and destination fingerprint. The model cannot choose a route strategy, candidate, account, model, endpoint or credential.

A child allocation is reserved first, then target selection is durably consumed into that allocation **before** worktree planning, Session creation or child provider entry. Classified child routing wraps the normal Phase-40 routing journal with the Phase-43 root budget: the delegation routing attempt reserves a root model-attempt slot before Phase-40 classifier intent/provider entry, while the ordinary routing result commits before delegation usage settlement. Recursive descendants inherit the same root route revision and may select different candidates only within that predeclared set.


## Phase 46 — bounded multi-model teams and explicit join

Phase 46 adds a host-selected immutable team template but **no second agent engine**. The root Agent receives a `team` tool only when `--subagent-team` resolves an actor-owned immutable template revision. The tool schema contains only a task and optional title; the model cannot choose members, join policy, provider/model, route, tool authority, depth, worktree, budgets or publisher. Recursive child Agents do not receive `team`.

A team template contains 2–4 ordered static members `(id, role, instruction)` and `all_required` join only. Candidates run sequentially in template order through the existing `subagents::Service`, therefore reusing Phase-42 worktree isolation, Phase-43 budgets/deadline accounting, Phase-44 recursion and Phase-45 child routing. Each candidate is an ordinary durable child Session. Candidate full answers remain there; team receipts store only Session/subagent IDs, answer digest and truncation evidence.

Join is deterministic: bounded imported answer prefixes are ordered by immutable template member order, labeled with role/member ID, hashed, and verified against each candidate Session's authoritative completed answer before acceptance. The resulting joined evidence returns as the normal `team` tool result to the root parent Agent. The parent Agent's next model round is the **sole publication/synthesis point**. Phase 46 does not add reviewer stages, a separate synthesizer model, voting/quorum, dynamic member generation, team-of-teams or automatic worktree merge.


## Phase 47 — native interaction correctness

Presentation and local input do not change provider, Session, routing, team, subagent, filesystem or billing authority. The unified palette derives click filters and typed `> ! @ # $ / &` prefixes from one parser; Explorer/main tabs derive hit regions from actual rendered rectangles and fold vertically when narrow; scroll indicators use the same render/list offsets as content; and the empty Agent composer remains one editor row with adaptive bounded placeholder text.

Read-only main panes expose a local cell-based selection layer. Explicit left-drag selection is highlighted and copied using bounded OSC 52 on release (or Ctrl+C while selected). Selection uses sanitized rendered text, Unicode cell widths and current horizontal/vertical offsets; it does not capture embedded terminal input, the Agent editor, command palette, Settings or Explorer action rows. Incoming model/file text still cannot emit terminal OSC controls through display rendering.


## Phase 48 — safe saved-Agent reactivation

Phase 48 fixes native Agent selection without weakening Session binding provenance. Ordinary `PersistentSession::open/open_session` still compares caller-current versus saved bindings and rejects directory/tool-root moves or changed destination disclosure. The fullscreen owner instead uses `open_saved/open_saved_session` to read the selected Session's own durable binding, then transactionally reconstructs the Host from that binding: saved working directory/tool root/system prompt/profile/model plus exact destination fingerprint.

The current live profile object may satisfy a saved Session when its profile name and destination fingerprint already match, preserving safe CLI endpoint overrides inside the same process. Otherwise the profile is resolved from configured/builtin profiles and must reproduce the saved destination fingerprint exactly. Unresolved reconciliation blocks switching to another Agent. A failed reconstruction/build restores the previous active Host/Agent rather than leaving navigation half-applied. The workspace projection refreshes root, tool mode, profile and model after a successful switch.

The same phase removes the extra DIM modifier from the empty fullscreen composer placeholder. The placeholder remains semantic-muted and one-line; this avoids terminal themes where muted+DIM made all text except the cursor-overlaid first character effectively invisible.


## Phase 49 — governed generative task planning

Phase 49 adds a root-only `plan` tool without introducing a second orchestration engine. The trusted host pins one actor-owned immutable routing strategy whose role is `planner` and whose mode is `fixed`; planner model selection is therefore host-declared and inference-free. The model-facing `plan` schema contains only a task and optional title. Planner/provider account, model, route, role allowlist, task count, child routing/tools/worktree/depth/budgets and publication policy remain host-owned.

The planner provider call uses a dedicated tool-free `PlannerExecutor` / `PlannerJournal` and `InvocationPurpose::Planner`. Planner evidence is bounded JSON containing only the root task, host-approved role labels and max-task cap. Strict planner output contains 1–6 typed tasks with ID, semantic role, bounded task text and dependency IDs. Unknown fields/roles/dependencies, self-dependencies, duplicates, cycles and oversized proposals fail before child admission.

The planner call creates/reuses the ordinary Phase-43 delegation root and consumes one shared root model-attempt slot before provider entry. A validated plan is durably materialized before child effects. Tasks then execute sequentially in deterministic topological order through the existing Phase-41–45 subagent service, preserving normal child routing, worktree, tool, depth and root-budget authority. Dependencies are ordering constraints only in Phase 49: a downstream child receives predecessor IDs but not predecessor answer text.

Each imported child result is verified against the authoritative completed child Session before a digest receipt is attached. The final planner join is deterministic and stores only task/session/subagent IDs, dependencies, answer digests/truncation flags and joined digest; full child/joined answer bodies remain outside the planner receipt tables. The root parent Agent receives the joined evidence as one tool result and is the sole final publisher/synthesizer. Restart never automatically replays planner or planned-child provider effects.

## Phase 50 — governed guardian review before parent publication

Phase 50 adds one optional host-selected guardian gate over already verified Phase-46 team joins and Phase-49 planner joins. It does **not** create another orchestration engine. The trusted native host pins one actor-owned immutable routing revision with `role=guardian` and `mode=fixed`, plus bounded host policy text. `aw-agent-core` invokes that target through a distinct tool-free `ReviewExecutor` / `ReviewJournal` and `InvocationPurpose::Guardian`; guardian output is strict typed `pass | deny | needs_human | require_evidence` data.

The team/planner join is committed and independently verified first. With a guardian configured, joined evidence is then quarantined from the parent Agent until the review gate releases it. `pass` satisfies only that review requirement. `deny` and `require_evidence` release no joined text. `needs_human` creates one durable owner approval request; a later parent turn may call the root-only `review_release` tool with only the review ID.

Guardian work shares the exact Phase-43 delegation root. It consumes one root model-attempt/deadline/usage slot before provider entry and consumes no fake child allocation. The root pins one immutable guardian route revision. Review records pin the original parent `InvocationIdentity` as well as the guardian identity, exact source join digest, policy/objective digests and strict verdict; therefore human approval cannot silently move evidence to another parent account/model destination.

Release is deterministic and non-generative. aw reconstructs the bounded team/planner join from authoritative completed child Sessions using the same Phase-46/49 import bounds, rechecks the immutable join digest and exact parent Session/branch/invocation identity, then creates/reuses an immutable release receipt. Owner restart maps admitted guardian work to interrupted and started provider intent to unknown without replay; already settled pass/deny/waiting-human/needs-evidence state survives. The parent Agent remains the sole final publisher.


## Phase 51 — native Agent presentation convergence (historical presentation substrate)

Agent presentation shares one Host/Agent/Session and semantic palette for assistant text, safe Thinking activity, errors, tool success/failure/neutral strips and menu selection. Fullscreen uses its alternate screen only when explicitly entered; `/tui inline` persists Inline before exit and the next ordinary turn stays inline. The first fullscreen Ratatui draw owns the fresh alternate-screen buffer instead of issuing a redundant explicit clear. See [Workspace](WORKSPACE.md).


## Phase 52 — safe native Markdown projection

Settled assistant text is projected through one pure native Markdown renderer shared by inline and fullscreen. Live partial deltas stay plain/sanitized; tool output, prompts, status and non-Agent content bypass Markdown. The renderer emits Ratatui spans only, treats HTML literally, exposes link destinations as text rather than terminal hyperlinks, performs no I/O/effects, and derives fullscreen search/copy rows from the exact painted lines. See [Workspace](WORKSPACE.md).


## Phase 53 — shared native syntax projection

Phase 53 factors the existing effect-free Syntect syntax/theme registry into the native TUI presentation layer behind the existing `workspace` feature. Workspace file/diff previews and settled assistant fenced-code Markdown now share that one compiled catalog. Fence language tokens are bounded local lookup keys only; unknown/malformed tokens fall back to neutral code, and no file/network/process/provider authority is introduced. Minimal/no-workspace builds retain neutral Markdown code fences without Syntect.


## Phase 54 — application mode vs Agent presentation ownership

Presentation preserves provider/tool/Session/governance boundaries and separates:

1. **application mode** — Agent (default) or Workspace;
2. **Agent TUI mode** — Scrollback, Inline, or Fullscreen.

One persistent `AgentPresenter` owns each Agent presentation lifetime. Since Phase 58, Scrollback owns one persistent aw presenter/session while its internal Ratatui Inline scratch viewport is dynamically reconstructed only when required row count changes; settled rows are inserted above it; Inline owns the full primary screen after preserving the pre-aw visible screen in terminal scrollback; Fullscreen uses the same Agent projection on the alternate screen. Workspace remains its existing independent alternate-screen multi-pane application.

This removes the previous sequence of independently constructed prompt/live/final inline viewports that could re-anchor from stale cursor coordinates after slash-popup changes or terminal resize. Owned modes redraw from authoritative Agent state instead of terminal archaeology. The full and minimal binaries use the same presenter abstraction.

Native scroll indicators now consume shared palette semantics: bright accent thumb/current viewport, dim/faint inactive track.


## Phase 55 — terminal lifecycle and shared Agent chrome

One generation-safe `tui::terminal_session::TerminalSession` now owns native physical terminal modes for Agent Scrollback/Inline/Fullscreen, Workspace and onboarding. Its atomic owner/flag state makes restoration idempotent and prevents an old presenter Drop from restoring a newer owner. Normal Drop/error, panic hook, and catchable Unix SIGHUP/SIGTERM/SIGQUIT converge on the same restore routine; SIGINT remains an application key.

The Agent UI also gains one shared `tui::agent_chrome` projection. Agent-only and Workspace Agent-tab presentation therefore share loaded-resource, prompt/Thinking/tool/assistant/Markdown/syntax semantics and composer placeholder logic rather than duplicating style/transcript code. Scrollback remains headerless except actual loaded-resource metadata; embedded Workspace omits redundant outer product/help lines. Context/Skills are host UI metadata and never inferred from model text. Workspace removes visible numeric tab/navigation prefixes and the redundant Explorer title while preserving positional shortcuts.

See [accepted history](../plans/accepted-history/TODO.md) and [Workspace](WORKSPACE.md).


## Phase 56 — non-blocking native UI work graph

Human Workspace work now follows one composable pattern: `UI intent -> bounded shared background executor -> typed result/progress -> generation-checked cache -> render`. Files/Git/layout/terminal-launch/progressive syntax consume the same executor; new human-UI components should accept that executor rather than spawn bespoke workers. The Agent owner/provider/tool/durable worker remains a distinct serialized authority.

Workspace draw/input performs no filesystem, Git, process launch, layout persistence or uncached Syntect work. File/Diff and settled Workspace-Agent rich syntax are progressive/cache-backed; plain content is available before highlighting. A reusable local viewport model owns fast scrolling, follow-end, scrolloff and bounded File/Diff tail overscroll without changing source-row identity. See [Workspace](WORKSPACE.md).


## Phase 57 — natural-flow Agent geometry and Scrollback release

Inline/Scrollback share a pure row-allocation model that separates **used flow height** from physical terminal height: short content consumes only real transcript/transient/composer rows and leaves trailing space below; saturated content consumes the full terminal and bottom-pins the composer by necessity. Fullscreen keeps the existing full-screen geometry. Default status moves to AboveComposer and exact historical defaults migrate without rewriting custom layouts.

Scrollback remains terminal-owned history plus one transient Ratatui inline scratch viewport. Presenter release is explicit and idempotent: `/quit` commits its prompt, then the scratch viewport is cleared and the cursor is repositioned to the actual completed-frame inline origin before terminal-mode restoration. This prevents a resumed shell from appearing inside stale aw chrome. See [Workspace](WORKSPACE.md).


## Phase 58 — dynamic Scrollback scratch sizing

The persistent aw `ScrollbackPresenter` now owns a dynamically sized Ratatui Inline scratch terminal whose height equals current human-visible transient rows. Height changes recreate only that internal Ratatui presentation object; provider/Session/tool/terminal-lifecycle authority is unaffected.

Natural terminal flow is derived from actual committed history rather than a fixed scratch reservation. Before saturation, dynamic scratch viewports remain adjacent to history. After the real viewport first reaches the physical bottom, a sticky presenter bit makes later scratch resizes reanchor to the bottom. Stable-height redraw/navigation does not recreate the scratch viewport. Phase-57 deterministic scratch release and Phase-55 terminal restoration remain authoritative.