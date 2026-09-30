# R5a — Native MCP tools through the official Rust SDK

Date: 2026-10-09. **Track:** unnumbered R5a of agentc → aw one-way parity, not historical Phase 69. Base main 9ca81a3, version 0.15.0, SQLite v33. Immutable historical phase-68 and skipped Phase 64/65 indexing/LSP remain unchanged. No push.

## Objective and scope

Implement MCP tool connections in standard Rust aw full/minimal through the existing one Agent tool execution, journaling, authorization and no-replay logic. Explicitly connect a named stdio executable or Streamable HTTP URL, negotiate with the official Rust MCP SDK, discover schemas, and require a host-selected exact server/tool grant before the model sees or invokes a tool. Never start a server just from app initialization/help. Use existing source-bound grants and keep R1 no_std portable kernel independent of networking/process dependencies.

## Shipped design

- New std-only aw-mcp crate with official rmcp 3.5.0 (client, async-rw, Streamable HTTP reqwest transport) and local bounded JSON Schema validator. SDK handles protocol lifecycle; no separate Agent loop or bespoke transport serializer.
- The official SDK's convenience child-process feature would pull process-wrap/nix 0.31.1 and conflicting libc version into the existing locked platform stack, so we use Tokio Command and rmcp::transport::async_rw::AsyncRwTransport over owned child stdin/stdout. Only explicit absolute executables; no shell, PATH lookup or ambient child environment; selected env variables are opt-in; kill child on drop.
- CLI options in both full and std minimal: --mcp-stdio SERVER=/ABS/EXEC, --mcp-arg SERVER=ARG, --mcp-pass-env SERVER=ENV, --mcp-http SERVER=URL, --mcp-auth-env SERVER=ENV, --mcp-ca-file SERVER=/ABS/PEM, --mcp-allow SERVER:TOOL.
- Separate aw mcp inspection command connects only to explicitly named servers and lists bounded tool names and unknown effect status, without creating an Agent, granting a tool or calling a model. No MCP work on plain aw --help, profile viewing or no flags.
- Bounded max 8 servers, max 32 total tools, 8 list pages, 16 child arguments per stdio server, 12 env pass entries. HTTP must be HTTPS except exact loopback hosts; no redirects, no proxies, no URL userinfo/query/fragment. Optional explicit 1MiB PEM trust bundle with hostname verification; cleartext loopback never weakens TLS for non-loopback.
- RMCP Auto lifecycle selects modern MCP protocol 2026-07-28 discovery or legacy initialization protocol 2025-11-25. The 2024-11-05 deprecated two-endpoint HTTP+SSE transport is NOT implemented and is tracked as R5b.
- External tool names are composed as mcp__SERVER__TOOL with portable name bound and collisions rejected. **Only explicitly granted tools are schema-compiled and registered**; ungranted advertisements cannot consume Agent registry slots, even with malicious schemas. The complete selected batch is preflighted against all existing registered tool names (including ungranted names) and the registry's 32-tool ceiling before the first MCP registration. Remote descriptions <=8192 bytes; JSON Schema <=64KiB, bounded nodes/depth, no external URL references. Compile local JSON Schema once and check every actual tool input before dispatch.
- All remote tool effects are Unknown regardless of remote readOnlyHint annotations, since metadata is untrusted. Registration creates no grant. A host-minted random SourceKind::Mcp epoch is bound to the exact actor/site/tool/effect. Only exact --mcp-allow SERVER:TOOL grants current advertised tools; invalid grant fails before any model provider invocation.
- Tool calls go through existing Agent intent-before-effect journal, cancellation and result paths. Use call_tool_once, deny interactive/task requirements and non-text media until a separate protocol/media grant; bounded 128KiB text/structured JSON output. Disabled session auto-reinit/replay on ambiguous external effect. No automatic router/account/tool fallback.
- Native CLI only. Browser/web/owner MCP rejected in R5a; no implicit multi-user delegation, child/subagent widening or persisted authority restored from saved Session text. Stdio child permissions follow the OS user; MCP itself is not a filesystem/kernel sandbox.

## R5a actual-binary tests

Tools/agentc_r5_mcp_smoke.py drives freshly compiled std-minimal **and full** aw binaries against a local fake OpenAI tool-call model and two independent isolated MCP servers, including an explicitly launched real Python stdio process.

1. Discovery-only aw mcp sends only tool listing requests, without a model/tool call or new grant.
2. A discovered MCP tool with readOnlyHint=true is not authorized or sent to the model without explicit host grant.
3. Explicit modern Streamable HTTP call with 2025-11-25 initialization returns a bounded echo result through the existing Agent/tool journal.
4. A catalog of 32 tools, including 31 hostile ungranted schemas, permits its single exact approved tool without registering other 31.
5. A 30-tool approved batch plus existing built-ins exceeds the bounded 32-tool registry and is refused before provider contact or partial registration.
6. Invalid schema input denied before remote tools/call.
7. Off-loopback cleartext HTTP rejected without provider/network call.
8. Grants naming undiscovered capabilities are denied before model invocation.
9. Absolute stdio Python process completes the MCP tool call with cleared child env/no shell.
10. Modern-only 2026-07-28 discover and stateless tool call with correct protocol metadata, without legacy initialize.
11. Help has no network or MCP child side effect.

Rust unit tests cover options/URL/source constraints, hostile read-only hints, JSON Schema nesting and external-ref denial. Source-only checker is not runtime proof; separate Rust/build/binary/PTY/no_std regressions are required.

## Fresh local closeout evidence (2026-10-09)

- **Compiled minimal and full binaries each passed 11/11** isolated MCP fake-model and HTTP/stdio scenarios, including 2026-07-28/2025-11-25 negotiation, exact grants, hostile ungranted schemas, and atomic over-capacity denial. These are mocked services, not live production MCP certification.
- R5a Rust crate unit tests **3/3** and source-bound checker **82/82** passed. Whole workspace compilation, Clippy with warnings denied, rustdoc and doctests, inherited full/minimal/stateless CLI, provider/SQLite and prior R1–R4 test suites, PTY and core-only `no_std` zero-import WASM consumer were run as independent regressions (see current BUILD-STATUS and reference evidence).
- The portable kernel consumer passed for wasm32v1-none with zero imports; this **does not** prove a fully embedded Agent or libc-free executable.
- Unrelated existing whole-repository rustfmt drift remains separate. New aw-mcp and mcp_cli modules are formatted; no sweeping formatting normalization is part of R5a.

**2026-10-09 R5b implementation successor:** [AGENTC-PARITY-R5B-LEGACY-SSE-2026-10-09.md](AGENTC-PARITY-R5B-LEGACY-SSE-2026-10-09.md) adds the previously missing explicit 2024-11-05 two-endpoint legacy SSE transport; the R5a gap list below is retained as the historical closeout state. R6 prompts/resources/list_changed and dynamic reconnect remain open.

## Explicit gaps and follow-up

- **R5b old two-endpoint HTTP+SSE**: deprecated MCP 2024-11-05 transport is not in rmcp's current official client. To achieve actual legacy server parity, a separately tested opt-in backwards-compat transport is required; do not conflate current 2025-11-25 initialized Streamable HTTP with the 2024 transport.
- R6 MCP prompts, resources, resource templates, list_changed subscriptions and invalidation, bounded resource text/media.
- Dynamic server refresh, reconnect/backoff and source epoch revocation are deferred; no automatic tool replay across uncertain effects, and no grant borrowed merely because a replacement server has the same name.
- The current remote HTTP client uses explicit TLS verification; live remote account, OAuth, custom CA actual handshake, Windows/macOS released builds, independent security assessment and performance/size benchmarks remain unverified.
- Full std and std-minimal must continue to pass all established R1–R4, PTY and JSONL/no-replay gates. Full no_std Agent/host with libc-free final artifact is independent E2–E6 work, not delivered by std RMCP.

## Implementation commands

    cargo test -p aw-mcp --lib -q
    cargo build --locked -p aw-cli --bin aw --no-default-features --features minimal
    python3 the removed agentc_r5_mcp_smoke helper --binary target/debug/aw
    python3 the removed test_agentc_parity_r5 helper
    cargo check --workspace --all-targets
    cargo clippy --workspace --all-targets -- -D warnings

## Sources

- https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- https://modelcontextprotocol.io/specification/2025-11-25/server/tools
- https://github.com/modelcontextprotocol/rust-sdk
- https://docs.rs/rmcp/3.5.0/rmcp/
- Local agentc 0.6.0 README.md and tests/mcp.sh through dotlink L5 (historical behavior, not copied implementation)
