# Phase 53 — shared native fenced-code syntax highlighting

Status: authorized 2026-10-06 by the user after Phase 52 review.

## Boundary

Phase 53 is **native presentation only**. SQLite remains **v32**.

It adds no provider/account/tool/routing/subagent/planner/guardian authority, no model-context mutation, no filesystem/network/process effects, no terminal hyperlink activation, and no browser redesign.

## Goal

Fenced code blocks in settled assistant Markdown should gain restrained, theme-aware syntax foregrounds in ordinary/default native builds while preserving the Phase-52 neutral code-strip background and all safety boundaries.

The implementation must reuse the existing native Syntect catalog used by file/diff views rather than creating a second parser/theme registry.

## Shared syntax service

Factor the pure syntax/theme/highlight core out of the workspace-only file-view wrapper into a shared native TUI syntax module.

The shared service owns:

- one lazy immutable Syntect `SyntaxSet`;
- the existing embedded TOML and Nix grammars;
- one lazy immutable Syntect `ThemeSet`;
- effective System/Light/Dark syntax-theme selection;
- Syntect → Ratatui foreground/font-style conversion;
- syntax detection by file path/content for existing file/diff views;
- syntax detection by fenced-code language token for Markdown.

Workspace file/diff rendering remains behaviorally unchanged and calls the shared service.

## Feature boundary

Syntect remains behind the existing `workspace` feature.

- default/full builds: fenced code can use Syntect;
- no-workspace/minimal builds: Markdown remains available but fenced code falls back to the Phase-52 neutral code strip;
- Phase 53 must not make Syntect a mandatory dependency of the tiny stateless build.

## Language token handling

Fence language is bounded and treated as display metadata only.

Recognize useful aliases without network/catalog lookup, including at least:

- `rs` / `rust`;
- `js` / `javascript`;
- `ts` / `typescript`;
- `py` / `python`;
- `sh` / `bash` / `shell`;
- `json`;
- `toml`;
- `nix`;
- `yaml` / `yml`;
- `md` / `markdown`.

Then fall back to existing Syntect name/extension lookup where safe.

Unknown, empty, malformed, or oversized language tokens use neutral plain code rendering.

The token never chooses a file, command, executable, provider, plugin, or external grammar.

## Code-block rendering

- syntax foreground/font modifiers come from the selected Syntect theme;
- every code span receives the existing Markdown neutral code background;
- language label remains restrained/muted;
- code rows remain full-width neutral strips;
- Unicode/tab/width bounds from Phase 52 remain authoritative;
- highlighting failure falls back to neutral code text rather than dropping content;
- no ANSI/OSC bytes are emitted;
- no filesystem sniffing is performed for fenced code.

## Live/settled boundary

Only **settled assistant fenced code** is highlighted.

Live partial streaming remains plain/sanitized exactly as Phase 52 defined.

Tools, prompts, Thinking/status/performance, files/diffs/terminals remain outside assistant Markdown semantics; file/diff syntax highlighting keeps its existing human-preview path.

## Safety and provenance

Syntax highlighting is pure projection from already-authorized display text plus a bounded language token.

It must never:

- read source files;
- inspect the repository;
- download grammars/themes;
- execute commands;
- open URLs;
- infer model/tool authority;
- mutate Session content;
- become model context.

## Validation

At minimum:

- one shared syntax catalog/theme registry is used by workspace files/diffs and Markdown fences;
- Rust and JSON fenced-code fixtures produce multiple syntax spans in default/full builds;
- explicit Light/Dark produce distinct syntax foregrounds;
- neutral code background remains applied to every highlighted span;
- unknown language falls back without losing text;
- malformed/oversized language token falls back safely;
- no-workspace build compiles warning-free and uses neutral fallback;
- file/TOML/Nix/JSON-lock and diff highlighting regressions remain green;
- Markdown width/control/HTML/link safety tests remain green;
- live partial Markdown remains plain;
- Phase-52/51/50 checkers remain green;
- schema remains v32;
- whole workspace/all targets, Clippy `-D warnings`, doctests/rustdoc, feature cuts and actual native PTY smokes remain green.

## Deferred

- semantic code actions from fences;
- copy/run buttons;
- OSC-8 links;
- downloading additional grammars;
- user-installed syntax definitions;
- code-fence filename/path interpretation;
- browser code-block convergence;
- full CommonMark/tables/images/footnotes;
- incremental partial-token highlighting.
