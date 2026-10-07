# Phase 37 post-completion design adaptation — 2026-10-02

**Status:** unnumbered user-design adaptation on top of the completed/tagged Phase 37 implementation. This is **not Phase 38**. The existing `phase-37` tag remains at `9941725`; the documentation handoff commit `70165a7` remains above it. Do not move/rewrite either history point.

This document records the user-requested review changes made after Phase 37 was declared complete. When it conflicts with older Phase-37 presentation/provider notes, this document describes the newer product surface. Architecture/safety invariants from `COMPREHENSIVE-HANDOFF.md` still dominate.

## User-design corrections

The adaptation corrects the following review feedback:

- slash autocomplete belongs immediately **above the composer**, never at the top of fullscreen;
- fullscreen must be fully mouse-usable: click controls/views and drag split boundaries;
- Explorer order is **Sessions / Files / Git**;
- default fullscreen geometry is **22.5% Explorer / 77.5% main**;
- the main conversation tab is labelled **Chat**;
- file/diff tabs use bounded filename-aware labels;
- Files, Sessions and Git Changes expose **Tree/List** presentation;
- Sessions tree nests every branch/fork under its Session;
- Git change groups (Conflicts/Staged/Unstaged/Untracked) fold independently;
- Git exposes Changes / Worktrees / Branches and selecting a worktree changes only that navigator's human Git context;
- workspace colour/focus language is restored using restrained Abird green/blue accents;
- `Alt+E` focuses Explorer, `Alt+W` focuses the main content section;
- `Alt+1..9` is contextual to the focused section; numbered hints render only in that section;
- `Ctrl+Shift+P` accepts terminal stacks that encode Shift as uppercase `P` without a separate SHIFT modifier bit;
- the browser root is rebuilt as SolidJS with a minimal Pi-Web/ChatGPT-like layout using Abird design language.

## Native workspace implementation

### Default layout

`aw-workspace::Workspace::default()` now starts with:

- a column split at ratio `2250`;
- left pane: Sessions navigator;
- right group: Chat (active) + Activity;
- existing tab IDs/resources and Activity default are preserved.

No saved layout is rewritten. Restored user layouts retain their persisted structure.

### Mouse/pointer contract

Fullscreen enables Crossterm mouse capture for the lifetime of the alternate-screen guard and disables it on exit.

Pointer hit regions cover:

- panes/focus;
- Explorer navigator selectors;
- Tree/List selectors;
- Git Changes/Worktrees/Branches selectors;
- navigator rows;
- content tabs;
- composer;
- slash suggestion rows;
- Settings rows;
- command-palette rows;
- split dividers.

Dragging a split divider invokes the pure layout `ResizeBetween` action. Mouse geometry is presentation-only and cannot change root/provider/account/tool authority.

### Explorer contract

Explorer order and number mapping:

1. Sessions
2. Files
3. Git

`Alt+E` focuses Explorer. `Alt+W` focuses the last/current content group. With Explorer focused, `Alt+1..9` selects the corresponding navigator. With a content group focused, the same keys select content tabs. Number hints are not permanently painted in inactive regions.

Files and Sessions use the same `Tree | List` presentation vocabulary. `t` toggles the current navigator mode. Files tree inventories are lazy and bounded. Sessions tree is built from durable Session + branch metadata; viewing a branch does not dispatch a model.

Git retains its existing bounded typed worker and now surfaces:

- Changes / Worktrees / Branches;
- Tree / List for Changes;
- independent collapsible change groups;
- worktree discovery and human-scope switching without moving the Agent or Files root.

### Labels and colour

The active conversation target is presented as **Chat**. File tabs use the filename and Diff tabs use `Diff: <filename>`, with cell-aware truncation. Active focus/tab highlighting uses the Abird green/blue family while keeping Ratatui terminal compatibility.

Provider-private reasoning remains non-renderable. F6/F7 remain presentation-only.

## Provider/settings adaptation

The initial Phase-37 rule remains true in its narrow form: **ChatGPT subscription credentials must never be sent through the Chat Completions adapter.**

After Phase 37 was tagged, OpenAI published a supported open-source **Sign in with ChatGPT** contract for eligible ChatGPT plans that targets the public Responses API. The adaptation therefore adds a **separate** Responses provider/auth path rather than weakening the original boundary.

The guided provider surface is now intentionally small:

- ChatGPT subscription;
- Ollama Cloud;
- custom OpenAI-compatible Chat Completions;
- custom OpenAI-compatible Responses API;
- Skip.

Local Ollama remains a built-in/explicit config target but is not duplicated as a first-screen guided choice.

### ChatGPT subscription flow

The implementation follows the supported dynamic-registration contract:

- persisted installation host ID in canonical `urn:uuid:<uuid>` form;
- first authorization uses dynamic client registration;
- loopback callback binds `127.0.0.1` on an ephemeral port before the browser is opened;
- fresh OAuth state, nonce and PKCE S256;
- exact requested resource/scope contract;
- callback must return a newly issued client ID;
- token exchange/refresh use that issued client ID;
- ID token signature/issuer/audience/nonce are verified with discovered JWKS;
- the verified exact issuer+subject identity is kept with that OAuth credential inside SecretStore;
- access/refresh/ID tokens are stored only in SecretStore;
- access-token refresh is serialized per registered account so rotating refresh tokens cannot race;
- cancel/disconnect/ready-setup expiry clears local credentials and attempts refresh-token revocation through the discovery document's revocation endpoint; an unconfirmed remote revoke is surfaced rather than silently called success;
- model catalog is fetched only after explicit authorization;
- only models with list visibility are offered;
- selected model is persisted through the same `ProfileUpdate/apply_profile_update` configuration authority.

The dedicated Responses adapter uses `store:false` and streaming. System instructions are projected into Responses `instructions`, not a System input item. The ChatGPT-plan path strips unsupported tuning fields. Function tools use the supported additional-tools developer item for this route.

With `store:false`, opaque encrypted provider reasoning continuation items may be retained only for the same in-flight provider tool loop. They are never shown as reasoning text and are not written to the durable journal. Existing context rebuild/switch logic clears provider-private continuation before another model/account endpoint.

Opening or saving ordinary provider settings remains network-free. Starting **Sign in with ChatGPT** is an explicit network/auth action and is not described as a configuration-only save.

## SolidJS browser root

The root browser UI is now a compiled SolidJS application embedded from `web/dist`.

Build inputs are pinned by `web/package-lock.json`:

- SolidJS;
- Vite + vite-plugin-solid;
- esbuild;
- markdown-it.

Raw Markdown HTML is disabled. The compiled root uses the official Abird Round Trip SVG copied exactly from the canonical design-v0.7 asset.

Primary shell:

- narrow Sessions / Files / Git Explorer;
- single Chat/content area;
- compact top bar;
- floating bottom composer;
- responsive mobile drawer;
- light/dark design tokens using Abird green/blue/orange.

The Solid settings surface can edit secret-free profile metadata and can initiate the explicit ChatGPT OAuth flow. The browser receives only authorization URL/setup status/model-catalog metadata; OAuth tokens stay host-side in SecretStore.

The older hand-written browser modules and their typed backend APIs remain in the repository as compatibility/reference surfaces while advanced terminal/site/context-flow/group-workbench UI is migrated. They are not the active root frontend.

## Invariants explicitly preserved

This adaptation does **not** change:

- Domain -> Workspace -> Session ownership;
- stable opaque user identity;
- writer epoch/head/revision CAS;
- unknown external effects are never silently replayed;
- paid results survive attachment conflicts;
- semantic synchronization instead of SQLite/WAL replication;
- typed MachineLink capabilities;
- SecretStore/environment-reference credential custody;
- exact issuer+subject OIDC identity rules;
- explicit materialization/model-context projection;
- transitive Phase-33 provenance/revocation;
- Phase-35 owner/direct locking;
- trusted-direct current-schema/local-filesystem/no-migration restrictions;
- Phase-36 typed Clap command tree;
- one shared profile-update authority;
- provider/model switching cannot silently carry old model context;
- F6/F7 remain presentation-only;
- SQLite schema remains v20.

## Validation evidence for this adaptation

Executed in the live repository:

- `cargo fmt --all -- --check`: green;
- `cargo check --locked -p aw-cli --all-targets`: green;
- `RUSTFLAGS=-Dwarnings cargo check`:
  - default/all-targets: green;
  - no-default-features: green;
  - each individual feature `web workspace client device team net oidc`: green;
- `cargo test --locked --workspace --doc`: green;
- `cargo test --locked --workspace --all-targets` with actual NixOS `AW_GIT`/`AW_BASH`: green;
- `cargo test --locked -p aw-cli --bin aw`: 87/87 green;
- `cargo test --locked -p aw-workspace --test workspace`: 42/42 green;
- `cargo test --locked -p aw-providers`: **47/47** green, including fail-closed Responses `[DONE]`/incomplete handling;
- `aw-store` unit suite: **57/57** green, including SecretStore-only exact ChatGPT issuer+subject custody;
- feature-cut test/check matrix (minimal/web/workspace/client/device/team/net/oidc): green after feature-gating cleanup;
- warning-denied `cargo check` for default, minimal and every individual feature cut: green;
- browser pure-model tests: **98/98** green;
- actual-binary first-run onboarding/no-contact smoke: green;
- actual-binary fullscreen workspace smoke: **10/10** green, including the real PTY Ctrl+Shift+P→Ctrl+P fallback path;
- actual-binary local-single-user web/fake-provider smoke: green;
- actual-binary embedded Solid UI smoke: green;
- Phase-32 OIDC/OAuth source checker: **121** checks green;
- Phase-37 onboarding source checker: **48** checks green;
- clean `npm ci` from `package-lock.json`: green; rebuilding reproduced the exact committed `web/dist` hashes;
- `npm audit`: **0 vulnerabilities**;
- `npm run build`: green;
- canonical/public/compiled Abird Round Trip SVG SHA-256 values are identical;
- `python3 the removed validate_source helper .`: **826 active files / 717 Rust test attributes / 705 local links / 0 errors / 0 warnings**;
- `nix-instantiate --parse flake.nix`: green;
- `python3 the removed validation_gate helper run --profile reference`: green;
- `git diff --check`: green at the corresponding checkpoint.

Two environment limitations must remain explicit:

1. `cargo clippy` is not installed in the connected host toolchain. Warning-free `cargo check` with `RUSTFLAGS=-Dwarnings` was used, but this is **not** a claim that Clippy ran.
2. Nix evaluation/build cannot be executed through this connector because its sandbox sees `/nix/store` read-only. The source fileset was nevertheless narrowed so `web/node_modules` is not captured by Nix build inputs.

No live OpenAI/ChatGPT login was performed during automated validation because that requires explicit human authorization. The OAuth implementation is protocol/static/unit validated; do not claim a completed real-account login until one is actually exercised.

The current product slice implements initial connection, model discovery, serialized token refresh, and disconnect/revocation handling. It does **not** yet claim the richer returning-account reauthorization UX described by OpenAI (for example account-picker reauthorization using the existing issued client registration plus retained ID-token/login hints), nor the special invalid-grant recovery path that preserves/reuses an issued registration across a failed first code exchange. Those should be completed as account-management refinement rather than silently approximated.

## Review boundary

This tree remains a **user design review checkpoint**. Do not create Phase 38, move `phase-37`, rewrite history, or push unless the user explicitly asks.
