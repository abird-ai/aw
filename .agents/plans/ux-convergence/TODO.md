# Native and browser UX convergence

Status: **Done within the visual/interaction scope**; resource-authority features remain in the Workspace views plan.
Source implementation: `2bd67498849d04e18c00551396ccc8fcc08dfcd4`.
Accepted/provenance checkpoint: `47ee8a03abecbb3aee84a49c0a590096d76ed1a7`.

Continue only authorized bounded slices after current validation. This is additive visual/interaction work, not permission to remove capabilities, implement another Agent or touch the separately owned UX subtree.

## Completed

- [x] Shipping browser/native shell and palette/focus slices use existing state/execution owners; strict browser CSP and accessible file/editor semantics remain.
- [x] Actual palette grammar is `>` Actions, `!` Windows, `@` Files, `#` Sessions, `$` Terminals, `/` Commands and `&` Agents. Empty-query Enter is unarmed; selected identity survives async insertion and invalidation stays inert.
- [x] F1/Ctrl+Shift+P, existing browser Ctrl/Cmd+K, exact draft/caret cancel, busy admission and original child-PTY/history key ownership are preserved.
- [x] Source-backed shipping native/browser UX **inventory** recorded in [README.md](README.md): 90 real Ratatui TestBackend frames (Dark/Light/System), isolated native child-shell/resize/maximize restoration checks, and four actual Chrome 155 initial-Agent screenshots (1440×900 and 390×844, Light/Dark) with no document overflow, missing composer or uncaught runtime exceptions. Exact current browser Files/Git/modal and contrast/keyboard parity remain **separate open checks**, not inferred from initial-shell screenshots.

## Committed

### `2bd67498849d04e18c00551396ccc8fcc08dfcd4` — verified native/browser semantic and interaction convergence

- [x] Reuse the existing semantic color contract across native Agent extensions, Workspace Git/Explorer, tabs, status and browser. Explicit Light/Dark themes have bounded 16/256/truecolor terminal fallbacks; System retains terminal-owned base colors. Strengthen metadata contrast in both browser themes, including the Explorer background. No CSS-only color is used as the sole status signal.
- [x] Preserve recognizable muted-blue user prompts, unboxed assistant prose and compact read-only tool outcomes. Native and browser both use factual status text: succeeded, failed, denied, interrupted, running and UNKNOWN remain distinguishable without relying on hue. A contradictory error output cannot be marked succeeded.
- [x] Remove empty Context/Skills noise from embedded native/browser Agent views without hiding admitted instructions, context provenance or loaded skills. Keep the full native header informative and preserve a single context-bearing title and existing command/Agent capability metadata.
- [x] Use existing one-row quiet/active tabs, composable Files/Git and single context bar, functional split separators, current palette and Session/Activity/Settings ownership. Native Explorer labels and Git changes use the same palette. Do not create decorative nested panels, new buttons for their own sake, substitute a new command palette, or move execution authority.
- [x] Make Settings an actual keyboard-owned browser modal: focus moves inside, Tab/Shift+Tab wrap, Escape requests existing safe close, and focus returns to the invoking Settings control. Do not bypass remote revocation checks when closing. Retain Command Palette cancellation/focus restoration, document-tab roving Home/End, original composer Unicode text/caret, Files/Git scoped read-only content and exact focus ownership.
- [x] Increase the actual native `Ui::draw` TestBackend gallery from **90 to 135 frames** (9 scenes × 3 themes × 5 sizes), covering Agent, Activity, Settings, Session Tree, File, Palette, tab overflow, split and stale terminal. Bounded Unicode/grapheme/title, single header, terminal viewport and hit-testing regressions are exercised by Rust tests.
- [x] Verify real native Workspace PTY in 16-color mode with F1 palette, 80×24 → 65×20 → 120×30 repeated resize, maximize/restore, explicit `quit-discard`, zero exit and byte-identical termios/alternate-screen restoration. A separate exact-source real child PTY check launched a shell, preserved the Workspace on shell-owned Ctrl+C, reopened F1 and exited through `quit-all` with identical restored terminal modes and no execution effects. No paid provider or user shell was used.
- [x] Verify real Chrome 155 in a private LocalWeb fixture: Agent Light/Dark at 1440×900, 390×844 and 320×568, scoped Sessions/File/Git Diff navigation, one context bar, read-only CodeMirror, Settings focus trap/Escape/restore at desktop and mobile, Command Palette open/Escape, tab Home/End keyboard navigation, no horizontal overflow, preserved Unicode draft/caret, and zero uncaught JS errors. Eight private screenshots were generated but not shipped. Metadata, foreground, success and danger contrast measured **at least 4.5:1** on the tested page/Explorer surfaces. Synthetic Git HEAD/index and six run/effect counters stayed unchanged.
- [x] All work is presentation-only: native Runtime/PTY control, Actor/Session/Store, ToolPolicy, scoped Sources, Git write/custody, accepted File/Review docking and binary model admission were preserved. No SQLite/JSONL/native layout migration, file-root widening, provider call, or deployment.

## Uncommitted

No pending work *within this UX visual and interaction plan*. Native second-terminal/PTY docking, separate Actor delegation, broader post-ACK crash/partition certification, Windows-native runtime and release readiness are tracked by their own plans—not silently marked done here.

## Tests and verification

- [x] `cargo fmt --all -- --check` and `cargo clippy --locked -p aw-cli --lib -- -D warnings` — passed on `2bd67498849d04e18c00551396ccc8fcc08dfcd4` in the pinned Nix shell.
- [x] `cargo test --workspace --locked --exclude aw-store -- --test-threads=4` — **1,309 passed, 4 ignored, zero failures** on the signed implementation.
- [x] `cargo test --locked -p aw-store --lib -- --test-threads=4` — **201 passed** on the signed implementation.
- [x] Five `aw-store` integration test targets (`composite_design`, `context_flow`, `custody_inventory`, `resources`, `store`) — **7+29+9+23+88 = 156 passed** on the signed implementation.
- [x] Twelve browser Node suites including `tests/web_ux_convergence.test.mjs` — **174 passed**, zero failures, on the signed implementation.
- [x] `cargo test --locked -p aw-cli --no-default-features --features web --lib` — **139 passed** on the signed implementation.
- [x] `cargo check --locked -p aw-cli --bin aw --no-default-features --features workspace` and `... --features minimal` — passed; binary `cargo build --locked -p aw-cli --bin aw` produced `aw 0.15.0`.
- [x] `npm --prefix web run build -- --outDir PRIVATE_TMP` — all **8 tracked `web/dist` assets rebuilt byte-identically** from the signed source.
- [x] `cargo test --locked -p aw-cli --bin aw ux_v15_native_gallery_preserves_identity_and_resize_state` — **135 actual TestBackend frames**, 9 scenes × 3 themes × 5 sizes, on signed `2bd6749`.
- [x] Focused native status/fallback tests: `native_tool_blocks_include_factual_status_words_in_all_color_modes`, `explicit_light_dark_and_system_have_bounded_16_256_truecolor_fallback`, and `semantic_status_extensions_use_native_theme_roles_not_fixed_bright_ansi` — passed.
- [x] Real Chrome 155 private synthetic owner `chrome_driver.py` — 16 acceptance checks, 8 private screenshots, no horizontal overflow, modal focus/trap/Escape/restore, 4.5:1 tested contrasts, zero browser exceptions and zero run/effect counters on signed `2bd6749`.
- [x] Real native PTY `native_pty.py` and separate child PTY `native_child_pty.py` — palette/resize/maximize/quit and shell Ctrl+C ownership/`quit-all`/termios restoration; both passed on the signed implementation. No execution effects.

## Deferred and external gates

- [!] A single uninterrupted `cargo test --workspace --locked` was **attempted twice** on this exact source, but the remote connector terminated each run at **112–115 seconds**. This is *not* a claimed monolithic pass or a product test failure. Every non-Store, Store-library and Store-integration target passed in split commands, with their exact counts above; uninterrupted CI/release verification remains in release readiness.
- [!] Strict `cargo clippy --locked -p aw-cli --bin aw -- -D warnings` still reports **six pre-existing warnings** in non-UX Workspace/Git code; strict library Clippy and all feature checks passed. Binary-wide cleanup/certification remains a separate maintenance/release gate.
- [ ] Actual Windows/macOS terminal/color and remote-browser device certification — requires those platforms. Nix/Linux and private Chrome/PTY tests cannot substitute.
- [ ] Native second-terminal/PTY cross-window lease/control docking and separately delegated Actor privileges — outside presentation UX; see [Workspace views](../workspace-views/TODO.md).
