# R3b — Native Gemini GenerateContent provider (agentc one-way parity)

Date: 2026-10-09. Status: implemented and focused-validated; final Git checkpoint recorded at commit. Parent: AGENTC-PARITY-IMPLEMENTATION-PHASES-2026-10-09.md. Baseline: aw 0.15.0, SQLite v33, commit 81a8214. This is unnumbered R3b, NOT historical Phase 69. R2b external search and Windows shell remain separate.

## Design and scope

Gemini is a native GenerateContent wire dialect sharing the **one** aw-agent-core Agent loop, ToolPolicy and intent-before-effect journal, and the existing std reqwest/Rustls/SSE provider transport. It does not create a separate Agent, credential vault, context store or automatic retry.

- ProviderKind Gemini, built-in Google AI Studio API profile with GEMINI_API_KEY environment **reference**, and an explicit Gemini API first-run wizard option. No consumer-OAuth or Vertex-ADC fallback.
- Strict one-segment model validation and a model-scoped streaming URL ending in models/MODEL:streamGenerateContent?alt=sse; query/path injection is denied before contact. Google requires x-goog-api-key, not Bearer. The full SQLite versioned SecretStore now accepts Gemini provider kind with exact existing actor, model, destination, executor and version gates. Provider-account create CLI also offers Gemini (and Anthropic).
- Native contents with systemInstruction, user text, model text/functionCall and user functionResponse. Host ToolSpec's full JSON Schema is published via Google’s documented parametersJsonSchema field, not silently down-converted to its narrower OpenAPI parameters subset. The actual SDK Tool.validate and rooted grant checks remain authoritative. Some upstream providers have exhibited schema incompatibilities; no silent retry/fallback after a possible paid call is allowed.
- Streaming decoder bounds text, parts, signed continuation, tool calls, IDs, frames and usage; supports interleaved text, private thought text, functionCall and thoughtSignature parts. Public text appears as deltas; thinking remains private. Explicit STOP/finishReason and successful stream EOF are mandatory, never inferred from partial streamed arguments.
- Provider IDs are preserved exactly, including dash characters. Gemini 3 must supply a native ID and signed first function-call part. Parallel calls may have signature only on the first part. Legacy Gemini 2.5 calls without IDs get a unique reserved internal ID for the Agent tool loop; synthetic IDs are omitted from reconstructed provider-visible functionCall/functionResponse parts.
- One in-memory private continuation envelope contains original ordered provider parts and exact IDs, is scoped to provider/model/endpoint and cross-checked against the public assistant content and calls. It is stripped from regular Session JSONL/export by Message.provider_context serde skip and Agent journal sanitization. API keys likewise never enter config/JSONL; existing environment-key digest pinning protects in-flight custody.
- Usage reads provider-reported prompt, candidate and thought token counts where present; this is not a verified billing invoice. Gemini 3 low/medium/high map to native thinkingLevel. Instant is refused when no true no-thinking option is guaranteed, not silently mapped to minimal. Gemini 2.5 Flash/Lite can use Instant via thinkingBudget=0 where supported; the rest of 2.5 level-to-budget handling and xhigh/max remain explicit unsupported cases until R4 catalog.

## Verified behavior required to close R3b

- Provider unit tests: endpoint injection defenses, native function declaration schema, exact tool result grouping and name/ID correlation, sequential and parallel signed calls, interleaved thought/text, legacy missing IDs, forged public/private continuation refusal, missing signatures, truncated streams, rejected model errors, finish reason, thinking and token usage.
- Actual built minimal aw binary against isolated local Gemini SSE server: three requests spanning two signed file reads, native header (no Bearer), private signatures and credential absent from JSONL, unsigned/truncated write proposals execute no effects, missing key/model path injection never contact provider.
- Setup/discovery and full SQLite versioned provider account regression; inherited Anthropic/OpenAI/Ollama, JSONL, rooted tools, Agent/PTY, no_std portable R1 consumer and compiler/Clippy/docs/reference checks stay green.
- Reference profile provides source-only validation; optional full Rust profile freshly builds the minimal binary then exercises the real mock. No live Google account, billing, multi-platform release or independent security audit claim.

## Fresh validation receipt (2026-10-09)

- Native CLI suites: full **260/260**, JSONL minimal **88/88**, stateless **85/85**, with Nix host AW_GIT/AW_BASH overrides supplied for old subagent Git fixtures.
- Provider tests **75/75** after the Gemini usage-only frame regression, including Gemini native body/partial signed function calls, legacy IDs, provider/privacy tampering, thinking levels, stream termination and model URL injection. First-run Gemini choice **6/6** onboarding suite.
- Full SQLite store library **77/77**, including a versioned Gemini provider account and rejected changed destination. aw-tools **7/7**, aw-agent-core all-target tests and portable kernel/consumer tests green.
- Freshly built std/JSONL minimal binary: offline Gemini 3 stream mock **5/5** (three signed sequential model calls, exact key/header and ID, both scoped file results, no private signature/key JSONL, unsigned/incomplete writes never dispatch, missing key/invalid model path never contact provider). Inherited R3a Anthropic **3/3**, R2 scoped search **4/4**, JSONL resume/no-replay **6/6**, Phase2 OpenAI/Ollama tools **4/4** also passed. Actual Agent Inline/Scrollback/Fullscreen PTY **3/3**.
- Whole workspace/all-target compilation and Clippy warnings denied, rustdoc warnings denied and workspace doctests green. Independent wasm32v1-none portable R1 consumer linked with **zero imports**, on available Nix Rust 1.95.0 (not a complete no_std aw Agent and not the pinned Rust 1.98.1 release proof).
- R3b source-only checker **57 checks**, inherited R3a **50**, R2a **50**, R1 **42**, validation-runner unit suite **44**, static source/handoff tests and reference profile are tracked separately.
- Whole-repository cargo fmt remains red on nine historically unformatted files (including provider/store files already carrying drift before this change); the new gemini.rs source passes rustfmt. No sweeping unrelated formatter normalization was performed. No real Google account, provider billing, Windows/macOS native runtime, full libc-free Agent or independent security audit was attempted.

## Explicit limitations

Gemini GenerateContent is now Google's legacy API; a future Interactions adapter is a separate typed provider transport under the same Agent kernel. Model catalog, discovery caching, provider-specific feature negotiation and price schedules are R4. Vertex-specific identity, multimodal parts, Google-hosted agent tools, cross-restart signed continuation rehydration and no_std HTTP networking are not implemented in this phase. A completed Session's public history remains durable, but signed provider internals are never treated as persistable authority.

No std/libc requirement is being imposed on the normal CLI or provider crates. The E1 portable no_std kernel stays core-only, with later E2/E3 embedded host support. Skipped historical filesystem indexing/LSP phases 64/65 remain skipped, and no Phase 69 or push is implied.

## Primary current Google specification sources

https://ai.google.dev/api/generate-content
https://ai.google.dev/gemini-api/docs/generate-content/function-calling
https://ai.google.dev/gemini-api/docs/generate-content/thought-signatures
https://ai.google.dev/gemini-api/docs/generate-content/thinking
https://ai.google.dev/api
