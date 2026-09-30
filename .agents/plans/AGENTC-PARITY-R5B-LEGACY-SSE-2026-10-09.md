# R5b — Deprecated MCP 2024-11-05 HTTP+SSE compatibility

Date: 2026-10-09. Track: unnumbered aw agentc → aw one-way parity phase R5b (NOT historical Phase 69). Baseline: committed R5a 8269dc9, source 0.15.0, SQLite v33. Historical phase-68 tag and skipped 64/65 indexing/LSP are unchanged.

## Scope and authority

The official MCP 2024-11-05 transport requires two endpoints: a long-lived SSE GET, whose first endpoint event announces a per-session JSON-RPC HTTP POST URL, and subsequent message SSE events delivering typed JSON-RPC responses. Streamable HTTP introduced in 2025-03-26 superseded it. This is deliberately **opt-in** with --mcp-sse SERVER=URL; there is NO automatic fallback to or from --mcp-http. This is a legacy interoperability feature, not a second agent/tool/protocol engine.

**Key design**: crates/aw-mcp/src/legacy_sse.rs implements only old transport framing and same-origin POST routing. Its two bounded tokio::io::duplex pipes feed rmcp 3.5's AsyncRwTransport, so **the same rmcp typed JSON-RPC ClientLifecycleMode::Initialize with exact ProtocolVersion::V_2024_11_05**, tool listing and calls, bounded schema validation and aw-agent-core Tool/Journal execute every effect. Std/full and std-minimal use the same flags/adapter. No no_std/embedded dependencies are changed.

### Transport/security contract

- --mcp-sse SERVER=URL must identify the exact opted-in server; --mcp-auth-env SERVER=ENV and optional HTTPS --mcp-ca-file SERVER=/ABS/PATH reuse the already validated R5a name, TLS, trust, origin and account rules. Only explicit loopback HTTP is permitted; other endpoints require verified HTTPS. No URL userinfo, fragment or user-supplied query.
- The bridge GETs SSE with Accept: text/event-stream, checks success/MIME and requires the **first non-comment SSE event to be endpoint** within an absolute 20-second negotiation deadline (heartbeats cannot reset it). SSE data is incrementally parsed, UTF-8 and CRLF supported, with bounded 128KiB frame and per-event data, no unbounded line growth.
- The untrusted endpoint announcement may include an opaque legacy session query (e.g. ?sessionId=opaque) but must resolve to **the identical scheme, hostname and effective port** as the explicitly selected GET URL. Off-origin redirects, protocol downgrade, userinfo, fragments, oversized URI/query and control characters are rejected before any POST or model invocation. Server response messages are required to be JSON-RPC 2.0 with request ID or method.
- A single selected bearer is resolved once into zeroizing transient memory and sent to GET and to validated **same-origin** POST only; no credentials or opaque session query enter prompts/Session JSONL or client logs. All connections disable redirects/proxies; HTTPS trust behavior is shared with modern R5a via one verified_http_client adapter, except the legacy SSE GET has no overall response timeout because it is intentionally long-lived.
- A scoped pair of async reader/writer tasks terminates with the Agent MCP connection (AbortOnDrop ownership), with bounded outbound line framing (128KiB), per-POST 30-second timeout, HTTP 200/202/204 acknowledgement and JSON-RPC results received only on SSE message events. Closing/failing the SSE stream cancels the writer and fails outstanding requests, **never automatically reconnecting or replaying an ambiguous tools/call POST**. Signed effect intent still belongs to the existing Agent journal, not this bridge.
- Source grants are still SourceKind::Mcp with new host-minted source epochs, exact actor/site/tool/effect, conservative Unknown even if a legacy server advertises readOnlyHint. Only --mcp-allow SERVER:TOOL registers and grants a current schema-validated tool; discovery alone does not authorize tool calls. The R5a bounded list pagination/schema/registry and standard tool execution path are reused without duplication. Cross-transport duplicate SERVER IDs fail before any provider contact.
- Legacy support is native CLI only. Browser/owner/MCP multiuser, unattended server starts, watcher/LSP, dynamic reconnect, legacy prompts/resources and background listChanged are not introduced in R5b.

## Implementation boundaries

**R5b is a transport-only module, not an independent JSON-RPC client.** Modern R5a HTTP and stdio remain unmodified in semantic behavior and share their tested credential/HTTP trust code. ClientVersion=2024-11-05 is explicit for this transport, rather than silently assuming a current initialized protocol. Output remains bounded text/structured JSON only; unsupported binary/interactive/task types are rejected through the existing aw-mcp RemoteTool.

## Real evidence to retain

- Rust aw-mcp native tests: SSE parser handles fragmented lines, CRLF, keepalive/multiline data and 128KiB bounds; same-origin endpoint acceptance, cross-origin/scheme/port/userinfo/fragment injection rejection, unexpected/non-JSONRPC SSE rejection, CLI named origin/transport/grant constraints.
- Freshly rebuilt **std-minimal and full** aw binaries using a disposable fake OpenAI tool-call server and independent Python 2024-11-05 two-endpoint SSE/POST servers: discovery only, no implicit grant, actual selected remote echo tool, modern flag refuses fallback, malformed schema input, missing capability denial, no --help side effects, cross-origin bearer safety, wrong MIME/missing endpoint/malformed JSONRPC refusal, non-loopback HTTP denial and failed POST sent once without model/account fallback. Python mock gives each GET a distinct session-specific SSE queue to prevent stale SSE clients intercepting a later session's replies.
- Current R5a modern/2025 initialized protocol and stdio actual-binary fixtures, R4 model catalog, R3b Gemini, R3a Anthropic, R2 search, JSONL and file tool loops remain separately tested. Std/full, std/JSONL minimal, no-state, Agent-core/SQLite provider/tests, PTY, source-bound reference and warning-denied Clippy/rustdoc must remain green.
- The optional core-only no_std/alloc capability consumer remains a separate zero-import WASM proof. No full no_std Agent or libc-free MCP networking library is implied. No production legacy MCP service or full cross-OS release has been tested.

## R5b fresh focused validation receipt (2026-10-09)

- Real freshly rebuilt **std/JSONL minimal** and **full** aw binaries passed **11/11 each** against isolated two-endpoint legacy SSE servers plus a separate fake model, including same-origin bearer/session handling, strict 2024 initialize, exact grants, no silent auto fallback, malformed MIME/SSE/JSONRPC, cross-origin announced endpoint refusal and one-only POST after ambiguous failure.
- The inherited modern RMCP HTTP/stdio test passed **11/11 each** in full and minimal. Earlier R4 catalog **8/8**, R3b Gemini **5/5**, R3a Anthropic **3/3**, R2 scoped search **4/4**, JSONL resume/recovery **6/6** and Phase2 tool fixtures **4/4** passed on freshly rebuilt minimal aw; no production provider accessed.
- Native tests: full CLI **260/260**, JSONL-minimal **88/88**, stateless **85/85**, aw-mcp **7/7**, providers **80/80**, SQLite store library **77/77**, tools **7/7**, aw-agent-core suites all green. Nix host used the existing explicit AW_GIT/AW_BASH overrides where needed.
- Whole workspace/all-target compilation and warning-denied Clippy, rustdoc, doctests, the actual PTY Inline/Scrollback/Fullscreen **3/3**, independent R1 no_std portable kernel and alloc consumer linked for wasm32v1-none with **zero WASM imports**, source-only R5b **80/80**, R5a **82/82**, comprehensive handoff **89/89**, source/static **zero errors and warnings**, validation-runner 44 tests, and source-bound reference profile passed. The source-bound reference run does **not** execute Rust or certify live providers by itself.
- Whole-repository cargo fmt --all -- --check still fails for **nine pre-existing, unrelated files** (provider/store/root formatting drift). Both aw-mcp Rust sources touched by R5b pass rustfmt independently. No broad unrelated reformatting was performed.
- No live deprecated remote MCP service, independent security audit, native Windows/macOS release, commercial provider billing or complete libc-free/no_std embedded Agent was claimed or tested.

## Remaining

R6 adds MCP prompts/resources/templates, pagination and bounded subscription/list_changed notifications under explicit grants. R7 extension hosting remains independent. R2b optional external rg/fd & Windows trusted shell is still pending. Future R5 enhancements (dynamic reload/reconnect) must use new source epochs and conservative no-automatic-effect-replay fencing.

## Primary sources

- https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2024-11-05/basic/transports.mdx
- https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- https://github.com/modelcontextprotocol/rust-sdk
- https://docs.rs/rmcp/3.5.0/rmcp/
- agentc 0.6.0 tests/mcp.sh for historical expected integration scenarios, not copied implementation

## Suggested next handoff

Read this R5b plan and AGENTC-PARITY-IMPLEMENTATION-PHASES-2026-10-09.md. Reanchor Git and live documentation. Proceed R6: MCP prompt/resource capability discovery over both current and legacy transports, explicit prompt/resource approval, URI/template validation, bounded pagination, lifecycle notifications and no grant escalation. Keep default std, no extra Agent loop, and no automatic server startup or uncertain replay. Do not rename historical Phases, push or revive 64/65 indexing/LSP.
