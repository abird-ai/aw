# R6 — MCP prompts, resources, templates and change notifications

Original immutable takeover snapshot: FULL-COMPREHENSIVE-HANDOFF-2026-10-09.md. Its embedded R0–R13 roadmap and historical manifest are verbatim **handoff-time history**; this R6 file and current START_HERE record the authorized continuation rather than editing those annexed bytes.

Date: 2026-10-09. Track: one-way agentc → aw parity. This is **R6**, not historical Phase 69; schema remains SQLite v33, source remains 0.15.0. R5a and R5b stay intact. Rust std full and JSONL-minimal builds share this capability through the same rmcp connection, Agent registry, exact-source ToolPolicy, and journal-before-effect executor. The independent core-only no_std kernel is unaffected; the actual portable embedded Agent (E2/E3) remains open.

## Goal and user workflows

Use a specifically chosen MCP server to inspect offered prompts, list resource URIs and URI templates, explicitly retrieve one text-only prompt or resource, or observe finite changes. Discovery never authorizes the model to call an external tool or read an arbitrary resource. A separate explicit --mcp-resource-allow SERVER:URI gives the existing Agent a source-bound Unknown-effect text reader scoped to **exactly** those currently advertised URIs. There is no automatic prompt injection, passive startup subscription, polling, provider call, or browser/owner MCP trust expansion. An explicitly granted and connected Agent may hold a negotiated change stream for its active source lifetime; it is aborted when that source is dropped.

For existing selected transport (stdio, modern Streamable HTTP, deprecated same-origin 2024 HTTP+SSE):

    aw mcp --mcp-http docs=https://host.example/mcp --json prompts
    aw mcp --mcp-http docs=https://host.example/mcp --json resources
    aw mcp --mcp-http docs=https://host.example/mcp --json templates
    aw mcp --mcp-http docs=https://host.example/mcp prompt docs review path=src/lib.rs
    aw mcp --mcp-http docs=https://host.example/mcp read docs test://docs/report
    aw mcp --mcp-http docs=https://host.example/mcp read-template docs entry id=report
    aw mcp --mcp-http docs=https://host.example/mcp watch docs --resource test://docs/report --seconds 3

Clap global --json and --mcp-* flags belong **before** a data subcommand. Plain aw mcp with no data command remains the R5 tool-only discovery command. The URL above is deliberately an illustrative placeholder, never a tested public endpoint. For an Agent (host selected, current execution site):

    aw --profile llama --model MODEL \
      --mcp-http docs=http://127.0.0.1:8000/mcp \
      --mcp-resource-allow docs:test://docs/report \
      --print "Summarize that resource"

This registers mcp__docs__resource_read with one JSON Schema URI enum, not a broad resource root or a hostname grant. Remote data stays model-untrusted; the selected host has to decide whether its contents may be used in the task. The list endpoint alone does not create or authorize a resource tool. More than one URI can be granted explicitly with additional flags, up to 16 across servers.

## Architecture and trust decisions

- One library module crates/aw-mcp/src/resources.rs wraps the existing rmcp 3.5 peer. It is not another MCP client, provider or Agent. Options, source binding, stdio environment clearing, HTTP authentication, CA pinning, modern negotiation, legacy SSE pump, no redirects, no proxies, no auto retry, and no cross-origin bearer are inherited intact from R5.
- Commands are typed Clap commands shared by full and minimal. Read operations happen only in an explicit native CLI invocation, and do not create an Agent or model call. Discovery results and prompt message roles are marked data-only, text/JSON with safe terminal escaping.
- Current explicitly advertised prompt names, resource URIs, and template names must match before prompts/get or resources/read. Prompts/get validates advertised argument names, required fields, duplicates, max 16 arguments and max 2 KiB text value; no elicitation/MRTR input response is attempted. Prompt messages are text-only; roles are displayed rather than promoted to system authority.
- URI templates implement a **conservative RFC 6570 level-1 {variable} subset**. Unsupported operators, duplicate/missing/extra variables or dangling braces fail closed. Values are component-percent-encoded; server URIs are never fetched by an ambient HTTP client, only sent to the selected MCP connection. Advanced RFC 6570 expressions require a future focused extension and tests, not silent partial interpretation.
- Pagination: up to 8 pages and 128 descriptors per invocation; cursor at most 512 bytes, cannot be empty/cyclic/control; names/URIs validated, duplicates rejected, each JSON descriptor at most 16 KiB. Prompts and resource returned content reject oversized data (128 KiB total), binary/unsupported media, URI mismatches, control-bearing identifiers and traversal. Resource blobs/media are deliberately NOT decoded or injected.
- MCP per-server resource source kind is SourceKind::Mcp with a host-generated nonzero epoch, current actor and execution site. The one Agent/tool registry/ToolPolicy validates projection and effect. The generated reader is **Unknown effect**, never inferred Read from annotations; the Agent journals intent before an external action and handles unknown effects conservatively. It refuses an unadvertised URI, argument substitution, a stale revision or a changed source in the same turn. Up to 32 combined tools are preflighted before partial registration.
- A custom rmcp ClientHandler receives legacy tools/list_changed, prompts/list_changed, resources/list_changed and resources/updated; modern 2026 uses an explicit subscriptions/listen stream for the authorized Agent source lifetime (with a second HTTP request slot). The connection has separate atomic capability-class revisions. Notifications invalidate stale tool and resource readers before their next use (and again after a remote result arrives), and are not credentials, permissions or autonomous task triggers. The rmcp peer retains its native cache invalidation semantics where notifications are delivered through the ordinary handler; the modern subscription stream is independently revision-fenced. The HTTP client's long-running stream has no whole-response deadline; individual commands and connection establishment are still time-bounded. A changed resource requires explicit re-connection/discovery/regrant; no automatic replay or hidden refresh.
- A selected modern Agent starts a negotiated change feed only when the server advertises it and the host has granted a tool or exact resource URI. The feed shares neither model nor tool-dispatch authority. A second HTTP request slot allows ordinary calls while the stream is open; the SSE request has no overall response deadline while each concrete action retains a bounded Tokio timeout and the TCP connection keeps its connect timeout; it is aborted with the MCP source. Legacy peers can subscribe to exact approved URIs when negotiated. Abrupt modern subscription termination invalidates all source-class revisions and never reconnects/retries an unknown effect.
- The opt-in aw mcp watch command is **finite 1–30 seconds**. Modern 2026 uses subscriptions/listen with the peer-negotiated filter and bounded receive channel; initialized 2025 and deprecated 2024 use resources/subscribe / resources/unsubscribe for a selected URI, or observe listChanged notifications without URI. Invalid/unsupported subscriptions, overflow, stream faults, or changed-source conflicts fail conservatively. Notification payloads only surface source-labeled changed events; they never invoke model/tool calls.
- HTTP/stdio connected native single-user CLI remains the sole trust surface. Existing full web/owner restriction and credential isolation apply. No direct UI binding of remote resources, no automatic project context/prompt injection, no trust for remote "instructions", and no claim of a production MCP server certification.

## Evidence — keep source gates separate from runtime

Implementation:
- crates/aw-mcp/src/lib.rs; crates/aw-mcp/src/resources.rs (existing rmcp transport/provenance and new typed data/notification client).
- src/mcp_cli.rs, src/model_cli.rs, src/main_minimal.rs, src/main_full.rs (one grammar for both binary cuts).
- the removed agentc_r6_mcp_mock helper, the removed agentc_r6_mcp_fake_stdio helper, the removed agentc_r6_mcp_smoke helper (independent HTTP 2025/2026, legacy SSE 2024, stdio, fake OpenAI model).
- the removed test_agentc_parity_r6 helper is **source-only**; the removed validation_gate helper registers independent source and actual-binary steps.

Scenario ledger — execution contracts (records represent local mock evidence only, not live-server certification):

| agentc parity scenario | aw implementation | actual std-minimal fixture | full binary | embedded | platform/live |
| --- | --- | --- | --- | --- | --- |
| Explicit prompts/list and paginated discovery | rmcp peer list_prompts, bounded cursor/name/description | R6 smoke: 2025/2026/2024 | Full R6 smoke 32/32 | E2/E3 later | Offline loopback only |
| Prompt get with required args | selected advertised prompt; reject missing/extra/duplicate | R6 smoke | Full R6 smoke 32/32 | Host adapter later | No interactive MRTR |
| Resources/list and text read | exact current URI; no ambient fetch/media | R6 smoke, negative binary/oversize/forged | Full R6 smoke 32/32 | Host adapter later | HTTP/SSE/stdio offline |
| Resource templates/list and expansion | bounded level-1 template; explicit read | R6 smoke | Full R6 smoke 32/32 | Host adapter later | Complex operators unsupported |
| Model text read with exact host source grant | ToolSource::Mcp, Unknown effect, pinned epoch/URI, one Agent journal | Fake model/tool continuation and denial | Full R6 smoke 32/32 | One Agent E2/E3 later | Remote effects may be uncertain |
| Modern subscribe/listChanged | opt-in subscriptions/listen, verified filter and finite stream | R6 modern SSE stream | Full R6 smoke 32/32 | Host event adapter later | No passive startup polling |
| Legacy subscribe/resource updated | opt-in resources/subscribe+unsubscribe, bounded notifications | R6 stdio/2024 SSE | Full R6 smoke 32/32 | Host event adapter later | Transport lifetime explicit |
| Revision invalidation and cancellation | connection-scoped atomic revisions, stale denial | Rust unit contract + finite fixture | Shared crate tests | Portable predicates already E1 | Not a live server audit |

**Validated R6 closeout (local offline fixtures, both separately built binaries):** R6 **32/32 each** for std-minimal and default full; R5a **11/11 each** and R5b **11/11 each** unchanged; aw-mcp unit tests **11/11**, minimal CLI **88/88**, full CLI **260/260** plus full lib **108/108**, PTY **3/3**, R6 source checker **116/116**, R5a/b source checker **82/82** / **80/80**, independent E1 wasm32v1-none kernel/consumer with zero WASM imports. Workspace all-targets Cargo and warning-denied Clippy passed; reference source-bound gate passed after explicitly preserving the original handoff-time Git hash manifest. Modern 2026 additionally proves a long-lived subscription stream **does not starve authorized reads**, resource updates invalidate the reader, and tools/list_changed blocks a previously approved tool before any effect; data CLI refuses tool and URI grants before connecting. Run full/minimal binaries separately: they overwrite target/debug/aw. No live provider or real-world MCP service certification is claimed. One pre-existing host issue was handled only with the documented AW_GIT/AW_BASH Nix absolute-path overrides, not a product change. Whole-repository rustfmt remains red on inherited source formatting, and the monolithic whole-workspace runtime command lost the connector; neither gate is claimed green. Relevant std-minimal/full CLI runtime, aw-mcp unit, entire workspace compiler/Clippy and docs/reference gates passed separately.

## Explicitly open / deferred

- Advanced/full RFC6570 template operators and browser resource navigation UI.
- Full MCP media content handling, interactive elicitation, prompts-as-system templates, subscriptions across host restart, server-initiated tool/catalog auto-admission.
- Independent live MCP provider certification, Windows/macOS, E2/E3 portable complete Agent and E4–E6 no-libc final artifact.
- R2b optional; R7–R13 remain open; no skipped historical indexing/LSP or numbered Phase 69.
- No forced 0.15.0 bump, SQLite schema migration, installed OS tool, hidden bearer fetch, historical tag change or Git push.

## Check/commit discipline

Execute focused Rust tests and source contract, fresh compiled minimal/full actual binary fixture, inherited R5a/R5b modern/legacy smokes, warning-denied workspace and feature-cut checks, docs/static/source-bound reference gates. Commit only validated implementation as an unnumbered R6 checkpoint. Keep full original 2026-10-09 comprehensive handoff preserved verbatim; update current forward pointers in START_HERE and roadmap without erasing historical authority.
