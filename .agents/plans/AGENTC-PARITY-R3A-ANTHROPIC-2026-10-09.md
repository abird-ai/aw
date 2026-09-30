# R3a — Native Anthropic Messages provider parity

Date: 2026-10-09. Track: agentc parity, R3a (not a historical numbered phase). Base: c695237, aw version 0.15.0, SQLite schema v33. Current implementation lives above the immutable Phase-68 tag. R3b Gemini remains a separate next phase.

## Scope and architecture

Native Anthropic Messages wire dialect, reusing the existing aw-agent-core model/tool/journal loop and aw-providers HttpProvider TLS/SSE plumbing. Nothing creates a second Agent loop, credential vault, retry scheduler, background listener, database or tool grant.

- New Anthropic provider kind and built-in profile: https://api.anthropic.com/v1, credential reference ANTHROPIC_API_KEY. First-run setup offers Claude API (Anthropic); this uses a supported Console API key, not consumer subscription OAuth.
- POST /v1/messages with required anthropic-version 2023-06-01, Bearer authentication when selected, and no silent alternative token/account or endpoint.
- The shared standard HttpProvider pins an in-memory SHA-256 digest of the first selected environment credential for its lifetime (Anthropic, OpenAI and Ollama alike). A later value change, missing reference or rotation fails visibly before continuing a tool round instead of silently moving to a different payer/key; explicitly start a new provider/Session context. The raw token exists only in a zeroizing per-invocation buffer and is never inserted into Session/config metadata. Independently, SecretStore-backed accounts remain version-fenced.
- Native system/user/assistant/tool content. Client functions use native name/description/input_schema, tool_use blocks decode bounded partial JSON and run only after a settled complete provider response and the existing model/tool permission checks. Contiguous tool_result blocks become one user message.
- Streaming text, tool_use, thinking, redacted_thinking, signature_delta, input_json_delta, explicit message_delta/message_stop, bounded block count/sizes, conservative stop reason/unknown-event handling and provider-report token usage.
- Provider-private continuation is strictly in-memory and bound to provider, selected model and destination. It preserves exact Anthropic block ordering, including interleaved text/tools and signed thinking, and checks public content/tool-call consistency before reuse. The underlying aw Session journal and serde history explicitly strip this private material; no persistent thinking signature or bearer disclosure.
- Native effort: low/medium/high/xhigh/max => output_config.effort with thinking adaptive. Instant explicitly requests disabled thinking. If a selected model rejects an option, the error is surfaced; no silent remap, automatic paid retry or older manual-budget probing.
- Input token summary includes reported normal/cache-write/cache-read counts with checked arithmetic. Missing provider usage remains unknown; estimates are not verified billing.
- Explicit native /models discovery and response parsing, with version/auth header; full catalog/metadata/cache/pagination is R4.
- The full SQLite SecretStore now accepts the Anthropic provider-kind and still enforces exact actor/model/executor/destination/version. No storage migration.

## Out of scope / remaining parity

R3b native Google Gemini generateContent/thought signatures. Older Claude manual-budget-only thinking modes. Model-specific capability discovery and pricing R4. Consumer Claude subscription OAuth is not presumed supported for third-party custom clients. Anthropic server-side autonomous tools, provider-specific beta features and multimodal blocks remain out of this text/tool R3a slice. No live provider account, real billing/cost, multi-OS release or libc-free Agent is claimed.

## Fresh R3a validation evidence and limitations

- Rust CLI: **full 259/259**, std/JSONL minimal **87/87**, stateless **84/84** (Nix AW_GIT/AW_BASH path overrides applied).
- Rust HTTP providers **67/67** tests, including native Anthropic SSE/body/continuation, explicit effort, token cache arithmetic and shared env-credential pinning. First-run native onboarding **5/5**.
- Full SQLite store library **76/76** tests including the Anthropic provider account/versioned-SecretStore and mismatched destination denial. aw-tools **7/7**, aw-agent-core all-target test suites green, no_std R1 kernel and independent consumer tests green.
- A freshly rebuilt **actual std-minimal aw binary** passes offline Anthropic SSE/tool mock **3/3** (native request headers/model, scoped file tool round-trip, no private thinking/bearer in persistent JSONL, no effect dispatch on incomplete stream). Prior R2 search actual binary **4/4**, Phase2 OpenAI/Ollama file-tool **4/4**, JSONL resume **6/6**. Native terminal Inline/Scrollback/Fullscreen PTY **3/3**.
- Whole-workspace/all-target compile and Clippy with `-D warnings` green; rustdoc with warnings denied, workspace doctests, 44 validation-runner unit tests and source/static reference profile green. Anthropic source checker **50/50**, R2 checker **50/50**, R1 **42/42**, comprehensive handoff integrity **89/89**. Static validation 1,068 active sources, 37 manifests, 1,045 authored Rust test attributes, 783 local links, no errors or warnings.
- Existing repository-wide rustfmt drift persists in unrelated files; no entire-repository formatting pass was claimed. The `anthropic.rs` new source is rustfmt-formatted. A complete native multi-target release, real Anthropic API account and billing, independent security audit, complete no_std Agent and complete agentc parity remain unclaimed.

## Evidence and acceptance

- Protocol Rust tests: ordinary text/usage, partial JSON tool input, thinking signatures, redacted thinking and malformed/truncated streams, interleaved text/thinking/tool ordering, public-private consistency and anti-forgery, effort settings, cache token accounting.
- Real minimal aw binary against disposable local fake Anthropic SSE: native path/header/key, actual scoped read-tool effect and complete two-request round; signature and bearer absent from JSONL; intentionally incomplete tool response never executes the proposed call.
- Shared provider credential-pin unit test verifies first selected environment secret, refusal after value mutation, and refusal after unsetting it; no network or sensitive values logged.
- Existing OpenAI Chat/Responses/Ollama, full CLI, minimal JSONL, stateless CLI, native onboarding, local SecretStore and tests remain independently validated.
- Source-only R3a checker runs in cheap reference profile; real binary offline Anthropic smoke runs after a fresh minimal build in opt-in Rust/full validation, not on ordinary startup.
- No hidden provider/model discovery and no background HTTPS access when opening the app or configuration.

Primary Anthropic documentation checked in October 2026:
https://platform.claude.com/docs/en/api/messages/create
https://platform.claude.com/docs/en/build-with-claude/streaming
https://platform.claude.com/docs/en/agents-and-tools/tool-use/overview
https://platform.claude.com/docs/en/build-with-claude/effort
https://platform.claude.com/docs/en/build-with-claude/extended-thinking
https://platform.claude.com/docs/en/manage-claude/authentication

## R3b next

Add native Gemini generateContent and streaming function calls in a separate provider module using the same canonical Agent state machine, with opaque thoughtSignature bound to its current provider/model/account/tool round, no private replay across migrations, and actual fake-provider tests. Keep R2b optional external rg/fd and Windows trusted-shell support separate, skip filesystem indexing/LSP phases 64/65, do not create a historical Phase69 or push.
