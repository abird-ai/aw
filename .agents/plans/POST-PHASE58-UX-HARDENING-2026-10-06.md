# Post-Phase-58 native UX / terminal hardening — 2026-10-06

**Status:** implemented and validated; ready for user review
**Numbered phase:** none. **Do not call this Phase 59.**
**Source version:** 0.15.0
**SQLite schema:** v32 (unchanged)
**Authority:** this document supersedes only the native UX/input/terminal-presentation details explicitly listed here. Phase 58 and the post-Phase-58 review remain authoritative for everything else.

## User-visible goals

This pass responds to the 2026-10-06 native UX review:

1. give the shared prompt editor normal shell/readline line editing and word movement/deletion;
2. make fullscreen Workspace spacing minimal/elegant and allow Agent vs Workspace editor/output padding to be configured independently;
3. allow Sessions deletion from the list with Delete, add a visible multi-select affordance, and preserve durable authority/history semantics;
4. make Sessions List mode a compact two-line layout with completion/running indicators;
5. fix Help/tab mouse hit geometry architecturally and make Esc actually close Help;
6. repair Scrollback/Inline resize and exit terminal artifacts/gaps while preserving the intended Scrollback vs owned-primary Inline distinction;
7. use a tiny shared three-box activity animation in the composer rule and running Session rows;
8. switch user prompts to the current Pi-like muted-blue full-width band;
9. use two compact Pi-like status rows, with truthful measured runtime-session average output TPS when available;
10. verify Pi composer geometry from current source before changing vertical padding.

## Pi source review / decisions

Current Pi source was inspected through the GitHub connector, primarily:

- `packages/tui/src/components/editor.ts`
- `packages/tui/src/keybindings.ts`
- `packages/tui/src/word-navigation.ts`
- `packages/coding-agent/src/modes/interactive/components/user-message.ts`
- `packages/coding-agent/src/modes/interactive/components/custom-editor.ts`
- `packages/coding-agent/src/modes/interactive/components/status-indicator.ts`

Findings used here:

- Pi's editor renders **top rule + visible editor lines + bottom rule**. A one-line composer is therefore three terminal rows.
- Pi's editor has horizontal `paddingX`, not an extra vertical editor padding setting. aw's existing one-row cursor/placeholder geometry already matched this, so **no extra vertical composer padding** is added.
- Pi's default editor bindings include Ctrl/Alt word movement, Ctrl+W / Alt+Backspace backward-word delete, Alt+D / Alt+Delete forward-word delete, Ctrl+U/K, Ctrl+A/E/B/F, Ctrl+D, Ctrl+Y/Alt+Y, etc.
- Pi's user message is a padded full-width themed background rather than a `>`/marker-only row.
- Pi can embed working status into the editor's top border. aw follows that structural idea but deliberately uses a smaller three-box animation rather than Pi's loader.
- Pi's status UI balances compact left/right session/model information rather than consuming large dedicated panels.

## Implementation decisions

### Shared editor semantics

`aw-editor::Editor` now owns Unicode-grapheme-safe word boundaries and line-end deletion. `tui::editor::edit_key` is the one terminal mapping seam for Agent, Workspace Agent composer and compatible single-line inputs.

Supported current chords include:

- Ctrl+A / Ctrl+E — line start/end
- Ctrl+B / Ctrl+F — one grapheme left/right
- Ctrl+Left / Alt+Left / Alt+B — word left
- Ctrl+Right / Alt+Right / Alt+F — word right
- Ctrl+W / Alt+Backspace — delete previous word
- Alt+D / Alt+Delete — delete next word
- Ctrl+U / Ctrl+K — delete to current line start/end
- Ctrl+D — delete next character when input is nonempty; outer Agent logic still handles empty Ctrl+D quit
- Ctrl+H / Backspace — delete previous character
- Shift+Enter / Alt+Enter / Ctrl+J — composer newline
- Home / End and normal arrows remain supported

The actual tmux smoke sends Ctrl+Left, Alt+Backspace and Alt+D through a real terminal and verifies the provider receives the edited prompt. This is intentionally the familiar movement/deletion/newline readline family, not a claim that `aw-editor` is a complete GNU Readline implementation: kill-ring/yank-pop, reverse-history search, transpose-word and configurable readline modes would require additional bounded editor-history state and remain separate work.

### User prompt band

Shared Agent chrome and owned Scrollback settlement now render user prompts as a full-width muted-blue band with one-cell horizontal text inset and no `›` marker. Long logical prompt lines wrap grapheme/cell-safely through that one shared renderer instead of truncating visible transcript content. Assistant prose remains unboxed. This is presentation-only and does not affect stored prompt text.

### Live activity

The old live `Thinking…` row immediately above the composer is removed. Generic live work appears inside the composer's **top rule**:

- a tiny cycling `▪▫▫` / `▫▪▫` / `▫▫▪` animation;
- `working` when width allows;
- elapsed time since prompt admission right-aligned when width allows.

Provider-private reasoning is still never displayed. F6 remains the projection control for safe settled Thinking/timing; F7 remains the typed tool-detail projection control.

The same `agent_chrome::progress_indicator` primitive drives in-progress Session-list markers.

### Status hierarchy

The exact built-in default is now two rows below the composer and mirrored as two `fullscreen-status` rows:

- row 1 left: compact cwd + `agent <branch>`
- row 1 right: tools + state
- row 2 left: measured runtime-session `avg t/s` when available
- row 2 right: model + `[profile]`

`SessionPerformance` is a non-durable typed accumulator. It sums only measured output tokens and measured generation duration for successful turns observed in the current process. It resets when the active Agent/Session changes. It does **not** invent timing for restored historical turns. No effort label is displayed because current provider/profile state has no shared typed effort field.

Custom `status_lines` remain ordered/configurable and use the ordinary extension projection. The exact historical three-below/one-fullscreen default migrates to this current default; arbitrary custom layouts are not rewritten.

### Separate Agent vs Workspace padding

`UiConfig` now has four bounded settings:

- `editor_padding_x` — standalone Agent composer
- `output_padding` — standalone Agent transcript
- `workspace_editor_padding_x` — Workspace embedded Agent composer
- `workspace_output_padding` — Workspace content/transcript

All validate to `0..=3`. Agent defaults remain 0/0; Workspace defaults are 1/1. Native Settings exposes all four. The settings API accepts the Workspace fields optionally so old callers preserve existing values.

### Sessions list / multi-select / Delete

Sessions List mode uses two display rows per item:

- first row: title + state marker;
- second row: relative age + turn count + branch.

Status markers:

- completed child Agent: small `●`;
- running child/current Agent: animated three-box indicator.

A compact multi-select icon sits beside Tree/List controls. While multi-select is enabled, clicking a Session row or Space/Enter toggles whole-Session selection. Delete operates on all selected Sessions, or the currently selected Session when the set is empty.

There is intentionally **no destructive Session-row delete** in SQLite. `SessionStore::set_session_hidden` appends owner-authorized `session.hidden` / `session.restored` audit events. Normal Session listing honors the latest visibility event. Immutable history, nodes, receipts and exact authorized Session access are preserved. This is the correct product meaning of “delete from the Sessions list” under aw's immutable-history invariant; a destructive purge would require a separate explicit lifecycle/authority design.

### Tab/hit geometry / Help Esc

`draw_group_tab_strip()` is the shared main-content tab painter. It both paints each tab and returns the exact same rectangles used for `Hit::GroupTab`. No second label-width reconstruction exists. This fixes the Help “click slightly left” bug for the whole main-tab family.

`close_help_on_escape()` is the explicit reducer seam for active Help; Esc leaves composer mode and closes active Help as advertised.

### Terminal resize / exit

Scrollback remains one persistent aw presenter with a dynamic Ratatui Inline scratch terminal. On row-count/physical resize reanchor:

1. capture/retain the old physical viewport origin;
2. clear the old physical scratch footprint with `Clear(FromCursorDown)`;
3. move to the new anchor;
4. rebuild the Inline terminal at the desired height;
5. retain sticky physical-bottom anchoring once real saturation has occurred.

This avoids stale old-size scratch artifacts and does not insert fake history.

Owned-primary Inline now publishes the first transient row after settled transcript content to the central `TerminalSession`. On restore, the centralized owner:

1. resets mouse/paste/style/cursor state;
2. moves to the published content boundary;
3. clears from there down;
4. restores canonical terminal mode.

It does **not** force the shell to the physical bottom and does not emit an extra blank newline. This removes the large terminal-height gap after short/resized Inline sessions.

## Main implementation files

- `crates/aw-editor/src/lib.rs`
- `crates/aw-providers/src/config.rs`
- `crates/aw-store/src/lib.rs`
- `crates/aw-store/tests/store.rs`
- `src/main_full.rs`
- `src/main_minimal.rs`
- `src/tui/agent_chrome.rs`
- `src/tui/commands.rs`
- `src/tui/editor.rs`
- `src/tui/extensions.rs`
- `src/tui/inline.rs`
- `src/tui/performance.rs`
- `src/tui/settings.rs`
- `src/tui/status.rs`
- `src/tui/terminal_session.rs`
- `src/tui/theme.rs`
- `src/web.rs`
- `src/workspace/owner.rs`
- `src/workspace/ui.rs`

## Regression / validation additions

Focused Rust regressions cover:

- word movement/deletion and Ctrl+K/Ctrl+D semantics;
- full-width prompt band style and wrap-without-dropping-content;
- shared three-box activity border + elapsed time;
- runtime session-average TPS aggregation;
- owner-authorized Session hide preserving exact history access;
- two-line Session rows, completion/running marker and multi-select hit;
- exact tab render/hit geometry;
- Esc closing Help;
- current two-row natural-flow geometry.

the removed post_phase58_ux_terminal_smoke helper additionally validates on a real tmux terminal:

- Ctrl+Left + Alt+Backspace + Alt+D word editing reaches the shared composer correctly;
- Inline resize twice then quit resumes the shell adjacent to aw content, with no physical-bottom blank gap;
- Scrollback repeated resizes leave exactly one live composer/status stack;
- Scrollback resize then quit leaves no stale scratch chrome after shell resumption.

## Validation status

Green on the final source during this pass:

- `cargo fmt --all -- --check`
- `cargo test --locked --workspace --all-targets`
- `cargo clippy --locked --workspace --all-targets -- -D warnings`
- full `cargo test --locked --bin aw` (213 tests)
- `cargo test -p aw-store`, `cargo test -p aw-providers`, and `cargo test -p aw-editor`
- no-default/minimal and individual feature cuts for web, surfaces, workspace, client, device, team, multiplayer, pipelines, routing, net and oidc, plus `team,multiplayer` and `web,oidc`
- `cargo test --locked --workspace --doc`
- `RUSTDOCFLAGS='-D warnings' cargo doc --locked --workspace --no-deps`
- `cargo check --locked --release -p aw-cli --features full`
- SDK dependency-boundary checks
- Phase 54 / 55 / 56 / 57 / 58 static checks after intentional contract updates
- post-Phase-58 review checker plus the new 88-check `test_post_phase58_ux_hardening.py`
- full `validation_gate.py run --profile reference`
- compiled full-feature debug `cli_smoke.py` and `workspace_smoke.py`
- compiled full-feature debug `tui_inline_smoke.py` and `tui_live_smoke.py`
- Phase-55 actual PTY lifecycle smoke
- Phase-57 actual tmux natural-flow smoke
- Phase-58 actual tmux saturation smoke
- new post-Phase-58 UX terminal smoke, including real Ctrl/Alt word editing and repeated resize/exit cleanup
- release installer packaging smoke against a compiled 0.15.0 artifact

One host/tool limitation is deliberately **not** counted as green: the connector repeatedly terminates the long optimized full-feature `cargo build --release` link before a new binary replaces the repository's old reduced-feature release artifact. The stale artifact was detected because it lacks `--tui-mode` and was not used for PTY validation. Release-profile compilation itself is green, and all actual UI/terminal execution proofs above use the freshly rebuilt full-feature debug binary.

## Non-goals

- no Phase 59;
- no schema v33;
- no historical phase-tag rewrite;
- no destructive Session-history purge;
- no invented provider “effort” field;
- no terminal vertical padding added merely to imitate screenshot spacing;
- no new model/tool/account authority;
- no push.
