Author: GPT-6

# Native and browser UX convergence

## Verified source completion — 18 October 2026

Signed implementation: `2bd67498849d04e18c00551396ccc8fcc08dfcd4`.
Scope: shipping **visual and interaction UX**, without new Runtime/Actor,
ToolPolicy, provider, PTY or Store authority. The separately owned
`.agents/plans/ux/` remained untouched.

- **Native palette and status** — one semantic Light/Dark/System role map
  across Agent, Git/Explorer/tabs/status extensions. Explicit palettes
  degrade to 16/256 colors and preserve truecolor where available;
  System defers to the terminal's default foreground/background.
  Factual tool status words survive any color depth and contradictory
  error results cannot be presented as success.
- **Less noise, same capabilities** — embedded native and browser Agents
  omit empty Context/Skills sections but retain actual loaded resource
  provenance. Single-row active tabs, compact tool blocks, unboxed
  assistant messages, quiet Explorer labels, real pane boundaries and
  existing keyboard commands remain; no presentation action grants
  a new Files, Git, Agent, Terminal or execution resource.
- **Keyboard and accessibility** — browser Settings traps Tab/Shift+Tab,
  supports Escape through the original safe close/revocation path and
  returns focus to its invoker. Existing Command Palette and tab
  navigation preserve their keyboard ownership and draft/caret.
  Light/Dark tested metadata, foreground and success/danger contrast
  achieved at least **4.5:1** on page/Explorer surfaces. This is a
  targeted contrast audit, *not* a full WCAG conformance certificate.
- **Native acceptance** — **135 shipping Ratatui TestBackend frames**
  (9 scenes, 3 themes, 5 widths/heights): Agent, Activity, Settings,
  Session Tree, File, Palette, overflowing tabs, split and unavailable
  terminal. Real isolated PTY in 16-color mode verified 80×24→65×20→120×30,
  global F1, maximize/restore, `quit-discard` exit 0, byte-equal termios
  and alternate-screen release. Another live synthetic child-shell PTY
  verified Ctrl+C stays with child shell, F1 remains reachable and
  `quit-all` releases child and restores terminal state.
- **Actual Chrome 155** — a private synthetic LocalWeb with desktop
  1440×900, mobile 390×844 and narrow 320×568, both Light/Dark.
  Sixteen verified visual/interaction scenarios include Sessions,
  actual File/Git Diff, one context title, Settings modal focus and
  390px geometry, Command Palette cancellation, Home/End document
  tabs, responsive layout and Unicode draft/caret preservation.
  Eight screenshots remained private in temporary fixtures. No
  horizontal document overflow, uncaught JS errors, Git HEAD/index
  modifications, Session runs or tool/effect receipts.
- **Product gates** — **1,309 non-Store Rust tests** (4 ignored),
  **201 Store library tests**, **156 Store integrations**, **174 Node
  tests**, **139 web-only Rust tests**, strict library Clippy,
  formatting, workspace/minimal feature builds, `aw 0.15.0` binary,
  and **8/8 byte-identical built frontend assets**, all verified on
  the signed implementation. Cargo coverage passed via split packages;
  a monolithic `cargo test --workspace --locked` was attempted but
  **not completed** within the connector's 112–115 second cap.
  Binary-wide strict Clippy still has six existing non-UX warnings.

**Remaining outside this scope:** native *second-terminal* docking
(transfer of exclusive PTY Runtime control), different-user delegated
access, adversarial post-ACK crash/partition certification, actual
Windows/macOS runtime certification and release/deployment. The
[Workspace views](../workspace-views/TODO.md) and
[Release readiness](../release-readiness/TODO.md) remain open.
Completing visual convergence does **not** complete those milestones.

## Historical shipping UX inventory — 17 October 2026

The paragraphs below record the **pre-convergence baseline** and
its earlier evidence; their "remaining work" statements are
historical and superseded, *within the present scope*, by the signed
completion above.

This is a source-backed comparison of the **shipping** native Workspace and
Solid browser shell, not a new product, a claim of browser parity or authority
to change execution. The separately owned `.agents/plans/ux/` is outside this
inventory. User data, provider/private traces and restored State are excluded.

## Existing, verified surfaces

| Topic | Native owner | Browser owner | Verification and boundary |
| --- | --- | --- | --- |
| Semantic colors | `src/tui/theme.rs` | `web/src/styles.css` | Native System keeps terminal base colors with `Reset`, Light/Dark have distinct roles. Browser has light/dark variables and focus-visible styling. This is source inspection, **not** an actual 16-color or browser contrast audit. |
| User/tool semantics | `src/tui/theme.rs`, `src/tui/markdown.rs` | `web/src/main.jsx`, `web/src/styles.css` | User prompt owns a bounded semantic background; tool success/failure/neutral are distinct. UNKNOWN and interrupted states need factual words as well as color. |
| Tabs/overflow | `src/workspace/tab_strip.rs` | `web/src/styles.css` `.document-tabs`, `.doc-tab` | Native uses one selected tab row, actual hit rectangles, keyboard-reachable hidden neighbors, grapheme clipping; browser has one-row retained document tabs. |
| Palette | `src/workspace/ui/interaction.rs`, `src/tui/palette.rs` | `web/src/main.jsx` | Native retains one parsed category/identity and has explicit empty-query no-op; F1 and Ctrl+Shift+P use existing command semantics. |
| Read-only Agent / scoped Files & Git | `src/workspace/ui/{agent_views,files_views,git_views}.rs` | `web/src/main.jsx` and scoped client modules | Existing independent IDs, exact binding, late-reply rejection and execution isolation must remain; do not rename or visually hide resource identity. |
| Native layout and visual proof | `src/workspace/ui/render.rs` and `src/workspace/ui/ux_visual_tests.rs` | `web/src/styles.css` | Executed `ux_v15_native_gallery_preserves_identity_and_resize_state` in the pinned Nix dev shell on starting HEAD `a20a4bd`: **90 TestBackend frames passed** across Dark/Light/System, six scenes and 80×24 through 200×60. The fixtures intentionally do **not** attach a real terminal child or effect owner. |

## Concrete remaining UX work — ordered, not silently accepted

1. **Status/theme roles:** `src/tui/extensions.rs` maps status `Tone` directly to
   fixed ANSI `Color` values, while Agent, Workspace and browser surface
   palettes use semantic tokens. Before changing roles, compare factual
   status text/focus/readability across real 16/256/truecolor terminal
   capabilities and light/dark browser screenshots. Retain all metadata.
2. **Small screens and hierarchy:** In the actual shipping 80×24 native
   TestBackend Agent fixture, the secondary Context/Skills headings show
   `none loaded` twice above the transcript. Determine whether the
   optional empty sections can fold at narrow widths **without** hiding
   loaded instructions, grants, context or provenance. The command palette
   shows dense/truncated long entries at 80×24; preserve keyboard search
   and command descriptions even if the visible summary is shortened.
3. **Chrome discipline:** `src/workspace/ui/render.rs` already uses a
   borderless main surface but sidecars/modal use borders in some states.
   Separate functional split/resize/focus boundaries from nested decorative
   frames. Keep exactly one context-bearing title and all terminal/cancel
   indicators. Compare live desktop/mobile browser views before editing.
4. **Interaction matrix:** Exercise modal cancellation and preserved draft/caret,
   narrow-width focus and split/maximize, Unicode clipboard/selection, file
   reconnect and Git stale-denial states, PTY Ctrl+C ownership/quit, repeated
   resize and real shell restoration. A 90-frame TestBackend gallery does
   not close the real child-PTY/native terminal acceptance item.
5. **Cross-surface convergence:** Check Agent/Session/Queue/Activity/Settings,
   tool UNKNOWN/denied/cancelled labels and tab close/overflow at Light,
   Dark and System. Do **not** weaken recovery/consent/revocation or replace
   actual status text with color-only badges/icons.

## Executed evidence and limits

- Starting worktree: `main` at `a20a4bdca52b4ac4c6503a4bbe30898d233c7a1b`,
  non-reserved index/worktree clean before these owned edits.
- Native 90-frame gate:
  `AW_UX_NATIVE_OUTPUT=PRIVATE nix --extra-experimental-features 'nix-command flakes' develop --no-update-lock-file -c cargo test --locked -p aw-cli --bin aw ux_v15_native_gallery_preserves_identity_and_resize_state -- --nocapture`.
  **One targeted test passed; 361 other binary tests filtered.**
- Live PTY with an isolated fake-provider profile and private SQLite State
  demonstrated Workspace startup, palette, 80×24 → 120×30 terminal resize,
  clean quit through explicit `quit-discard`, restored `termios`, and
  alternate-screen release on the prebuilt 0.15.0 immutable binary. A second
  smoke reached 65×20, Ctrl+K Z maximize/restore (both redrew, Sessions
  header repainted), F1 modal close/reopen and 120×30, exited 0 with equal
  terminal settings and six zero State counters. A third smoke exercised an
  actual child shell, shell-owned Ctrl+C and explicit quit-all, also exiting
  0 with terminal restoration and zero State counters. This has **not**
  exercised a model provider, Windows or production user data. A first
  attempt using Ctrl+C alone did **not** quit because
  Workspace reserves that chord for Agent cancel/draft clearing; it restored
  modes after catchable fixture termination. This is an expected behavior,
  not evidence of successful graceful exit.
- Real Chrome 155 shipping Solid UI captures used its DevTools Protocol on
  an isolated fake-provider LocalWeb owner. The four explicit viewports were
  1440×900 Light/Dark and 390×844 Light/Dark. Each `data-effective-theme`
  matched the requested scheme; the shell and composer were present, document
  and viewport widths were equal (no horizontal overflow), the composer was
  visible and zero uncaught JavaScript exceptions were observed. These are
  transient private screenshots, not shipped proof artifacts. The initial
  390 px Agent header/resource labels/composer remain coherent. This is **not**
  actual Files/Git/diff/modal/denial-browser parity or a WCAG contrast audit.
- The earlier Chrome 155 Files/Git/browser owner acceptance in
  `../workspace-views/TODO.md` remains separately scoped. Muted labels and
  resource details still require a targeted accessibility/keyboard audit.
- The subsequent bounded same-window docking substrate passed actual Chrome
  155 at **390×844 Dark** with two Files views: the move selector fit the
  viewport, the exact same CodeMirror editor and draft/caret survived moving,
  no new workspace read was issued, closing the source kept the moved tab,
  and the private State had zero run/effect counters. Cross-window handoff,
  full File/Diff contrast testing and native second-terminal ownership are
  not implied. See [Workspace views](../workspace-views/README.md).

Keep visual/design work downstream of stable resource identity and current
ownership. The next change should fix a *measured* user-facing gap with a narrow
before/after native or real-browser acceptance, not mechanically restyle every
pane.
