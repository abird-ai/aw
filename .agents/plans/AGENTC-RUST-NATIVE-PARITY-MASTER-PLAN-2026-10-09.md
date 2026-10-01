# aw — Rust-native agentc capability parity: research, architecture and implementation master plan

**Date:** 2026-10-09
**Status:** planning-only, proposed for execution in unnumbered parity work packages; NOT a claim of achieved parity
**Reference:** agentc 0.6.0, live master at 864351bf81847931f5eaa861d7851820e09c8f0a (read-only inspection on 2026-10-09)
**Target:** aw 0.15.0, base 2d9dc55367738d1ce7a5172acf8c5a957d0979e5, SQLite schema v33; immutable phase-68 tag unchanged
**Scope:** strictly one-way agentc -> aw; aw's extra capabilities are irrelevant to the parity score
**Optional embedded requirement (2026-10-09):** see AW-EMBEDDED-NOSTD-LIBC-FREE-ARCHITECTURE-2026-10-09.md. Std is the normal/default environment. A separate no_std+alloc libc-independent embedded library must share the same Agent kernel, not a second Agent executor; this is planning only and is not built yet.
**Prior evidence:** AGENTC-MINIMAL-RUST-PARITY-2026-10-08.md (capability ledger and M0 implementation)
**Architecture authority:** FULL-COMPREHENSIVE-HANDOFF-2026-10-08.md, START_HERE.md, focused .agents/docs and live source/tests/Git
**Decision boundary:** no Phase 69, no rewritten tags, no push, no reintroduction of deliberately skipped Phase 64/65 indexing/LSP

**Execution successor (2026-10-09):** [AGENTC-PARITY-IMPLEMENTATION-PHASES-2026-10-09.md](AGENTC-PARITY-IMPLEMENTATION-PHASES-2026-10-09.md) is the current multi-phase R0–R13 implementation roadmap; [AGENTC-PARITY-R1-KERNEL-2026-10-09.md](AGENTC-PARITY-R1-KERNEL-2026-10-09.md) records the first reviewable A1/E1 source slice. The master remains the design authority, not an assertion that missing agentc features are complete.

## 0. Executive decision

Treat agentc as an educational reference and an executable, scenario-level oracle, not a porting specification or codebase to transplant.

The desired result is a Rust-idiomatic, small-by-default agent with **all** of agentc's real user/developer capabilities. aw can deliberately improve details: safer trust and permissions, explicit paid-effect recovery, modern MCP compatibility, intuitive command grammar, better plugin isolation, native Rust reuse and less duplicated state.

Make the minimal binary the ordinary coding-agent product profile. Keep the full product as that same engine plus optional durable/distributed/multi-user/workspace capabilities. Neither profile has a second Agent loop, parser, command registry, tool executor, session persistence implementation, provider semantics or theme/UX reducer.

Do not conflate three different claims:

1. **Functional coverage:** one can accomplish every agentc task (plus C/Rust extension author workflows) with aw minimal.
2. **Behavioral certification:** matched real-binary tests verify commands, error paths, context continuity, tool semantics, UI and platform-specific behavior.
3. **Footprint:** measured disk/RSS/startup/first-paint data on the same host, with identical workloads. Rust is not required to reproduce agentc's C23 940 KiB binary or its allocator. Do not invent memory/size superiority.

The current committed M0 baseline provides minimal = [jsonl], a local Session journal with explicit resume/list/new/title, compatibility fencing, root-level uncertain-effect checks, actual-binary fake-provider proof, and no SQLite/remote feature dependency. **That is not full parity.**

## 1. Evidence, source precedence and freshness

- Live agentc: README.md, include/agentc*.h, src/{app,base,core,ext,net,plat,prov,tui,wire}, tests/, extensions/, .agents/design/{00,10,20,30,40,50} and .agents/plans/P5-AXES.md. agentc 0.6.0 reference HEAD now includes subsequent TUI fixes and opt-in raw TUI diagnostics; the older agentc handoff's frozen HEAD is historical, not current source.
- Live aw: Cargo.toml, src/main.rs, src/main_minimal.rs, src/minimal_sessions.rs, src/sessions.rs, crates/{aw-agent-core,aw-providers,aw-tools,aw-store-jsonl,aw-store-api,aw-ui-extension,aw-platform,aw-store}, src/tui, src/workspace, the removed validation_gate helper and actual CLI flags. Read current code before changing any component.
- Existing aw source-bound and runtime checks remain authoritative for implemented features. The new plan must never replace full/review gates with documentation assertions.
- External standards/providers below were checked on 2026-10-09. They can change; pin source URLs, Cargo.lock and accepted protocol versions during implementation, and re-check the published upstream contract before each upgrade.

### External primary references and what they establish

| Source | Implication |
| --- | --- |
| MCP specification 2026-07-28 release, https://blog.modelcontextprotocol.io/posts/2026-07-28/ | Current MCP removes mandatory initialize and protocol session IDs; requests are stateless, support capability discovery, cache hints and subscriptions. |
| MCP current base/changelog, https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/changelog.mdx | Changed wire negotiation from 2025-11-25; requires explicit backwards compatibility with legacy servers. |
| MCP Streamable HTTP, https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/transports/streamable-http.mdx | POST per request and response-scoped SSE; no current GET stream or session-header assumption. |
| MCP security considerations, https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/authorization/security-considerations.mdx | Consent, confused-deputy and token forwarding threats are distinct from tool schema validity. |
| Official Rust MCP SDK, https://github.com/modelcontextprotocol/rust-sdk and https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/README.md | rmcp (currently 3.x) supports client stdio child processes and Streamable HTTP, feature-gated. Benchmark/verify an actually locked compatible version rather than building custom MCP framing. |
| Anthropic Messages API, https://platform.claude.com/docs/en/api/messages/create and https://platform.claude.com/docs/en/build-with-claude/streaming | Typed text/tool/thinking blocks; original signed continuation metadata cannot be transformed into plain text. |
| Google Gemini content/tool docs, https://ai.google.dev/api/generate-content and https://ai.google.dev/gemini-api/docs/generate-content/thought-signatures | Streaming function calls and opaque thought signatures must preserve exact part, order and model binding. |
| Gemini newer Interactions API, https://ai.google.dev/api | Google offers a newer interaction primitive; implement reference generateContent first, leave an adapter seam for Interactions without duplicating the core. |
| Anthropic third-party authentication, https://support.claude.com/en/articles/13189465-log-in-to-your-claude-account | Preferred third-party access is an official API credential, not assuming consumer subscription OAuth can be reused. |
| Anthropic subscriber API credits, https://platform.claude.com/docs/en/about-claude/api-credits-for-subscribers | An eligible subscriber can link a Console org for supported API credits; billing/custody differs from consumer app login. |
| OAuth Security BCP, https://www.rfc-editor.org/rfc/rfc9700.html | PKCE, issuer binding, token handling and redirect safety must drive auth flows. |
| Cargo features, https://doc.rust-lang.org/cargo/reference/features.html | Feature sets are additive/unified. Full/minimal profiles should compose positive capabilities, not compile-time opposite implementations. |
| Rust ignore/WalkBuilder, https://docs.rs/ignore/latest/ignore/struct.WalkBuilder.html | Existing gitignore-aware on-demand walk can be reused as a search candidate filter; not a security/symlink boundary. |
| Tokio JoinSet, https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html | Structured bounded task groups are available, but aborting a future does not prove external process/side-effect cancellation. |

**Special care:** some older agentc designs target MCP 2024-11-05 with initialize and a persistent stdio pump. Those are historical interoperability tests, not the architecture to adopt for 2026-07-28. Optional new MCP features (Tasks, multi-round-trip, elicitation, MCP Apps) need negotiated support and explicit authority; they do not become general aw Agent powers just because the SDK exposes them.

## 2. User-visible product contract

An ordinary user should understand five concepts first: Agent, model/account, current working directory, tools/permissions, and Session. Additional concepts appear when invoked.

### Primary commands (proposed grammar, not shipped claims)

    aw                                    # interactive local Agent, no hidden server
    aw --print "explain this error"       # one prompt
    aw -C PROJECT --tools read-only       # explicitly permit safe built-in reads/search
    aw model list                         # unified catalog and discovered metadata
    aw model refresh --provider anthropic
    aw provider list                      # profiles, available auth methods and health
    aw provider login PROVIDER            # only supported authorization mechanism
    aw provider logout PROVIDER
    aw session list|open|new|fork|export
    aw mcp list|show|add|remove|connect|status|tools|prompts|resources
    aw extension list|show|install|remove|enable|disable
    aw tool list|explain|allow|deny
    aw trust show|allow|revoke
    aw rpc --stdio                        # line-delimited JSON control protocol
    aw --output jsonl --print "..."      # line-delimited events

These names are design candidates to validate against the existing one-typed-Clap tree before implementation. Do not break current aliases, remove the current aw -p/--profile meaning, hijack existing --mode agent|workspace, or silently reuse an argument with another meaning merely to mimic agentc's flags. Flags select execution; nouns manage durable resources. Slash commands remain one shared typed registry, not a second interpreter.

### UX rules

- Bare aw performs no passive repo crawl, provider HTTP request, MCP spawn, plugin execution, scheduled task, listener start or privilege expansion. Creation of local session storage is deferred until a user action requires it.
- A previously configured trusted project may declare opt-in services/resources; show their connected and authorized status accurately. Presence of an MCP or extension declaration does not grant a tool.
- "Model" displays provider, endpoint/account alias and context/capability data; unknown fields are labelled unknown. Do not silently move the Session to another provider when the user changes the model.
- Tools status distinguishes available / connected / permitted / active; no confusing "registered therefore executable" UI.
- Failed or unknown effects surface a concise actionable message and exact inspection/retry options.
- In-session /model, /think, /resume, /compact, /theme and /skill should have a useful no-argument picker or status; invalid values explain available choices rather than silently resetting.
- Common interactions use keyboard-first, accessible semantics in all Agent views (Inline, Scrollback, Fullscreen and Workspace Agent tab) and browser. No duplicated key reducer or duplicated render interpretation.
- Advanced machine/network/governance controls stay behind existing namespaces; do not burden minimal help/composer.

## 3. Proposed canonical module ownership

    aw-cli                  one typed Clap surface, composition and user policy
       |
       +-- aw-agent-core    one Agent/turn/tool loop and typed events/hooks
       +-- aw-providers     provider adapters, discovery, models, credentials-by-reference
       +-- aw-tools         bounded built-in tools, search, shell, effect classification
       +-- aw-capabilities  (new only if justified) registry snapshots + grant evaluation
       +-- aw-mcp           (new) official rmcp client adapter and remote resource projections
       +-- aw-extensions    (new only if justified) external host/lifecycle adapter
       +-- aw-store-api     stable Session/Journaling interfaces
       |   +-- aw-store-jsonl  minimal single-owner, no replay
       |   +-- aw-store        full SQLite, CAS/writer epochs/leases
       +-- aw-editor        shared grapheme-safe composer
       +-- aw-ui-extension  existing presentation-only typed projections
       +-- aw-tui           existing src/tui, same state/interaction engine
       +-- optional web/workspace/agentnet/multiplayer/routing/etc.

Do not create an aw-capabilities or aw-extensions crate before there is a second real consumer. Begin with narrowly owned modules in the existing crates, extract only when Rust module boundaries and dependency cuts demonstrate lower coupling.

### Ownership rules

- **AgentCore** is the sole authority for model/tool round ordering, cancellation, attempt identity, event emission and response materialization. It does not open sockets, files, credential vaults or a SQLite connection.
- **Provider adapters** convert provider wire events to one typed response/content format. They never choose accounts, grant tools, write Session history or retry by contacting a different provider.
- **Tool registry** owns descriptors and immutable active snapshots; **ToolPolicy** owns permission, independent of source. MCP/extension contributions are metadata until explicitly authorized and bound.
- **Executor** owns tool start/finish, reservations and per-call deadlines. The journal remains the before-effect authority. Source-order tool-result assembly is separate from observed completion order.
- **MCP** owns protocol transport, discovery, cache and reconnect. It does not authorize activity or write private Session facts directly.
- **Extension runtime** owns lifecycle and bounded hooks; it may propose typed changes but cannot write arbitrary durable authority or create credentials.
- **Resources** owns text prompt/skill/theme discovery, project-trust association, provenance and explicit admission. It does not scan merely because a renderer opens.
- **SecretStore abstraction** owns actual credentials (local minimal vault or full server store). Session metadata contains opaque handles, never bearer/refresh values.
- **Context** owns explicit selection, summary artifacts and CAS attachment; automatic compaction is a policy-driven request through this same service, not a shadow conversation store.
- **Surfaces** subscribe to read-only typed events and render state. They do not make provider/tool/authority decisions.

## 4. Key architectural decisions (accept/reject rationale)

| ID | Adopt | Reject / reason |
| --- | --- | --- |
| D01 | One Rust Agent engine for minimal and full | Copying agentc's C submit loop into a minimal engine creates drift |
| D02 | One typed tool/permission model for builtin, MCP, plugins | Separate MCP tool loop or implied permission from discovery |
| D03 | Separate availability, selection, authorization and execution | "Connected means enabled" is a confused-deputy risk |
| D04 | Immutable registry snapshot pinned per tool batch | Changing provider/tool schema halfway through a batch |
| D05 | Pure, bounded tool schema validation before side effects | Trying to repair malformed JSON by guessing intent |
| D06 | Default concurrent READ waves; serialize writes/process/unknown | Fully parallel writes or treating server readOnlyHint as trusted |
| D07 | Existing Session journal before each external effect | Promise of exactly-once execution on the public internet |
| D08 | Explicit new LLM attempt after ambiguous provider dispatch | Re-issuing an uncertain paid call as invisible transparent retry |
| D09 | Context auto-compaction through authorized existing summary/apply | Trimming history in-place or bypassing derived restrictions |
| D10 | Official rmcp client adapter with protocol-version matrix | Homegrown MCP parser/transport copied from legacy C |
| D11 | Versioned process extensions by default; compiled Rust traits locally | Stable Rust trait-object dylib ABI or unrestricted in-process plugins |
| D12 | Per-provider typed content parts and opaque continuation | Flattening thinking signatures/tool metadata to user text |
| D13 | Model catalog + discovery cache is descriptive metadata | Allowing discovered models to override an account's allowlist |
| D14 | Common SecretStore contract; minimal local custody adapter | SQLite just to save a login or plaintext tokens in config/session |
| D15 | One locked semantic config writer + per-field conflicts | Reintroducing whole-file stale config overwrites |
| D16 | Explicit user/project resource trust and provenance | Auto-executing repo instructions/plugins after entering directory |
| D17 | One typed event vocabulary, JSONL/RPC serializations at edges | Separate agent engine for RPC/headless or raw global event bus |
| D18 | Versioned feature cuts and conformance matrix as acceptance | A single "all tests green" claim without platform/behavior evidence |
| D19 | Pin dependencies with Cargo.lock, use upstream libraries when apt | Vendoring an in-house HTTP/TLS/OAuth/MCP/regex implementation |
| D20 | Current platform and release safety gates remain mandatory | A tiny binary whose install/runtime safety bypasses the full product |
| D21 | Keep std as the default/preferred host, add optional no_std embedding | Replacing working std facilities with custom embedded runtimes |
| D22 | Positive std and alloc features, core-only and no_std+alloc build proofs | A negative no_std feature or conflating static musl with libc-free |
| D23 | One portable Agent transition kernel consumed by hosted and embedded adapters | Two separate Agent execution loops and diverging semantics |
| D24 | Optional host-supplied time, entropy, transport, journal, tools, secrets and memory | Fake or insecure fallback capability implementations |
| D25 | Real final-linked ELF/WASM import/symbol audits | Claiming libc-free from library source or an rlib compile |

### Protocol and data types to converge on

- ProviderDescriptor: ID, wire dialect, base URL policy, auth modes, declared discovery, model capability metadata.
- ModelDescriptor: provider-scoped ID, context/output limits if known, reasoning/tools/vision flags if known, source, fetched_at and TTL; price schedule separate from capability metadata.
- CredentialRef: provider, issuer (when applicable), local vault handle, version/epoch, owner/custodian; **no secret**.
- CapabilityDescriptor: stable scoped name, source, source revision, input/output schema, type/effect, timeout/budget, ownership.
- ToolGrant: actor, approved capability ID, grant scope/root/executor, policy revision and expiration; never inferred from description.
- ModelPart: Text, visible ReasoningSummary, ToolCall, ToolResult, provider-specific opaque signed continuation; never inject hidden thinking into assistant answer.
- AgentEvent: Started, MessageStarted, Delta, Reset(attempt_id), ToolProposed, ToolStarted, ToolFinished, ContextChanged, ModelAttemptSettled, TurnSettled, plus state/diagnostic events; typed/bounded.
- ExternalEffectReceipt: operation_id, attempt_id, call_id, actor, executor/target, capability revision, context digest, intent before dispatch, known completion/unknown, output provenance.

Do not prematurely implement all fields in one giant wire object. Land the minimum typed fields with versioned, extensible representations, and add schema evolution only when an actual backend needs it.

## 5. Component plan: tools, search, permissions and concurrent execution

**R2a implementation decision (2026-10-09):** use ripgrep/fd family std crates regex/globset/ignore in aw-tools, but retain descriptor-rooted Root::list/Root::read instead of their ambient filesystem walkers or binaries. Tool schemas ls/find/grep and source-grant narrowing are in [R2a plan](AGENTC-PARITY-R2-SEARCH-2026-10-09.md). This is an accepted maintenance/security simplification, not a claim that the agentc process backend switch is shipped. Future no_std search consumes host bytes and can reuse alloc-only regex-automata.

### Search as on-demand tools, not filesystem indexing

Offer read-only model tools ls, find, grep and richer read alongside existing exact edit/write and trusted shell. Give exact schema, root policy, normalized bounded output and discovery metadata. No daemon, watcher, LSP or persistent index.

- Use aw-platform's scoped/no-follow root for final filesystem access. An ignore/gitignore-aware walker may enumerate candidate paths, but never by itself authorize opening them. Recheck inside the root before opening; prevent traversal/symlink/hardlink bypasses and refuse unsupported special files.
- Root conflict checks must consider **overlapping/nested tool roots**, not only byte-identical canonical root strings. Until a secure overlap/inode proof exists, serialize or conservatively fence conflicting mutations within one owner and require isolated worktrees across independent processes. A filesystem walker or an advisory file lock is not cross-user isolation.
- Use Rust libraries such as ignore, globset and a bounded regex engine rather than copying agentc's bespoke parser. Cap visited entries, depth, bytes, matches, line length, elapsed time and output. Cancel on deadline; explicit truncated markers.
- Tool backends: internal Rust fallback always available; optional external rg/fd with exact argv, absolute resolved executables, -- delimiter, sanitized environment, no shell, explicit tool/backend status and deterministic normalized result policy. No automatic arbitrary PATH execution if not permitted by the current tool policy.
- Shell: preserve current explicit trusted grant. Add Windows cmd/PowerShell/pwsh options through aw-platform with exact user selection, visible runtime selection and process-group/job-object cancellation; never select an unknown ambient shell silently.
- Model-facing named allowlists select only from already-approved ToolPolicy capabilities. No CLI flag can widen beyond host/actor grants.
- Keep a bounded stable registry: builtin, MCP and extension registration share naming, collision policy, source revision and snapshot lifecycle. When tools change while a turn is running, apply next turn/batch, not mid-call.

### Parallelism: simple by default, safe under cancellation

1. Finish parsing and validating *all* proposed tool calls and authority gates before dispatching the first.
2. Snapshot identity/permissions, tool schemas and involved resource roots, then append all external intents that are about to be dispatched.
3. Dispatch independent read-only calls concurrently under one bounded JoinSet with a semaphore; keep concurrency limits low/configurable. No speculative write parallelism.
4. Serialize file mutations, shell, network/external or unknown-effect tools. If a batch mixes reads with mutations, execute ordered non-overlapping waves; preserve ordering where reads could otherwise observe concurrent writes.
5. Complete each result into its original call-index slot. Frontends may show completion order; the subsequent model request includes one exact result per declared call ID in **source order**. Cancelled/rejected calls receive explicit terminal statuses without invented effect success.
6. On cancellation, bound child/remote cleanup; if effect completion is uncertain, mark unknown and require reconciliation before another conflicting write or automatic retry. Never infer that a cancelled Tokio task cancelled a process or external API call.

Acceptance includes multiple long/short read tools finishing out of order, read/write overlap decisions, mid-batch revocation, remote MCP timeout, orphan subprocess, process crash after Nth intent, result-order stability and no duplicated writes.

**R3a implementation successor (2026-10-09):** [AGENTC-PARITY-R3A-ANTHROPIC-2026-10-09.md](AGENTC-PARITY-R3A-ANTHROPIC-2026-10-09.md) records the new Anthropic native Messages adapter, opaque signed thinking, API key/SecretStore custody, native effort and actual-binary mock tests. R3b Gemini, provider presets/caching/pricing remain pending. Core no_std Agent extraction is unaffected; std remains the default provider transport.

**R3b implementation successor (2026-10-09):** [AGENTC-PARITY-R3B-GEMINI-2026-10-09.md](AGENTC-PARITY-R3B-GEMINI-2026-10-09.md) adds native Gemini GenerateContent streaming/function-call IDs and opaque same-model thought-signature continuity, Google API-key headers, first-run preset, discovery and SecretStore kind. The common Agent/tool/journal remain unchanged. R4 adds the model capability catalog, pricing, richer model-specific thinking controls, and explicit Interactions research.

**R4 implementation successor (2026-10-09):** [AGENTC-PARITY-R4-MODEL-CATALOG-2026-10-09.md](AGENTC-PARITY-R4-MODEL-CATALOG-2026-10-09.md) adds a single metadata catalog for all native provider adapters and descriptive presets, bounded page discovery, private per-account 24h cache, user-priced integer rates and a typed aw model CLI in std minimal/full. A cache entry never authorizes a model or replaces selected credentials. No hidden discovery on startup; no provider invoices fabricated. Gemini 2.5/manual model setting enrichment remains a follow-up.

**R5a MCP tools successor (2026-10-09):** [AGENTC-PARITY-R5A-MCP-TOOLS-2026-10-09.md](AGENTC-PARITY-R5A-MCP-TOOLS-2026-10-09.md) implements optional modern/legacy MCP stdio/Streamable HTTP tools via rmcp with exact grants. R5b two-endpoint 2024 SSE and R6 MCP resources/prompts remain pending.

**R5b legacy SSE transport successor (2026-10-09):** [AGENTC-PARITY-R5B-LEGACY-SSE-2026-10-09.md](AGENTC-PARITY-R5B-LEGACY-SSE-2026-10-09.md) extends the R5a official rmcp client with an explicit 2024-11-05 two-endpoint GET SSE/POST transport-only bridge. RMCP lifecycle, JSON-RPC/tool schema and aw exact-source Agent authorization remain canonical; no HTTP silent fallback, SSRF bearer redirect or auto replay. R6 resources/prompts/subscriptions are next.

## 6. Component plan: providers, models and credentials

### Adapter architecture

One Provider trait and shared HTTP client setup; small per-wire adapters:
- Existing OpenAI Chat Completions, Responses and Ollama remain.
- Native Anthropic Messages: streaming text/tool and thinking blocks, stop/usage mapping, cache tokens, partial JSON accumulation, exact signed thinking continuation.
- Native Google Gemini generateContent: streaming, multiple function calls, functionResponse IDs, thoughtSignature continuity, usage and finish reasons. Future Interactions API is a separate optional adapter, not a second Agent loop.
- OpenAI-compatible presets (OpenRouter, xAI, DeepSeek, Groq, Mistral, Together, etc.) are **data descriptors** and endpoint/auth policy, not cloned adapters.
- Third-party custom providers use an explicit provider descriptor and a bounded plugin/extension adapter, not arbitrary hostname-derived authority.

Build a provider conformance table for: text, multi-step tools, thinking, cancellation, malformed/partial SSE, backpressure, stop/usage, cache accounting, HTTP 4xx/429/5xx, TLS and auth errors. No live endpoint required for ordinary development.

### Typed content and private continuation

Keep public transcript content and provider-private continuation distinct. Exact opaque signatures may be required to continue a *current* Anthropic/Gemini tool round. Bind each opaque value to provider, model, account, executor, source response/attempt and stage. It must not be displayed, exported as normal JSONL, injected as ordinary prompt text or reused after switching custody. For crashes, either persist it in a separate explicitly protected local secret/private continuation store with clear retention or mark the current provider step unrecoverable and require a new authorized attempt; never forge a signature.

### Model registry and pricing

- One bundled baseline descriptor catalog, refreshed explicitly or by bounded TTL after user-approved provider connection.
- Discovery reports source and freshness, respects advertised context/output/reasoning/tool/vision capabilities, handles pagination/limits/timeouts and never widens account allowlists.
- A single model picker consumes the exact same merged snapshot in TUI, browser and headless commands. Cache keyed by provider + endpoint fingerprint + credential custody class; OAuth refresh handled at the auth seam.
- Price entries are dated, provider-scoped, currency/unit-specific and provenance-labelled. Prefer signed/first-party documented rates or explicit user overrides; never turn an estimated cost into verified provider billing. Unknown cost remains unknown. Separately account input/output/cache-read/cache-write/reasoning when present.
- A cache miss or failed discovery never prevents manual entry of a valid model ID.

### Login and custody

Introduce a backend-neutral local CredentialVault/SecretStore **implementation seam**, not a second auth service. Full aw keeps its existing SecretStore custody. Minimal stores handles/config and uses platform keyring when available, or an explicitly unlocked encrypted local vault; environment-only credentials are a valid headless fallback. Do not claim encrypted-at-rest storage when the decryption key sits beside the ciphertext with identical permissions.

Supported Claude API keys should be the official third-party default; do not assume that Claude Pro/Max browser OAuth may be reused by custom clients. Official subscription-linked API credits are separate from consumer session credentials. ChatGPT plan auth remains supported only through an explicitly permitted flow, with no tokens in ordinary Session JSONL. Use OAuth Security BCP issuer/state/nonce/PKCE/refresh-rotation rules, consent, revocation and exact audience/issuer binding. Failed refresh never silently selects another key or payer.


## 7. Component plan: MCP 2026 and legacy interoperability

### Use the official Rust SDK, not an MCP rewrite

Create an optional aw MCP *client adapter* using the official modelcontextprotocol/rust-sdk (rmcp), with client-side child-process stdio and Streamable HTTP features enabled, macros/server extras disabled unless needed. Pin an actually compiled 3.x version and Cargo.lock, inspect its backward compatibility, feature closure, MSRV and licensing. Preserve the already-approved aw HTTP/TLS/SecretStore policy where possible rather than giving rmcp a separate ambient proxy/credentials path.

- Prefer current MCP 2026-07-28 behavior: stateless JSON-RPC requests, per-request version/capability metadata, response-scoped POST/SSE, standard HTTP routing headers and separately negotiated discovery.
- Provide a **versioned legacy session adapter** for 2025-11-25 and older servers that expect initialize, initialized and Mcp-Session-Id/stream conventions. Do not conflate a transport instance with an authorized aw Session.
- Only negotiate protocol versions actually supported by the installed SDK. Unsupported or downgrade-incompatible servers fail with explicit diagnostic and no credential/tool grant broadened.
- Server features: tools, prompts, resources, resource templates, pagination, changing-list notifications and negotiated subscriptions. Resource reads remain text-only initially; binary resource policy must be explicit rather than silently truncating/misparsing.
- New optional MCP 2026 features (Tasks, MRTR, elicitation, Apps) are separate negotiated feature modules. Do not expose remote sampling, local filesystem roots, OS paths, host UI or autonomous model invocation because a server requested it. Implement only when a real agentc-equivalent workflow or a later explicit requirement needs them.
- Spawn configured stdio processes only under an explicitly accepted user/project server declaration, not on aw --help or from rendering status.
- Remote HTTP MCP supports HTTPS by default, origin/redirect/SSRF filtering, bounded DNS/connect/read/response, exact OAuth resource/issuer binding, no credential forwarding to a different origin, and redacted diagnostic logs.
- Local stdio servers receive an allowlisted environment/cwd; do not implicitly inherit every AWS/GitHub/cloud credential from the aw process.
- Response schemas, tool names and resources are **untrusted data**; bound size/depth/count, remote refs, schemas, JSON nesting, pagination, errors and notifications. Validate JSON Schema 2020-12 with a mature library; prohibit arbitrary network dereference of $ref and cap recursion/CPU.
- Honor cache TTL/scope and invalidation notifications, but cache never grants ongoing authority. A changed tool definition yields a new registry revision; an already-admitted tool call remains bound to its original revision or fails conservatively.
- An MCP "readOnlyHint" is advisory, never a security proof; unreviewed tools classify as Unknown and need permission. Prompt-injection content from a server remains model input data, not executable policy.
- No hidden listening MCP *server* is required for parity. An explicit future aw MCP server mode can reuse the same registry but must be separate from the client connector.

### Clear state machine and UX

    Disconnected -> Connecting -> Ready[version, capabilities, revisions]
           |           |                 |
           v           v                 v
         Disabled     Failed <---- notifications/disconnect
                          |
                      BoundedBackoff -> Connecting

A server is available only when the user explicitly enabled it and the trust context still permits it. Represent connection health independently of ToolGrant. In the status view show last error, next retry (if configured), tools/prompts/resources counts and selected protocol version. A temporary MCP disconnection may block that tool; it must not trigger re-routing or replay of its previous unknown effect.

Use existing typed events to publish server lifecycle. No periodic poll service when MCP is disabled or unconfigured. Ensure one slow server cannot starve others; bound concurrent connection attempts with a small semaphore. Resource subscriptions update cache and registered descriptors atomically.

### Interoperability tests

Build a small mock MCP server with both versions, stdio and HTTP transports, mixed tools/prompts/resources, reconnect, OAuth fixture and malicious schema/error cases. Add conformance fixtures against the official spec and at least one independent real third-party MCP server in an opt-in network/isolated container gate. Required scenarios include tool schema changes during a call, retired name, notification storms, oversized resource, HTTP redirect, unauthorized request, cancellation, timed-out process and unexpected server exit. No live credentials in CI.

## 8. Component plan: extensions and typed lifecycle hooks

### One capability catalog, separate hook domains

Keep the existing compile-linked Rust Agent Extension trait (execution gates) and UI ExtensionHost contract (presentation only). Add a **bounded typed extension registry** at the composition boundary for tools, provider adapters, prompt templates, command handlers, status providers and declared resources. All registry contributions carry owner identity, version, manifest, revision and explicit grants.

Runtime extensions default to a separately spawned process through a versioned, length-bounded protocol. JSON-RPC is adequate for management/registration/hooks; reuse MCP for tools/resources/prompts wherever it covers the intended capability. Avoid duplicate implementation of MCP in an aw-specific protocol. One external extension host must not run another Agent loop or acquire direct SQLite/SecretStore/terminal handles.

If C/Rust authors need functionality analogous to agentc's versioned in-process ABI, supply small C and Rust **process protocol SDKs**, with a hello C and a Rust provider/tool extension fixture. This meets the ability to author/run runtime extensions without forcing an unstable Rust dynamic-library ABI. Optionally later support audited, trusted native C ABI loading as a separate dangerous/advanced feature. Document that a C extension built for agentc is **not binary-load compatible** with aw.

### Hooks: explicit phase and decision semantics

Implement only hooks with a concrete consumer and test:
- observe: agent/turn/message start, updates, end, provider attempt settlement, tool propose/start/finish, session/model/effort/compaction/MCP state transitions.
- validate/veto: before prompt, before provider dispatch, before tool effect, before context commit, before session switch, before compaction; multiple approvals never turn a denial into permission.
- transform: non-authority data such as user input prompt, candidate response or display metadata under exact schema and bound; cannot rewrite actor, target, account, payer, custody, tool grant or immutable attempt evidence.
- provider adapter registration: named/configurable wire dialect with typed streaming result and AccountRef, but no direct bearer tokens in plugin-facing payload.

Hook ordering is deterministic: priority + registration order + exact revision. Use a snapshot per turn/batch. A failed mutating/authorization hook fails closed, observation errors remain diagnostics with bounded delivery. Require cancellation, time limit, recursion/re-entrancy guard and crash isolation; prohibit arbitrary hook callbacks in the middle of a store transaction. A post-tool denial cannot undo effects and must preserve the receipt as unknown/denied rather than pretending the tool never ran.

Status/UI extensions keep the existing safe Line/Span presentation protocol. No raw terminal escape, DOM, SQL handle or implicit tool grant enters those projections.

### Installation and trust

Metadata-only extension install ≠ load ≠ enable ≠ grant. Store extension manifests/code outside trusted project data paths, verify origin/integrity and version compatibility, and expose the requested capabilities before first activation. Never execute downloaded bytes in a package installer just to identify version. A disabled extension's registration and in-flight calls settle cleanly; revocation is rechecked before any new dispatch.

## 9. Component plan: context compaction and safe retry

### Automatic compaction as a policy on existing context flow

The current full product already has context preparation, tool-free model summarization, immutable artifact metadata, provenance and explicit CAS application. The minimal JSONL backend instead has one Session journal and HistoryState checkpoint semantics, not the full materialized multi-user ContextFlow store. **Do not copy the SQLite context engine into minimal.**

The current in-memory Conversation enforces a bounded active turn count. Therefore an automatic compaction feature must also define how a long-lived Session's **active projection** advances beyond that limit while its immutable total history stays retained. Advancing the context window is not equivalent to deleting durable attempts; add an explicit active-tail projection/checkpoint cursor with compatible JSONL/SQLite semantics and prove 500+ successive turns over repeated compactions without circumventing context/effect limits.

Implement a bounded, backend-neutral CompactionController which:
1. reads exactly the currently authorized public provider projection, version/head and model context window metadata;
2. estimates usage conservatively with known provider tokens where possible, budget margins and a capped fallback estimate when unknown;
3. computes a cut that keeps a recent coherent span and never splits a tool call/result batch or private context continuation;
4. generates a tool-free summary with exact current account/executor/cost policy, journaled as a separate paid attempt;
5. validates and stores immutable summary evidence; applies only by expected source revision and policy CAS (for minimal JSONL, one append-only checkpoint/retained-tail event through its store API);
6. preserves original immutable history plus dependency closure and appropriate notice to the user;
7. refuses if source/custody/permissions changed or a matching compaction is already in flight.

Controls: compact.mode = off | ask | auto, reserve/context thresholds, keep_recent, max_summary_tokens, max automatic operations per turn, and per-session cost budgets. Default to **ask** when model context/cost is unknown; allow auto for a known context limit and an explicitly selected billing policy. Manual /compact uses this same backend-neutral service, not a shortcut that unconditionally drops old turns.

An estimated trigger is not permission to spend; the responsible account and executor remain selected at the moment of preparation. On failure leave the old projection intact, surface actionable limit and never create repeated hidden summarizer calls.

### Retry policy as durable attempt policy, not network loop magic

Differentiate:
- transport setup fails provably before provider dispatch (safe same logical operation, new trace event);
- provider explicitly rejects without producing a result (can retry only under the account/user's permitted retry policy, with a *new* attempt record);
- dispatch may have occurred / partial stream / final response unknown (unknown paid cost; no unacknowledged interactive automatic replay);
- tool effect may have occurred (never automatically replay; inspect and reconcile first).

A Retry-After value controls the proposed delay but is not proof that another request is free. Backoff/jitter capped, deadlines and spend envelopes bounded. Same-context retry never silently chooses a different model, account or route. For transient pre-effect retries, expose attempt number and frontend DeltaReset(attempt identity); never erase the immutable old attempt or its cost evidence.

Use cancellation-aware async waits; cancellation does not prove vendor cancellation. Do not add an independent retry scheduler or automatic background "healer" in minimal. Full governed autonomous schedules can use existing durable policy/occurrence receipts only.

## 10. Component plan: resources, trust, TUI and configuration

### Instructions, skills, prompts and theme discovery

Reuse the current typed loaded-resource provenance and explicit skill registry. Add optional user and **trusted** project resource roots for:
- AGENTS.md, AGENTS.override.md, CLAUDE.md or agentc-specific instructions along the ancestor path;
- SKILL.md frontmatter metadata and on-demand /skill:name body admission;
- prompt templates with bounded, documented positional/named variables, clear defaults and slash invocation;
- named theme files as typed palettes constrained to semantic color slots (no scripts/raw terminal controls);
- selected MCP prompt/resource content by name and server identity.

Parsing/discovery is passive **data only** and never itself grants tool/network/shell authority. No untrusted project file can directly change account, endpoint, permitted model, SecretStore custody, execution root, command list or plugin activation. Trust stores canonical project identity + source provenance and a revocable decision with explicit scope; symlink/mount and cross-platform path handling must be reviewed. Do not load project resources from the renderer; load only at admission/policy events.

Config precedence: explicit CLI > per-user override > user defaults > trusted project data overrides limited to non-authority settings > built-in defaults. Actual account/credential grants are separately authorized, not overwritten by any config layer. Reuse the one current locked three-way merge authority, with per-field independent merges and same-field explicit conflicts. Unknown keys are preserved only where schema and safety policy permit; do not silently accept "future" authority keys as harmless.

### TUI UX parity with fewer special cases

All presentational behavior projects the same Agent data, not per-mode copy/paste:
- one picker primitive for model/provider, effort, Session, MCP services, commands and theme;
- one autocomplete registry for built-in slash commands, MCP prompts, file templates and explicit skills, with deterministic collision/priority rules;
- one grapheme-safe aw-editor, persistent private history, bracketed paste (never submit on paste), bounded collapsed multi-line paste chips, @file path completion constrained to tool root, undo/kill/yank/transpose;
- one chat/Markdown renderer with visible thinking policy, settled vs provisional text, tool cards (pending/success/error), exact progress, pagination and copy/search;
- Inline/Scrollback/Fullscreen use existing terminal lifecycle/CPR fallback with no ghost rows, terminal injection, or split-brain between owned scratch and shell scrollback. Prefer natural flow until saturation, then bottom anchor;
- responsive named themes work for native and browser without a second theme CSS authority.

Terminal PTY tests include resize storms, tmux, no CPR, UTF-8 graphemes, combining marks, emoji width, paste >10 lines, Ctrl+C/ESC/tmux key protocols, no TTY, binary tool output, extension-provided ANSI injection, tool cancellation and mode switching in-flight. UI compatibility, not pixel identity with C renderer, is the goal.

## 11. Component plan: machine-readable events, RPC and embedding

Do not overload the existing root --mode agent|workspace with a second semantic interpretation. Introduce a separate headless --output=jsonl option and an explicit aw rpc --stdio command, with optional well-defined aliases only if they do not break prior invocations.

One typed AgentEvent vocabulary; two serializers/adapters:
- stdout JSONL: ordered versioned event envelopes, model/provisional text and tool deltas, completion, reset, error, compaction and immutable attempt IDs. No raw secret/provider-only signed blocks.
- stdin/stdout RPC: bounded request IDs, response vs accepted/settled events, no mixing human TUI text into stdout. Commands include prompt, prompt_template, abort, get_state, set_model/effort, available_models, compact, trusted bash with explicit grant, new/open Session, get_messages, and supported extension control.
- Rust SDK: existing aw-agent-core/aw-client remain the programmatic local interfaces, no new in-process Agent implementation.
- C/Rust external consumers: use a versioned RPC client library with explicit reconnect and status lookup; never resend an uncertain prompt simply because the pipe broke.

RPC uses the same request admission/receipt path as the native/local owner. A cancelled/drop/closed stream cannot turn an unobserved paid/effectful call into "never executed." Bound input bytes, queue depths, slow consumers, event fanout and response buffering. Strictly partition stdout protocol from stderr diagnostics. Emit exact capability advertisement, including when a feature is compiled out, not success-shaped placeholders.

Opt-in debug/wire recording is sanitized by default: redact Authorization, cookies, bearer keys, OAuth headers and private continuation. Raw wire capture requires explicit user permission, private destination and a warning; no debugging flag grants authority or modifies provider retries.

## 12. Build, platform and footprint strategy

### Stable feature cuts (contract, not a new binary)

    --no-default-features                        stateless/in-memory Agent
    --no-default-features --features minimal      all agentc-equivalent local features (target)
    --no-default-features --features jsonl        current local journal subset
    --features full                               current full default (must also get shared parity)
    other existing explicit feature combinations  remain valid

Internally minimal becomes a feature aggregator over independently tested capabilities (provider-dialects, model-discovery, mcp-client, extensions, tools-search, local-credentials, context-compact, jsonl, rpc, resources). Keep these modules optional; no idle service starts from feature presence. Keep the full default stable and add the same new shared components to full. Dependency-tree checks must prove minimal omits rusqlite, aw-store, AwNet/quinn, Axum/browser and heavyweight distributed orchestration.

### Cross-platform matrix

agentc's reference targets include Linux x86_64, aarch64, riscv64; macOS arm64/x86_64; Windows x86_64 (with Windows arm64 research support). aw's current release graph covers Linux x86_64/aarch64, Windows x86_64/aarch64 and macOS arm64. Add missing Linux riscv64 and macOS x86_64 if build/runtime costs and toolchain availability permit, without removing existing targets. Do not claim cross-platform parity solely from ELF/Mach-O/PE headers.

Test the actual minimal binary on:
- Linux native x86_64: full loops, PTY, MCP stdio/HTTP, mock provider/TLS, extensions, OAuth fixture, benchmarks.
- Linux aarch64 and riscv64: cross-build plus QEMU/equivalent runtime smoke with clear cross-emulation limitations.
- macOS arm64/x86_64: real host CI for terminal/process/HTTPS, not cross-link-only.
- Windows x86_64 (and existing aw arm64 target): native CI or Wine plus explicitly separate real-host console/PowerShell testing.

Retain existing release manifest, independent asset audit, checksum installer with no pre-install candidate execution, scrubbed environment probes and no unsigned publisher-signature claims. Signing/notarization is a separate, explicit key-custody decision.

### Performance reporting

Measure current agentc and each aw cut on the same Linux host/terminal:
- stripped binary and compressed download;
- cold --version and first TUI paint (median/p95 and distribution, not single lucky run);
- clean idle RSS and session RSS at 0, 100 KB, 1 MB and near-cap transcripts;
- first-token and total response latency on a deterministic local mock;
- busy/no-work UI CPU and filesystem/network process count;
- cold incremental build/dependency size (developer cost) separately from runtime footprint.

No arbitrary "must be 940 KiB" gate. Set evidence-based budgets after measuring the initial parity-capable Rust release. Investigate outliers, configure opt-level/LTO/codegen/strip only after no regressions; never sacrifice correctness/supported-platform behavior merely for size.

## 13. End-user defaults and graceful capability disclosure

- Minimal default: local persisted Session, current selected provider/account and previously authorized resources. No daemons/listeners or project auto-executions.
- Tools default off unless user/profile explicitly grants; read-only grant enables read/list/search, writable grants remain separately confirmed.
- MCP servers are disconnected until an accepted server configuration is relevant to active use. Optional explicit background reconnect only within an active Agent with the server enabled.
- auto-compaction default ask unless known model context and user-approved billing policy; manual /compact always available when a summarizer and storage capability exist.
- model discovery is opt-in at setup or explicit refresh; stale cache is labelled stale and manual ID remains usable.
- Dynamic/installable extension disabled until installed and intentionally enabled; tool grants separately required.
- HTTP retries conservative and bounded; no silent reroutes or ambiguous-effect replay.
- Cost is reported as exact provider-returned token usage plus a separately labelled estimate, never as verified invoice.
- Status/editors show actual capabilities rather than menus leading to misleading no-op placeholders.
- A missing optional feature returns one clear action: rebuild with the named feature or connect/authorize the required external service.

## 14. Work packages and dependency graph (all unnumbered)

Each is a reviewable working slice with its own focused plan, tests and commit; do not auto-create Phase 69 or a historical tag. A larger package should be further sliced if it stops being reviewable. "Goal" never means blanket approval to push.

| Work package | Dependencies | Implementation outputs | Exit gate |
| --- | --- | --- | --- |
| A0 — M0 foundation, already done | existing aw core/JSONL | minimal feature, private JSONL Sessions, no-state, destination/root effect fencing | previously recorded 86/86 minimal, 6/6 actual-binary, 3/3 PTY; rerun after changes |
| A1 — capability inventory + registry (co-design with E1) | A0; embedded E0 design | core-compatible typed descriptors, immutable registry revision, independent grants, shared std/embedded authorization predicates | unit + fake server + std tool schema tests AND wasm32v1-none core-only no_std compile |
| A2 — on-demand coding tools | A1 | read/list/find/grep, rg/fd fallback, exact tool allowlists, Windows shell policy | real-binary file/escape/symlink/output-limit/proc tests on Linux and Windows target cuts |
| A3 — provider wire protocols | A0 | Anthropic + Gemini adapters, typed signed parts, conformance fixtures | multi-round tool/thinking/partial-error tests with no leakage or forged continuation |
| A4 — model/catalog/discovery | A3 | presets, provider metadata, freshness-aware cache, pricing/override provenance, picker source | offline setup and discovery tests for every supported wire adapter; account allowlist never expands |
| A5 — MCP core client | A1 | rmcp stdio+HTTP adapter, current/legacy version support, MCP tools list/call, lifecycle | independent mock servers in old/new versions; no unauthorized execution |
| A6 — MCP prompts/resources | A5 | bounded prompts/args/resources/templates, notification/subscription refresh and caching | changed-list and malicious input tests; exact resource isolation |
| A7 — typed extension host | A1,A5 | process host, bounded hook bus and registry, C/Rust SDK examples, custom provider/command/status | crashing/disabled/revoked process tests; no trait ABI or authority leakage |
| A8 — safe batch parallelism | A1,A2,A5 | read wave concurrency, effect serialization and exact result index | fault injection: crash/cancel/revoke/timeout/results-in-order, no replay |
| A9 — compaction | A3,A4 | shared CompactionController for JSONL/SQLite, /compact and auto/ask modes | retained-tail/provider-custody/CAS/recovery/cost tests |
| A10 — classified retry | A3,A9 | attempt evidence, Retry-After policy, reset events, safe pre-effect retry | provider response/drop/restart and paid-unknown crash tests |
| A11 — trusted resources + TUI | A4,A6,A7,A9 | project trust, SKILL.md, prompt templates, named themes, picker/editor UX | actual PTY and browser workflows, no passive I/O/exec, consistent slash surface |
| A12 — minimal local credentials | A3,A4,A5 | backend-neutral local CredentialVault, supported provider login/logout | OAuth PKCE/issuer/refresh/revocation fixtures, no tokens in JSONL/config |
| A13 — headless protocols/SDK | A7,A9,A10 | JSONL events, RPC stdio, C/Rust client examples, wire diagnostics | independent subprocess clients, cancellation/idempotency and parity traces |
| A14 — package/platform/benchmark | A2–A13, embedded E6 for the optional embedder | all capabilities in std minimal/full, six-target proof where feasible, perf report; separately linked no_std embedded audit | full conformance/profile cut/platform/installer audits; embedded proof is separately labelled |
| A15 — final one-way parity sign-off | A14 | scenario-by-scenario matrix with source, executable and CI evidence | **every agentc-implemented row certified**; partial/unavailable stays open |

Embedded coordination: [AW-EMBEDDED-NOSTD-LIBC-FREE-ARCHITECTURE-2026-10-09.md](AW-EMBEDDED-NOSTD-LIBC-FREE-ARCHITECTURE-2026-10-09.md) defines E0–E6. E0 shapes A1, E1 proves no_std core types in A1, E2/E3 move the same Agent transition kernel beneath hosted and embedded adapters before A8/A9/A10 become tightly hosted-specific. The std-based minimal and full product keep all current std/Tokio/Reqwest behavior. No libc-free aw Agent build is claimed yet.

Critical path: A0 -> A1 -> A2/A5 -> A6/A7 -> A8 -> A9/A10 -> A11/A13 -> A14 -> A15. Providers A3/A4 and credentials A12 can proceed in parallel as independent modules once typed content and SecretStore seams are agreed.

Do not rush heavy MCP/extension runtime support into A1. First write type/protocol tests with fake implementations; then add the concrete rmcp and process host adapters.

## 15. Acceptance ledger by area (agentc implemented features only)

For every row in the earlier 2026-10-08 one-way checklist, maintain the following fields in a **machine-readable, reviewed test manifest**, not an optimistic prose percentage:

    feature_id, reference_source_at_commit, reference_scenario,
    aw_component, aw_feature_cut, aw_test_id,
    baseline_behavior, accepted_better_deviation_with_reason,
    linux_evidence, macos_evidence, windows_evidence,
    status=missing|partial|implemented|certified,
    approval_if_security_semantics_differ

Initial groups (each expands to individual cases in implementation):
- AG-01..AG-12: Agent submit/stream/think/tool loop/stop/cancel/retry/compaction/usage/trace/headless.
- TL-01..TL-14: read/edit/write/bash/ls/find/grep, path safety, external backends, selective grants, concurrent/ordered results, shell OS variants.
- PV-01..PV-12: OpenAI/Responses/Ollama/Anthropic/Gemini/gateways, discovery, catalog/cache/prices, OAuth, tools, thinking signatures.
- MC-01..MC-10: MCP stdio/HTTP, versions, tools, prompts, resources, template arguments, notifications, reconnect and authorization.
- EX-01..EX-12: C/Rust plugin authoring, compiled/runtime installation, tools/custom providers/commands/prompt sections/status, lifecycle/hooks, trust.
- UX-01..UX-14: TUI three modes, model/thinking/session pickers, input history/word keys/paste/@file, named themes, SKILL.md, project trust, Markdown/cards.
- IF-01..IF-08: JSONL event/RPC commands, embeddings, recorded-wire redaction, native release targets, packaging, benchmark.

An implemented source function without a scenario test is at most **implemented**, never certified. Do not count a feature from full aw if it is absent from **minimal**. Do not compare only command names: include semantic paths for malformed input, authorization failure, retries, cancellation, restart and mixed provider/tool batches.

## 16. Test architecture and day-to-day developer speed

### Cheap default

- cargo fmt on changed files, compile smallest relevant feature cut;
- unit tests for the affected crate and typed component; source-only policy checker;
- mock provider/server fixtures local and ephemeral, no production APIs;
- no Nix cross-build, full workspace, live OAuth, network, QEMU or GUI test on every editor save.

### Opt-in full parity validation

    cargo test --locked -p aw-cli --no-default-features --features minimal
    cargo test --locked --workspace --all-targets
    cargo clippy --locked --workspace --all-targets -- -D warnings
    cargo test --locked --workspace --doc
    RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --no-deps
    python3 the removed validation_gate helper run --profile reference
    python3 the removed agentc_minimal_parity_smoke helper --binary target/debug/aw

Extend the existing explicit Rust/full validation profile with separate MCP, provider, extension, tool, context, RPC, actual-binary/PTTY/browser and static tests, and build the **minimal binary freshly** before minimal binary smokes (do not accidentally run target/debug/aw from a full-profile build). Use feature-disambiguated target dirs or binary paths when parallel builds happen.

Run test dependencies in disposable local containers/temporary roots with test-only credentials. Clean up even when tests fail. Any network or provider test is opt-in and names what it contacted; no production environment assumed. External services and GitHub cross-target runners are only claimed when their actual gates return success. Use subprocess process groups and hard timeouts; no source-bound Python structural checker substitutes for Rust/real-binary evidence.

### Specific adverse-case matrix

- No hidden network/FS/MCP/plugin/process side effects on aw --help, --version, no-tools bare prompt and Settings open/save.
- Long-lived JSONL sessions survive repeated compaction beyond the original active 256-turn bound, without losing immutable history or allowing untrusted context to escape its source policy.
- Same-root **and nested-root** unknown-effect protection, symlink-alias/path normalization, and separate process ownership recovery remain conservative.
- Two processes edit distinct/same config fields concurrently, preserving independent changes and surfacing exact conflict.
- Session switch/profile/model change during live tool, pending provider attempt or unresolved external effect.
- Unauthorized/disconnected/stale MCP capability, forged readOnlyHint, tool name collision and list revision race.
- Project trust revocation while a discovered SKILL.md/template/extension is already known.
- Read-only tool traversal with symlinks, hardlinks, invalid Unicode, deleted files, ignored files, special files, Windows drive/UNC paths.
- Concurrent read calls complete in reverse order; mixed write batch never produces an invented successful result; output remains paired and ordered.
- Crash after tool/provider intent, before dispatch, during effect, after result persistence, before branch CAS/JSONL append and before client delivery.
- Anthropic/Gemini mixed thinking/text/tool signatures across partial streams/restart/model/account changes.
- Auto-compaction expensive generation receives cancellation, loses paid result or sees stale CAS target.
- CLI JSONL/RPC truncated frame, invalid IDs, slow reader, duplicate request, reconnect and no replay.
- PTY resize/paste/theme/status/locale/keyboard/terminal restore and two-pane Workspace Agent parity.
- Binary installer checksum/name/version mismatch, downloaded candidate never executed before install; scrubbed auditor environment.
- Cross target builds checked through actual QEMU/Wine/native runtime as appropriate, source version tagged/provenance recorded.

## 17. Migration and compatibility discipline

1. Land opt-in new capabilities under typed crates/features with existing default behavior intact.
2. Put shared structs at the lowest crate that needs them; avoid circular aw-cli/aw-agent-core/aw-mcp/provider dependencies.
3. Add backwards-compatible serde version gates for wire/session/logs and explicit migrations only when stored authority changes.
4. Minimal JSONL remains one-owner/one-main-branch; do not silently pretend it provides SQLite's distributed ACL/writer lease semantics.
5. Preserve exact provider AccountRef and credential custody during model changes; history must not regrant tools.
6. Reuse current config.lock/mutate/save_changes, do not write stale whole-file TOML snapshots.
7. No automatic rewriting of user projects, trust grants, skill bundles, old Session exports, Git tags or credentials.
8. Provide one-time opt-in import from agentc JSONL through the existing session-format/interop seam when implemented, treating all imported paths/authority as *untrusted metadata*. Copy text/tool history; never replay old effects or restore provider keys from files.
9. When a reference behavior is less secure than aw's current boundary, explicitly document a safe workflow-based deviation and its test. Do not quietly drop the feature.

## 18. Risk register and anti-complexity decisions

| Risk | Guardrail | Evidence to demand |
| --- | --- | --- |
| A second minimal Agent implementation emerges | one aw-agent-core entry point and one TUI composer reducer | static feature cuts + same mock loop tests |
| MCP spec/SDK changes | pin rmcp; adapter owns version transitions; conformance tests | legacy and current independent server fixtures |
| Extension privilege escalation | grant registry independent of contributed metadata, safe process host | malicious process + revoke/kill tests |
| Unbounded plugin/MCP complexity | add only real hooks/negotiated methods; process caps and timeouts | coverage per registered extension method |
| Provider opaque continuation leak | exact provider/account/model/step binding and separate private store | cross-account/model switch tests |
| Hidden provider cost via retry/compaction | durable new attempts, budgets and explicit policy | fake-billed delay/drop/crash tests |
| Concurrent mutations corrupt state | serialize unknown/write effects, prepare/commit under journal and CAS | stress and kill-window proofs |
| Search tool bypasses scoped root | secure final open; on-demand bounded traversal | symlink/race/path fuzz fixture |
| Huge compile/runtime footprint | feature cut and dependency tree budget, don't ship optional services | same-host minimal/full/agentc measurements |
| UX surfaces drift | one typed command/status/event model | Agent three modes, Workspace and browser parity |
| Config/project trust confusion | explicit precedence and separate authority grants | trust revoke/stale config collisions |
| Version/OS claims exceed evidence | unsigned local receipts, source-bound matrix, target runtime smoke | native/CI reports with SHA and exact binary |
| Maintenance overload from too many crates | extraction rule: >1 real consumer + clear dependency edge | no redundant dispatcher/store/provider wrapper |

## 19. Suggested first implementation prompt (after plan approval)

    You are implementing the user-approved one-way agentc 0.6.0 -> aw Rust-native
    parity program. Read .agents/plans/AGENTC-RUST-NATIVE-PARITY-MASTER-PLAN-
    2026-10-09.md, .agents/plans/AW-EMBEDDED-NOSTD-LIBC-FREE-ARCHITECTURE-
    2026-10-09.md and .agents/plans/AGENTC-MINIMAL-RUST-PARITY-2026-10-08.md,
    then START_HERE, the master handoff, and focused current docs.
    Inspect live Git, actual source and both minimal/full Cargo feature graphs.
    Do NOT start Phase 69, revive Phase 64/65 indexing/LSP, move tags or push.
    Implement ONLY A1/E1 first: one typed capability descriptor/registry and
    explicit grant/selection contract shared by existing std tools and a core-only
    no_std test host. Keep std as the default; do not create a second Agent loop,
    an embedded HTTP stack, or an unproven libc-free release claim. Add concrete adversarial
    tests, then run minimal/full compile, Clippy, relevant actual-binary and
    full source-bound reference gates. Commit only a validated reviewable slice.

## 20. Acceptance boundary / final decision

This is a **design proposal**, not an implementation completion claim. It supersedes the 2026-10-08 parity checklist's sequencing and refines the design decisions, but preserves that checklist as the baseline one-way feature ledger.

Before any package is called complete:
- demonstrate real behavior through the *minimal* binary and the same shared source through full;
- prove boundaries do not widen model/account/tool/provider/machine authority;
- run focused and inherited reference/cargo/Clippy/docs/PTY/browser/static gates appropriate to the change;
- mark unsupported providers, protocols or release targets as missing/partial rather than passing by inference;
- keep the code readable, small and composable; remove obsolete branches whenever a replacement is verified.

**Final goal:** a standalone Rust aw minimal that can perform every tested agentc user/developer workflow through first-class, safe and intuitive Rust-native components, without adopting agentc's freestanding-C constraints or weakening aw's proven durable-effect architecture.
