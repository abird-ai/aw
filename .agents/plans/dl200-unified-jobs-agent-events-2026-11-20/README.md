# DL-200 — unified Jobs, Agents, Events, History and Checkpoints

Author: @pvl
Co-authors: OpenAI GPT-5.6 Sol
Owner: @pvl

Status: **SEALED / IMPLEMENTATION PLAN READY — NOT PROMOTED**
Opened: 2026-11-20
Base: `f035a00031b8f5879f4d77f3fd14bfe02b89305c`

DL-200 is the successor design layer over the sealed DL-150 tool/runtime contracts.

It does **not** rewrite DL-150. It defines the next public/runtime architecture for:

- the simplified six-tool Jobs surface;
- Agent as the public security/history/collaboration/execution envelope;
- one global Domain/Principal/Project/Agent/Turn/Job/Node namespace;
- user/group principal ownership and Agent ACLs;
- one `agent.create(from=...)` constructor for fresh and exact-history forked Agents;
- structurally shared immutable HistoryBase/HistoryGrant ancestry;
- host staged input planning;
- logical shared/derived/fresh Sandboxes independent from history;
- Agent-scoped Vault grants for secret/service projection;
- plural named typed SourceSets and source-aware checkpoints;
- non-destructive checkpoint restore into a new Agent + Sandbox;
- one generic NATS-style Event system for observation, publication and durable Agent
  reactions;
- one plain `Extension` unit registered through one typed `ExtensionRegistry`;
- typed subregistries under that root: Tool/Hook/Action/Resource/View/EventType/
  EventHandler/Provider/ProviderHook/ProcessHook/UI/Setting/State/Compactor registries plus
  native SourceDriver/SandboxDriver/VaultProvider registries;
- one small semantic vocabulary for the registered values: Hook, Handler, Action, Tool,
  View, State, Resource, UI, Setting, Provider, Compactor, Driver and true Policy;
- bounded View pull facets and durable EventHandler push handlers for embedding products
  such as Histate;
- one minimal dynamic Extension host: any explicitly selected executable speaking the
  versioned AW Extension stdio protocol, with TypeScript/JavaScript as the first SDK;
- WASM remains an optional later ExtensionHost behind the same registry contract rather
  than a second extension API/runtime requirement.

## Design

The public resource tree is:

```text
/d:<domain>
  /i:<principal>
    /p:<project>
      ├── /a:<agent>
      │     /t:<turn>
      │       /j:<job>
      │         /n:<node>
      └── /s:<sandbox>
```

The Agent branch is the `ExecutionRef` hierarchy. `SandboxRef` is a distinct
Project-level resource and never enters Jobs/control mutation APIs.

`i:` is one user-or-group principal namespace.

Agent is the public security/history/execution object. There is no public AW Session
coordinate or `session.*` API. Existing SessionStore/session_id may remain internal
persistence/recovery machinery during migration. A product such as Histate may still
have its own product-level Session that references AW Agents/Sandboxes.

Events project the same hierarchy to NATS-style dot subjects:

```text
d:<domain>.i:<principal>.p:<project>.a:<agent>
[.t:<turn>][.j:<job>][.n:<node>].e:<event>
```

with NATS-style wildcard cardinality/terminal semantics while preserving typed tokens.
Literal handle payloads are reversibly encoded in Event subjects so existing canonical
handles containing `.` remain one token (`foo.bar` -> `foo%2Ebar`):

```text
a:* / s:* / p:* / i:* / d:* / e:*   exactly one token of that declared kind
>                              one-or-more trailing tokens, terminal only
```

System Event names are bare. Custom Agent Events are structurally prefixed with `#`:

```text
e:changed
e:settled
e:#ready
e:#tests_passed
```

There is no custom-only partial-token wildcard such as `e:#*`.

## Key decisions

- Model-facing Jobs is exactly six tools:
  `jobs_list/jobs_get/jobs_read/jobs_send/jobs_ack/jobs_cancel`.
- `jobs_send` is directed Agent input or Process stdin/EOF only.
- Agent queueing/steering/interruption belongs to the host input owner.
- Host input staging is `list/add/edit/delete/send`.
- `rev` is mutable owner-state version, never immutable-history identity or authority.
- Referenced group membership changes do not fan out Agent `rev`; security revalidates
  independent group/principal generations.
- Fresh and exact-history fork construction use one host `agent.create(from=...)`
  operation. Fork remains a semantic relation, not another constructor.
- `checkpoint.restore` remains because it has a stronger contract: exact checkpoint
  history **and** exact SourceSet are recreated in a new Agent + Sandbox.
- Checkpoint = immutable history + immutable named typed SourceSet, possibly empty.
- Git is the mandatory first SourceDriver. Clean Git uses exact commit OID; dirty Git uses
  a hidden synthetic commit/tree built with a private index. Every live clean/dirty Git
  checkpoint anchor has its own private retention ref so branch reset/rebase cannot GC it.
- The SourceDriver contract also admits fail-closed immutable/versioned object/bucket
  manifests without turning AW into a generic archive service.
- Multi-source checkpoint capture is all-or-nothing at one fenced settled Agent boundary.
- Sandbox is durable logical mutable execution state, not one persistent OS namespace.
- An Agent selects one Sandbox at creation and does not live-rebind in v1. Agents may share
  one, branch one structurally, or use a fresh Project Sandbox by creating a new Agent.
- Named sources attach through only `sandbox.sources.list/set`; SourceRef is provider/
  driver identity, while credentials remain Vault authority.
- History ancestry and Sandbox ancestry are orthogonal.
- Same Sandbox never implies same authority; per-Agent Sandbox/Vault/tool authority is
  independently derived and revalidated.
- Vaults are provider-backed secret/service capabilities. A Vault is never mounted
  wholesale; selected entries project through provider/host-defined broker/socket/
  ephemeral-file/env profiles, never arbitrary caller-chosen mount/env targets.
- Checkpoints never contain Vault secrets, live leases or VaultGrant authority.
- Exact history forks structurally share immutable HistoryBase/HistoryGrant links over
  the existing Session journal and preserve attribution.
- Model exact-history forks default to the source Project + inherited source ACL; trusted
  host policy may narrow, never silently broaden.
- HistoryBase identity is immutable history/provenance only. Prefix/cache identity is a
  derived model-visible invocation projection and excludes Agent ACL/security identity
  unless that metadata is actually projected to the model.
- Agent access is one allow-only ACL over direct user/group principals.
- `i:<principal>` is one Domain handle namespace; user/group kind is metadata.
- Project is a principal-scoped public view over existing Workspace backing, not a
  second Project database.
- Events and prior Signals are one Event concept.
- Bare Event names are system-owned forever; custom publication requires `#name`, so no
  reserved-name registry or `aw_` prefix is needed.
- Event patterns use the canonical resource namespace and NATS-style wildcards.
- `events_publish` emits bounded custom `#...` Events from the current Agent.
- `events_subscribe` is the model durable reaction API.
- Host Event surface is `events.read`, `events.subscribe`, `events.publish` and bounded
  `events.subscriptions.list`.
- Existing immutable `commit_log` is the single Event history/order owner; subscriptions,
  cursors, indexes, causal counters and durable handler positions are current projections,
  not another Event log.
- Events carry stable `Event.id`, causal root/hop and an internal immutable
  DisclosureManifest ref where needed. Read, Agent delivery and durable Handlers all
  reauthorize the exact source plus that disclosure closure; private scan positions stay
  hidden.
- No topic/channel creation, EventLink service or second durable Event bus/database.
- Product data is not stuffed into arbitrary Agent metadata. Namespaced Views augment
  authorized reads only when requested, and a View failure never hides/fails the core
  authoritative snapshot.
- Trusted durable EventHandlers may build permanent product projections/integrations over
  the same commit log without depending on the finite public Event replay window.
- Extension registries publish definitions; installation never means activation/grant.
  Runtime `ctx.*` ports expose scoped current authority. Typed State uses per-key sequence/
  CAS where snapshot-relative; cross-Agent/product graphs stay in the product's own store.
- Commands/shortcuts/buttons converge on typed ActionRegistry entries; dynamic model Tool
  exposure still uses scoped `ctx.tools` rather than a Loadout type.
- Provider transport interception uses ProviderHook; Process launch interception uses
  ProcessHook. Extensions do not receive raw credentials or an unsandboxed `exec` escape
  hatch.
- Histate owns Builds, product Sessions, Decisions/Evidence/summaries/semantic graphs and
  commit classification; a product Session may reference many Agents/Sandboxes and a
  product checkpoint may reference one/more AW Checkpoints. Cross-store mutation uses a
  product-owned idempotent saga/outbox, not a hidden distributed transaction.
- Dynamic Extensions reuse the existing `aw-extensions` process direction rather than an
  embedded JS/WASM runtime: bounded full-duplex stdio + one registry manifest + scoped
  `ctx.*` calls, absolute selected executables, no same-process re-entry, and bounded
  nested calls. Node/Bun/Deno/Python/Rust/etc. can participate immediately.
- AW does not own npm/pip/cargo installation or an extension marketplace in v1; the user
  selects an already-installed executable/arguments.
- Do not embed Wasmtime in v1 solely for extensions; a WASI extension may already run via
  an external `wasmtime run ...` process speaking the same protocol.
- One authenticated mutation replay identity/fingerprint protects constructors/additive
  mutations without adding request IDs to every semantic API. Multi-resource creation uses
  private prepare -> atomic AW publication -> reconciled cleanup; half-created resources
  are never public.
- Shared Process settlement adds only `Tree::{Empty,Live,Unknown}` plus per-stream output
  completeness; `ProcessTerminal` remains the one factual exit/stop/tree/output result.
- Dotlink alone owns the ~30-minute checkpoint guidance + 35-minute hard conversational
  command horizon, result/log projection and non-login Bash policy; AW gains no turn API.
- The existing `017-sandbox-mount-convergence-2026-11-18` plan remains frozen. DL-200
  defines logical Sandbox/Vault composition only; lower mount/Bubblewrap implementation
  still requires separate explicit promotion.

## Read order

1. `../../docs/AW-DL200-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md`
2. `SPEC.md`
3. `IMPLEMENTATION.md`
4. `TODO.md`
5. sealed DL-150 references under `../dotlink-tool-port/reference/`
6. live source/tests before implementation.

## References

- `../dotlink-tool-port/reference/DL150-LIVE-CA89F959-README.md`
- `../../docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md`
- `../../docs/AW-DOTLINK-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md`
- `../../docs/AW-DL200-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md`

## Implementation gate

DL-200 is sealed after the fresh full pre-seal architecture audit and the simplified
0.17.6 Process-settlement adoption. The phased plan is `IMPLEMENTATION.md`.

Implementation is **not promoted**. Until a separate explicit implementation promotion:

- do not implement DL-200 source changes;
- do not mutate the published 0.17 runtime merely to approximate the sealed design;
- preserve current v36/current-main behavior;
- keep sandbox/mount/config-v13 implementation frozen.
