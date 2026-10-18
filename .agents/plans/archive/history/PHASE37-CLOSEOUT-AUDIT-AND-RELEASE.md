# Phase 37 closeout audit and release architecture — 2026-10-02

Status: unnumbered closeout on top of Phase 37. This is not Phase 38. Preserve the historical phase-37 tag at 9941725. The prior design-adaptation checkpoint is 1e257d4. Do not move phase tags or push unless explicitly requested.

## Why this document exists

The user asked for a fresh scan of earlier aw prompts to verify that requested work was either implemented or explicitly identified as future work, then asked aw to adopt the same reproducible cross-build and GitHub release architecture proven in ../dotlink.

This document is the traceability checkpoint for that request. It does not turn intentionally deferred roadmap items into current-phase obligations.

## Earlier-prompt audit

### Current Phase-37 review requests — implemented

The current tree covers the concrete review requests from the October 1–2 design passes:

- Pi-like regular/scrollback mode with Ratatui composer, zero-padding defaults, terminal-owned scrollback and no private provider reasoning.
- Codex/VS Code/Helix-like fullscreen interaction.
- Slash autocomplete is attached immediately above the composer, not at the top of the screen.
- Fullscreen pointer support covers focus, tabs, navigator rows, Settings, slash choices, palette choices and split dragging.
- Default fullscreen geometry is 22.5% Explorer / 77.5% main.
- Explorer order is Sessions / Files / Git.
- Main conversation label is Chat; filename/Diff tab labels are bounded and filename-aware.
- Files and Sessions have Tree/List projection.
- Session tree nests durable branches/forks beneath each Session.
- Git exposes Changes / Worktrees / Branches, Tree/List changes and independently foldable change groups.
- Worktree selection affects only human Git inspection context.
- Workspace focus colours use restrained Abird green/blue language.
- Alt+E focuses Explorer; Alt+W focuses main content; Alt+1..9 is contextual to the focused section and number hints render only there.
- Ctrl+Shift+P works both with metadata-rich terminals and terminals that collapse it to the Ctrl+P control byte.
- F6/F7 remain presentation-only thinking/tool-detail toggles; provider-private reasoning is never exposed.
- Local single-user web starts with zero models and requires no pairing.
- Browser root is SolidJS/Vite with the official Round Trip mark, Abird design tokens, a compact Sessions/Files/Git Explorer, Chat/content surface and floating composer.
- Guided providers are ChatGPT subscription, Ollama Cloud, custom Chat Completions and custom Responses API; Local Ollama remains an explicit built-in/config target.
- ChatGPT subscription is a dedicated Responses + supported OAuth path, never a Chat Completions token alias.
- Ordinary provider configuration save remains provider-network-free.
- Domain -> Workspace -> Session, durable no-replay, SecretStore custody, exact OIDC identity, context authority and Phase-35 direct-mode invariants remain intact.

### Inline/onboarding/UI-extension refinement — implemented

The latest Pi-style inline and extensibility requests are implemented in the active tree:

- normal/default Cargo builds select `full`, including OIDC; `--no-default-features` remains the deliberately tiny stateless agent and is used by the current Windows release boundary;
- first-run **Skip for now** enters the ordinary inline composer rather than terminating;
- the first attempted Send while no model is configured opens the same Provider & model flow and preserves the unsent draft;
- guided setup may explicitly perform bounded model-catalog discovery after endpoint/auth-reference input; saving profile configuration itself still performs no provider/inference request;
- the actual-binary onboarding smoke proves both explicit discovery/save separation and deferred Skip/draft preservation;
- inline mode uses Pi-style two horizontal rules with a stable per-Session accent chosen from a bounded palette;
- Settings/keymap/help/provider sub-views rendered inside those same rules use one reserved orange that cannot be selected as a composer accent;
- rendered-cell tests verify the actual top/bottom rule colors for both normal composer and Settings, not just the palette helper.

UI presentation is now extension-owned through `aw-ui-extension`:

- `UiExtension` is the compiled-in Rust implementation trait today;
- serde-typed `Manifest`, `Request`, `Response`, `Context`, `Contribution`, `Line`, `Span` and `Slot` form the transport-neutral contract;
- `ExtensionHost` is the registry-facing request/response seam;
- `InProcessExtensionHost` adapts built-in extensions today;
- `Registry::from_hosts(...)` and a protocol-only-host test prove a future external extension-host process can proxy the same contract without changing renderers;
- UI extensions receive bounded immutable presentation context and return validated semantic lines; they do not gain provider/tool/filesystem/process/network/model-context authority.

The built-in `StatusLineExtension` owns arbitrary ordered, bounded status rows across `above-composer`, `below-composer` and `fullscreen-status`. Three compact below-composer rows are the current Pi-like default. The canonical `ui.status_lines` model supports up to 32 rows / 16 items per row, while the legacy flat `status_line` form is read-only compatibility.

The active Solid Settings UI now edits that canonical model directly: add/remove/reorder rows, choose placement, toggle row items, and configure TPS/performance, TTFT, waterfall, thinking/tool visibility, TUI mode, padding and autocomplete. It does not depend on the historical `web/settings.js` compatibility surface.

`PerformanceExtension` completely owns human-facing TPS/token-count/TTFT/elapsed/waterfall rendering in `after-turn`. Core runtime keeps only raw measurements; there is no second hardwired TPS renderer.

The future external extension-host process itself is intentionally not implemented yet. Process lifecycle/framing/resource limits/sandboxing belong to that future host adapter, not to the UI registry or renderers.

### Latest web UI repair — implemented

The follow-up screenshot review is now reflected in the active Solid root rather than only in design notes:

- the top chrome is a real search/command bar followed by a document tab strip;
- search covers commands, open tabs, Sessions and bounded server-side file search;
- the Explorer/main boundary is pointer-draggable and persists its local width;
- the compact New button uses the restrained Abird green treatment rather than the broken saturated state;
- Files is a compact searchable worktree-style navigator with lazy folders, noise suppression (.git, target, node_modules, etc.) and Tree/List projection;
- Git uses a compact segmented Changes / Worktrees / Branches control, right-side Tree/List buttons, independent foldable change groups and a real nested path tree in Tree mode;
- file/diff tabs remain bounded and filename-aware;
- file views use read-only CodeMirror;
- diff views use CodeMirror MergeView;
- syntax token decoration is produced by Shiki themes/languages;
- CodeMirror/Shiki is lazy-loaded as chunk-code-surface.js, so the main chat/workspace shell stays small and snappy;
- the Rust embedded-asset router explicitly serves the lazy editor chunk.

Production Vite build, npm audit, 98 browser-model tests, actual Rust web smoke and embedded-asset smoke are green. The connected host cannot complete a rendered Chrome screenshot because its Chrome runtime lacks usable host fontconfig/machine-id support, so no screenshot-render claim is made from this connector.

### Store/session/minimal-runtime requests — implemented

The later storage/minimal-agent requests are now explicit architecture rather than SQLite-shaped application code:

- aw-store-api owns backend-neutral Session types, SessionBackend / SessionCursor, StoreDescriptor and declared StoreCapability values;
- SQLite implements the full capability set and remains the rich authority/coordination backend;
- aw-store-jsonl is a real single-owner append-only backend, not just an exporter;
- JSONL writes admission/effect intent before external actions, fsyncs records, and restart-gates interrupted operations for inspection instead of replay;
- JSONL forks become separate Session files with parent provenance, Pi-style;
- ordinary inline chat supports --store-backend jsonl, lazy creation, resume and file-per-fork sessions;
- aw session capabilities|list|show|tree|fork|import|export --store-backend sqlite|jsonl goes through the common backend API;
- aw-session-format owns portable aw JSONL plus conservative Pi v2/v3 JSONL codecs;
- Pi import never promotes Pi system/tool/extension text into aw System/Tool authority; lossy/flattened mappings report warnings;
- portable aw/Pi export and SQLite↔JSONL migration are explicit data operations, never provider/tool replay;
- Postgres/LevelDB are represented only as future backend kinds/capability targets; no fake implementation is claimed.

Actual-binary smokes are green for:

- JSONL first turn -> process restart -> exact resume context;
- JSONL fork -> new file;
- Pi -> JSONL;
- JSONL -> portable aw;
- JSONL -> Pi;
- portable aw -> SQLite;
- imported Pi system text confirmed not promoted to aw authority;
- Unix JSONL directory/file permissions verified 0700 / 0600.

Normal Cargo default is now `full`, including OIDC and the complete reviewed product surface. The genuinely small, database-free single-user agent remains available intentionally through `--no-default-features`: provider/profile/model config + inline Ratatui chat + optional basic local tools, without SQLite/web/workspace/multi-user/distributed layers.

Feature ownership is explicit: durable/sqlite, jsonl, session-interop, web, surfaces, workspace, coordination, multiuser, multiplayer, team, net, device, client, oidc, and aggregate full.

### Platform abstraction request — implemented

Platform-specific production primitives are now owned by:

- aw-platform-unix;
- aw-platform-win;
- target-selecting facade aw-platform.

Higher layers use aw_platform::* instead of embedding raw OS calls. The source validator now fails if production Rust outside the platform backends reintroduces raw libc::, std::os::unix or std::os::windows.

The platform boundary currently owns:

- stable local principal projection (unix:<uid> / windows:<SID>);
- private-file/private-directory creation and validation;
- no-follow/reparse-aware scoped filesystem operations;
- parent directory synchronization / atomic user metadata writes;
- null-device selection;
- Git path conversion and process-tree handling;
- local IPC pathname/socket ownership on Unix;
- trusted-direct local-filesystem classification;
- process-group termination policy.

aw-git no longer embeds Unix path/process primitives. aw-terminal no longer has a direct libc/raw-fd reactor: it uses portable-pty so Unix PTY and Windows ConPTY share the same reader/resize/input lifecycle, with process termination delegated to aw-platform.

Windows secure owner-only ACL custody for authority-bearing durable state/SecretStore is still deliberately unimplemented and fail-closed. Therefore the Windows release currently ships the normal minimal Ratatui agent, not a fake full build. The platform seam is in place for later enabling richer Windows capabilities without spreading cfg(windows) through application crates.

### Earlier implementation goals — already delivered

The phase history already contains the earlier conversation -> tools -> durable state -> workspace -> Git/terminal -> web/server -> distributed authority progression. In particular, the current tree already has durable SQLite sessions/forks/checkpoints, explicit tool grants, provider/account separation, Files/Git/Sessions workspace navigation, PTY terminals, browser APIs, stable opaque users, SecretStore/OIDC, Domain/Machine/Fleet/AwNet structures, semantic synchronization/materialization, writer epochs/CAS and schema-v20 trusted-local direct mode.

Those are not missing items for a new phase.

### Explicitly future/deferred — not silently missed

Earlier v0.7 prompts/specs also described later product layers. These remain future work unless the user explicitly defines a new numbered phase:

- multiplayer unsent-draft collaboration/presence with the documented Off/Auto/On model;
- pipelines/routing/autonomy and advanced governance/cloud layers;
- broader remote-local personal inference/credential bridging beyond the current explicit custody and MachineLink contracts;
- full migration of every historical multi-group/browser-terminal/site/context-flow workbench surface into the simpler Solid root;
- broad Git mutation UX such as checkout/commit/fetch/push, hunk staging and conflict-resolution workflows;
- independent concurrent Agent ownership per browser tab;
- persistent synchronized browser/native layouts as authority-bearing shared state.

These are documented deferrals, not regressions in the current Phase-37 review checkpoint.

## Release architecture adopted from ../dotlink

aw now uses the same release principles as dotlink:

- one pinned Rust toolchain file (Rust 1.98.1, rustfmt, clippy);
- nixpkgs + Crane + rust-overlay pinned in flake.lock;
- Cargo.toml is the single release-version source;
- one aggregate Nix release-all derivation;
- five named release assets, each with SHA-256 sidecar;
- VERSION and PLATFORMS.txt generated alongside the binaries;
- checksum-verifying Unix and PowerShell installers;
- GitHub Actions build once, upload one immutable workflow artifact set, and promote that exact set on version tags;
- tag name must equal v + Cargo package version;
- GitHub Actions are pinned to immutable commit SHAs.

### Release target matrix

1. Linux x86_64
   - target x86_64-unknown-linux-musl
   - static musl
   - `--features full` release build

2. Linux ARM64
   - target aarch64-unknown-linux-musl
   - static musl
   - `--features full` release build

3. Windows x86_64
   - target x86_64-pc-windows-gnu
   - `--no-default-features` minimal Ratatui agent
   - provider/profile/model configuration, in-memory chat and optional basic local tools
   - durable stores, web, fullscreen workspace, SecretStore/OIDC, device/team and AwNet roles are compile-time omitted

4. Windows ARM64
   - target aarch64-pc-windows-gnullvm
   - LLVM-MinGW/UCRT toolchain, matching the proven dotlink architecture
   - same minimal no-default-features boundary as Windows x86_64

5. macOS ARM64 / Apple Silicon
   - target aarch64-apple-darwin
   - Linux cross-build through pinned Apple SDK 14.4 + clang/lld
   - full Cargo feature build
   - Linux-only same-UID/device/team/file-tool authorities remain fail-closed at runtime

The Windows boundary is deliberate. Secure owner-only ACL custody for durable state/SecretStore is not yet implemented, so the release uses the minimal stateless agent rather than weakening those guarantees merely to claim feature parity.

## Cargo feature boundary added for portability

The normal/default product and the intentionally tiny build are now distinct compile-time contracts:

- default = ["full"]: ordinary Cargo builds include the complete reviewed product, including OIDC;
- --no-default-features: tiny stateless single-user Ratatui agent used explicitly when the richer product is not wanted/available;
- durable/sqlite and jsonl: independent persistence backends/capabilities;
- web: HTTP/Solid chat/settings transport over the durable owner;
- surfaces: typed human Files/Git/PTY surface host;
- web-workspace: web + surfaces;
- workspace: native fullscreen Git/PTY workspace;
- coordination/multiuser/multiplayer/net/device/client/oidc: explicit additive authority layers;
- full: reviewed aggregate used by normal/default Linux/macOS builds.

Windows release artifacts intentionally compile the explicit `--no-default-features` minimal closure. This keeps SQLite/SecretStore/web/workspace/distributed authority out of the binary until the corresponding Windows invariants are implemented. Warning-denied default/full, explicit minimal, and whole-workspace checks are green on the current native host.

## GitHub workflows

.github/workflows/build-binaries.yml now:

- runs on workflow_dispatch, pull requests, main pushes and v* tags;
- checks out the exact revision without persisted credentials;
- installs Nix with flakes enabled;
- builds .#release-all;
- verifies all SHA-256 sidecars;
- verifies the native Linux x86_64 version against VERSION;
- checks all five expected files and PLATFORMS.txt;
- uploads one aw-binaries workflow artifact;
- on a v* tag, downloads that exact artifact set, verifies tag/version equality and creates/uploads the GitHub Release.

The existing validate.yml remains the explicit full validation/evidence workflow. The reference gate now includes release-architecture source validation, while the full profile includes release-all.

## Installer contract

install.sh supports Linux x86_64/ARM64 and macOS ARM64, plus MSYS/Cygwin Windows when used from a Unix-like shell. install.ps1 supports Windows x64/ARM64.

Both installers:

- select only a published asset for the detected architecture;
- download its SHA-256 sidecar;
- verify the checksum before installation;
- execute only --version before install;
- verify a specifically requested version;
- atomically replace the destination;
- never write provider credentials.

## Validation and evidence boundary

Executed against the final closeout tree:

- cargo fmt --all -- --check: green;
- cargo test --locked --workspace --all-targets: green;
- cargo test --locked --workspace --doc: green;
- warning-denied minimal and --features full checks: green;
- whole-workspace warning-denied check: green;
- pinned Rust/Cargo/Clippy 1.98.1 cargo clippy --locked --workspace --all-targets -- -D warnings: green;
- Phase/reference validation profile: 39/39 green;
- actual fullscreen PTY workspace smoke: 10/10 green;
- actual local-web/fake-provider smoke: green;
- embedded Solid/web UI smoke: green;
- JSONL ordinary-chat/resume/fork smoke: green;
- Pi/aw/JSONL/SQLite interoperability smoke: green;
- browser pure-model suite: 98/98 green;
- npm audit: 0 vulnerabilities;
- production Vite/Solid build: green;
- source architecture validator: 0 errors / 0 warnings;
- release architecture validator: 59 checks green;
- installer smoke: green;
- nix-instantiate --parse flake.nix: green;
- git diff --check: green.

The monolithic validation_gate.py --profile rust wrapper cannot be claimed as a clean single report on this connector because long invocations repeatedly get interrupted by the connector and the host's default Cargo 1.95 path lacks Clippy. Its constituent Rust steps were therefore run explicitly; pinned 1.98.1 Clippy is green.

The actual Nix cross build was attempted:

nix --extra-experimental-features 'nix-command flakes' build path:.#cross-windows-x86_64 --no-link

It fails before evaluation/build materialization because this connector mounts /nix/store read-only:

error: creating directory "/nix/store/tmp-...": Read-only file system

Therefore the five produced binaries and GitHub release execution are not claimed locally. Acceptance of those artifacts requires .#release-all on a writable Nix host or the authored GitHub workflow.

A real ChatGPT OAuth login also still requires explicit human authorization and is not implied by release validation.

## Review boundary

This remains Phase-37 closeout/review work. Do not create Phase 38 automatically. Do not move phase-37. Do not push unless the user asks.
