# Phase 37 — first-run provider/model onboarding

**Status:** implementation and validation complete; committed/tagged at `9941725 / phase-37`; awaiting user design review.
**Schema:** SQLite v20 unchanged.  
**Product boundary:** provider/model configuration before inference; no new database or authority protocol.

## Goal

Bare interactive `aw` must not fail just because no executable provider/model is configured.

When a TTY user starts `aw` without an effective model, the normal Pi-like inline surface now presents a concise first-run provider/model flow before any model-bound Session is created. The same provider editor is also reachable later from native `/settings`.

Local single-user `aw web` already supports zero-model Settings before inference and remains unchanged in that respect.

## Supported first-run choices

The native onboarding flow exposes only provider paths implemented by the current provider layer:

1. **Local Ollama**
   - provider: Ollama native `/api/chat`;
   - default endpoint: `http://127.0.0.1:11434`;
   - explicit model required.

2. **Ollama Cloud**
   - provider: Ollama native `/api/chat`;
   - endpoint: `https://ollama.com`;
   - config stores only the environment-variable name `OLLAMA_API_KEY`;
   - explicit model required.

3. **OpenAI API**
   - provider: current OpenAI-compatible Chat Completions adapter;
   - endpoint: `https://api.openai.com/v1`;
   - config stores only `OPENAI_API_KEY`;
   - keeps the existing OpenAI wire defaults:
     - `max_completion_tokens`;
     - streamed usage enabled;
   - explicit model required.

4. **Custom Ollama endpoint**
   - explicit endpoint;
   - optional API-key environment-variable name;
   - explicit model.

5. **Custom OpenAI-compatible endpoint**
   - explicit endpoint;
   - optional API-key environment-variable name;
   - explicit model.

6. **Skip for now**
   - leaves the process intentionally unconfigured;
   - native `/settings` and local-web Settings remain available later.

## Deliberate non-feature: ChatGPT subscription OAuth

The current `aw` provider implementation speaks Ollama native chat and OpenAI-compatible **Chat Completions**. It does not yet implement the OpenAI **Responses API** / ChatGPT-plan OAuth path.

Therefore Phase 37 does **not** advertise a “ChatGPT subscription” onboarding choice and does not attempt to reuse a ChatGPT-plan token with the current Chat Completions adapter.

That path can be added later only with real protocol/auth support; the Phase-37 profile/settings architecture is designed so such a provider can plug into the same persisted model without another onboarding redesign.

## One shared configuration authority

Provider/profile mutation is centralized in:

```text
aw_providers::config::ProfileUpdate
aw_providers::config::apply_profile_update(...)
```

This helper validates:

- profile name;
- provider kind;
- endpoint security rules;
- explicit/default model requirements;
- API-key environment-variable **name** syntax;
- provider wire fields such as token-field selection and streamed usage.

It returns a new `FileConfig` plus validated `Profile`; persistence remains the existing atomic/private `config::save(...)`.

The same helper is used by:

- first-run native onboarding;
- native provider setup launched from `/settings`;
- local single-user web Settings.

There are not three independent profile mutation implementations.

## Native UI behavior

### First run

The wizard is a Ratatui inline component:

- terminal-owned scrollback is preserved;
- provider choices use the Codex-style highlighted row;
- text entry stays inside Pi-like prompt rules;
- Enter advances;
- Esc goes back / skips from the first page;
- Ctrl-C skips;
- ordinary field editing and Save are provider-network-free; after endpoint/auth-reference entry the user may explicitly start bounded model-catalog discovery, which is a separate GET request and never sends a prompt.

### Native `/settings`

The existing in-editor Settings list now includes:

```text
Provider & model…    open
```

Selecting it opens the same provider editor.

During an active turn, provider setup is refused until the turn settles. Thinking/tool presentation toggles remain presentation-only.

### Fullscreen

Selecting Provider & model from fullscreen Settings returns through the normal Host ownership boundary, runs the same provider editor, then re-enters fullscreen. No second provider editor exists.

## Context-separation rule on provider change

Changing provider/model from native Settings:

- atomically persists the new profile metadata;
- updates the selected profile;
- clears the current in-memory Agent;
- detaches the current persistent Session handle;
- clears an old explicit account selector;
- clears retry/performance transient state.

Saved history itself is not deleted.

This prevents the old conversation context from being silently disclosed to a newly selected endpoint. The next model turn starts from a fresh model context unless the user later performs an explicit resume/context-transfer flow under the existing rules.

## First-run gate

The native first-run wizard appears only when all are true:

- effective profile has no model;
- not owner/service/web mode;
- not `--print`;
- no immediate positional prompt;
- not a management/import/context-action operation;
- stdin and stdout are TTYs.

It is bypassed when:

- an explicit `--model` is supplied;
- a saved Session supplies its historical model hint;
- the command is web/agent/print/management/non-TTY;
- a model is already configured.

## Network-effect boundary

Saving onboarding configuration is configuration-only.

The onboarding renderer/state machine itself contains no provider client, Agent prompt call, network socket, or HTTP client. Guided model discovery is a separate explicit call through `aw::discovery::discover_models` after endpoint/auth-reference entry. That discovery performs only the bounded provider catalog request; Save itself still performs no provider/inference request.

The actual-binary the removed onboarding_smoke helper:

1. starts bare zero-model `aw` in a real PTY;
2. selects a custom OpenAI-compatible endpoint backed by a fake HTTP catalog server;
3. explicitly discovers `fixture-model` through `GET /v1/models`;
4. selects that model and saves the profile;
5. quits without sending a prompt;
6. verifies the generated config;
7. asserts that the **only** provider traffic before the first prompt was the explicit discovery GET and that no POST/inference request occurred;
8. separately proves **Skip for now → composer → first Send reopens setup → second Skip returns to the preserved draft** without persisting provider config.

## Secret custody

Phase 37 never asks the user to type a provider secret value into ordinary config.

It stores only environment-variable names such as:

- `OLLAMA_API_KEY`;
- `OPENAI_API_KEY`.

Existing SecretStore/provider-account custody remains separate and unchanged.

## Preserved boundaries

Phase 37 does not change:

- SQLite schema v20;
- Phase-35 owner/direct lock separation;
- no-replay/unknown-effect semantics;
- stable users, writer epochs or semantic replication;
- the Phase-36 typed Clap command tree;
- zero-model/no-pair local web startup;
- Pi-like regular TUI / Codex-like fullscreen behavior;
- provider-private reasoning privacy;
- F6/F7 presentation-only thinking/tool visibility controls.

## Validation

Final Phase-37 checkpoint evidence is green:

- focused/reference checkers Phase29–37: **75 / 142 / 159 / 120 / 119 / 101 / 47 / 88 / 49**;
- default/minimal/web/net/OIDC feature cuts: green;
- full `cargo check --locked --workspace --all-targets`: green;
- full `cargo test --locked --workspace --all-targets`: green;
- actual-binary CLI/help smoke: green;
- actual-binary idle inline TUI smoke: green;
- actual-binary live-turn F6/F7 smoke: green;
- actual-binary **first-run onboarding → explicit model discovery → save config with no extra provider/inference request** smoke: green;
- actual-binary **Skip for now → composer → first Send reopens provider setup with draft preserved** smoke: green;
- actual-binary zero-model local-web Settings/workspace smoke: green;
- actual-binary fullscreen Git/PTY workspace smoke: **9/9**;
- source/static validation: **811 active files**, **87 Python sources**, **689 local links**, **0 errors / 0 warnings**;
- design-doc validation: green;
- complete reference profile: **39/39**;
- validation plans: **106 Rust / 108 full**.

The separate full-host Clippy/Nix release gate remains environment-pending and is not claimed here.

## Review boundary

After Phase 37 is committed/tagged, **stop for user design review**.

Do not begin a Phase 38 unless explicitly requested.

## Post-completion user-design adaptation

The user subsequently requested an **unnumbered adaptation of the Phase-37 review surface**, without authorizing Phase 38. That newer review tree is recorded in [PHASE37-DESIGN-ADAPTATION.md](PHASE37-DESIGN-ADAPTATION.md).

Important supersessions:

- fullscreen now defaults to a 22.5% Sessions-first Explorer beside Chat+Activity and adds full pointer interactions/contextual Alt navigation;
- Files/Sessions/Git expose the reviewed tree/list/fold/worktree behavior;
- the browser root is now SolidJS with Abird design language;
- OpenAI has since published a supported open-source Sign in with ChatGPT path for eligible plans, so the adaptation adds a **dedicated Responses + OAuth** implementation;
- the original prohibition remains: ChatGPT-subscription credentials are never routed through the Chat Completions adapter;
- ordinary provider/profile Save remains network-free; explicitly starting ChatGPT login is an authentication action, not a Save side effect.

Read the adaptation document immediately after the comprehensive handoff when taking over the current tree.
