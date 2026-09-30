# R4 — shared provider/model catalog, discovery, pricing and offline model controls

Date: 2026-10-09. Status: implemented and focused-validated. Scope: unnumbered agentc one-way parity R4, not historical Phase 69. Baseline: main 738d44d, aw 0.15.0, SQLite schema v33. User authorizes implementation; no push or retag. Standard Rust is preferred. The shared core-only no_std capability kernel remains unaffected; a complete embedded Agent is future E2/E3.

## Product and ownership

The entire local-model presentation and guided discovery reads one aw-providers catalog:
- Portable model metadata: exact ID, optional display name, input/context/output limits, tools/thinking capability flags when actually provided, lifecycle and optional price schedule. Unknown is represented as null/unknown, not zero/false.
- Explicit source: configured model, bundled dated reference, or provider_discovery. The latter is factual source-reported metadata, NOT an account/model authorization. Bundled reference names are only shown for exact official provider endpoints and do not pick a model, imply a billable API account or grant inference.
- Data-only OpenAI-compatible presets: OpenRouter, xAI, DeepSeek, Groq, Mistral and Together, each with an explicit supported endpoint and env *reference*. No key values in TOML or automatic network.
- Explicit model discovery for OpenAI Chat/Responses and compatible endpoints, Ollama, Anthropic and Gemini. Anthropic uses after_id/has_more; Gemini uses pageToken. Up to 16 pages, 2048 entries, 2 MiB/page, 8 MiB aggregate; cyclic/invalid/incomplete pagination is rejected without caching a partial catalog. For exact official OpenRouter endpoint alone, parse optional provider-reported USD/token price metadata. Custom endpoints do not inherit OpenRouter billing assumptions.
- Guided first-run discovery now delegates to the same catalog, reuses fresh cached data within 24 hours, and contacts a provider only when the user is explicitly in setup. No second model registry or background polling.
- Shared typed Clap namespace in both minimal and full binaries: aw model list/show/refresh and aw model price show/set/remove/estimate. List/show are offline; refresh or list --refresh explicitly contact the selected provider. The existing interactive /model command remains a direct model change; existing first-run chooser already consumes the same discovered IDs. More advanced standalone filter/switch picker interactions can be refined in R11 without an extra engine.
- Precise integer micro-USD-per-million pricing, with user overrides separate from provider-reported rates. Missing rates remain unknown. Estimates are not invoices and never imply an account or spend permission. No default current-rate claims for commercial providers.
- Provider read/transport/credential boundaries unchanged; no provider, account, tool or route is selected based on discovery metadata.

## Cache and privacy

- Private user-local XDG model-catalog cache (or HOME fallback), no project auto-discovery, no creation on missing offline lookup. Lock-protected user price mutations and atomic owner-only metadata writes reuse aw-platform. Cache entry and aggregate sizes are bounded.
- Provider kind, exact endpoint and selected environment credential reference determine cache identity. A random-salted SHA-256 verifier further pins the actual selected API secret within metadata and price files, so key rotation or a missing secret cannot expose or attribute cached another-payer model data or prices. Raw token, unsalted key hash and refresh credentials are never in the catalog. A key is resolved once and passed through the whole discovery request and cache commit, so a changed environment during a paginated run does not change payer.
- The default local read handles missing cache and unknown models without any network request. Incomplete pages/error do not replace a known last-good snapshot.
- OAuth/subscription-backed discovery must use its existing specialized authorization path; the generic catalog cannot silently reuse plan tokens or bind discovered models as granted accounts. Users can always enter a valid model ID manually; this still must pass normal run and account admission.

## Testable acceptance

1. Rust unit tests for decimal conversion, overflow, ordinary cached-token estimation and unknown cache pricing; private file permissions, independently locked concurrent overrides, salt/account rotation, official-host-only references, TTL and model limit validation.
2. Offline actual-binary fake-provider fixture with exact Gemini/Anthropic paginated paths/headers, OpenAI and Ollama, cache freshness and forced refresh, no startup/config file creation, no raw key in cache, no second payer reuse, user rate set/estimate/remove, and cycle/no partial-cache regression.
3. Existing R3b native Gemini, R3a native Anthropic, R2 on-demand tools, R1 zero-import core-only no_std authorization consumer, full/minimal/stateless CLI, SQLite SecretStore, TUI/PTTY, all-target compile, Clippy warnings denied, Rust docs and source-bound reference profile remain valid.
4. No live commercial provider, current provider-rate guarantees, independent security audit, six-target release or complete libc-free embedded Agent is certified by R4.
5. Historical Phase 68 and SQLite schema v33 unchanged, skipped Phase64/65 indexing/LSP unchanged, no push.

## Fresh implementation evidence (2026-10-09)

- Full CLI native suite **260/260**, JSONL-minimal **88/88**, stateless **85/85**, providers **80/80** including five new catalog/cache tests, full SQLite store **77/77**, and core/tool suites green. Nix-host Git-dependent suites used explicitly selected AW_GIT/AW_BASH paths.
- Rebuilt minimal actual-binary offline provider catalog **8/8**: Gemini and Anthropic authenticated cursor pages, OpenAI/Ollama metadata, zero-network list with no cache creation, per-account salt and rate changes, fresh/stale/forced refresh, unknown price/cost handling and bounded cycle rejection. No real provider contacted.
- Real earlier minimal-binary provider/tool regressions: Gemini native signed tools **5/5**, Anthropic native tool SSE **3/3**, R2 bounded search **4/4**, JSONL no-replay/session recovery **6/6**, Phase2 OpenAI/Ollama file-tool **4/4**.
- Actual native Agent PTY Inline/Scrollback/Fullscreen **3/3**; independent R1 core-only/alloc no_std wasm32v1-none consumer linked with zero imports (NOT complete embedded aw Agent).
- Whole-workspace/all-target compile, Clippy with warnings denied, rustdoc warning denied, workspace doctests, 44 validation-runner tests and source/static/reference profile passed. R4 source-only checker **93** currently; R3a/R3b checkers changed to verify the new shared parser rather than obsolete function names while preserving native provider contract checks; handoff integrity **89**, static validation zero errors/warnings.
- Bundle reference entries have explicit 2026-10-09 dates and are NOT verified availability for a user account; no seeded commercial rate is asserted. Current provider preset endpoint selection was source-checked, and Together remains the current official .xyz address rather than agentc’s historical .ai.
- Full-repo cargo fmt check remains red due to pre-existing unrelated formatting drift. The three new/rewritten R4 source files are rustfmt-clean; no broad reformatting was performed. No current live provider/model pricing/billing, native macOS/Windows release, independent security audit, or full no_std Agent was verified.

## R4 follow-up outside this independently reviewable slice

- Full provider-specific metadata catalogs for inference parameters including explicit Gemini 2.5 budget choices and older Claude manual thinking, with versioned accepted schema; do not hard-code provider protocol fallback based on an untrusted model label.
- Better native model picker filtering/comparison, built-in price-sync UX, status usage overlay and current subscription quota only through supported live account custody; source+timestamp labels remain required. This is UX and data enrichment, not a second catalog engine.
- MCP tools/extensions R5–R7 and E2/E3 portable Agent extraction are the next core parity prerequisites; R2b external rg/fd process and Windows shell remain separately reviewable.

## Research/source contracts

Official reference documentation checked October 2026:
- OpenAI Models endpoint https://platform.openai.com/docs/api-reference/models/list
- Anthropic Models endpoint https://platform.claude.com/docs/en/api/models/list
- Gemini Models endpoint https://ai.google.dev/api/models
- OpenRouter model metadata and pricing https://openrouter.ai/docs/api/api-reference/models/get-models
- xAI https://docs.x.ai/docs/api-reference
- DeepSeek https://api-docs.deepseek.com/
- Groq https://console.groq.com/docs/openai
- Together https://docs.together.ai/docs/openai-api-compatibility
- Cargo additive features https://doc.rust-lang.org/cargo/reference/features.html
