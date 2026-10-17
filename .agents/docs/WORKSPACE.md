# Native Agent and Workspace — current through Phase 68 + validated prior hardening / 0.15.0

**Current native TUI note:** Agent scrollback/inline/fullscreen and the separate Workspace application share Ratatui/Crossterm interaction primitives and one slash-command registry. Post-Phase59 hardening makes those presentation choices values of one `/tui` command rather than parallel slash verbs. Phase 54 separates physical Agent presentation from the Workspace application; the older Phase-5 service/ownership text below remains useful provenance, and the provisional `Ctrl+]` workspace leader is retired in favor of familiar VS Code/Helix-style bindings.

## Current Phase-54 application and Agent-presentation contract

There are two native **application modes**:

- **Agent** is the default (`aw`, `aw --mode=agent`). It is the ordinary single-Agent product and owns one persistent presenter for prompt editing, slash completion, streaming, settlement, subsequent prompts, and resize.
- **Workspace** is the richer multi-pane product (`aw --mode=workspace`, `aw workspace`, `aw w`, `/tui workspace`, or the `/workspace` shortcut from Agent). It owns Explorer / Agent / Activity / Files / Git / Diff / retained terminals and the workspace command palette. It is no longer another spelling for Agent fullscreen.

Agent has three independent physical **TUI modes** selected by `--tui-mode=scrollback|inline|fullscreen`, Settings, or `/tui scrollback|inline|fullscreen`:

- **scrollback** is the append-oriented Pi-like option. One persistent Ratatui inline viewport remains reserved for the entire Agent lifetime; settled prompt/answer rows are inserted above it once into terminal-owned history. Transient slash/settings/live rows redraw only that same viewport. A physical terminal resize may recompute the persistent inline viewport, but ordinary popup/turn changes never create a second terminal anchor.
- **inline** is the default. On terminals that support cursor-position reporting, aw owns one dynamically sized Ratatui `Viewport::Inline` on the primary screen: short content grows naturally, loaded historical turns may leave the active viewport without being re-emitted, and only newly settled runtime turns that must be evicted after real physical saturation are inserted into ordinary terminal scrollback. Pending/live/composer/status rows are never committed as settled history. Resize/popup geometry truthfully re-anchors the inline viewport without entering the alternate screen. CPR-less/bare PTYs use an explicit bounded fixed-bottom compatibility fallback; that degraded path never pretends a clipped turn was committed to terminal history.
- **fullscreen** uses the same Agent renderer/state as inline on the **alternate** screen. It is not Workspace: no Explorer, Files/Git panes, Activity pane or workspace layout is created merely because Agent fullscreen is selected.

Agent presentation switches are presentation-only and preserve the same Host/Agent/Session/draft/provider/account/tool grants: `/tui scrollback`, `/tui inline`, and `/tui fullscreen`. `/tui workspace` hands the same live Host/Session into the distinct fullscreen Workspace application; `/workspace` is its direct shortcut. Workspace exposes `/minimal` as the single return-to-Agent action. That return preserves the configured Agent `tui_mode`; Workspace does not offer `/agent`, `/inline`, or `/scrollback` return aliases and never rewrites the Agent presentation just to leave Workspace.

The current visual language remains Pi/Codex-inspired and low-chrome. Post-Phase59 hardening keeps user prompts as full-width muted-blue bands with no `›` marker and gives prompt/tool semantic bands one filled blank row above and below their content for Pi-like vertical breathing room. Horizontal content inset comes only from `output_padding` / `workspace_output_padding`; zero means zero and no renderer manufactures a hidden one-cell inset. Assistant prose remains unboxed, tool strips remain semantic/muted, slash-menu selection remains compact, and Agent physical ownership/lifetime remains Phase-54-derived.

Presentation settings are shared and persisted in `[ui]`: **theme** (`system`/`auto` by default, or explicit `light` / `dark`), TUI mode, standalone-Agent editor/output padding, separate Workspace editor/output padding, autocomplete maximum, typed `status_lines` with independent left/right item groups and explicit `above-composer` / `below-composer` / `fullscreen-status` placement, performance-extension enablement, optional TTFT/waterfall, visible thinking activity, and tool-call detail. Native `system` mode leaves terminal foreground/background ownership with `Color::Reset`; semantic surfaces use terminal/ANSI colors rather than guessed fixed RGB. `COLORFGBG` is only a best-effort Syntect light/dark hint. Status items include Agent, Model, Provider, Effort, SessionTps, Directory, Branch, Tools, Session and State; defaults and custom rows use the same renderer. Exact historical default shapes migrate, arbitrary custom left/right layouts do not. `/think instant|low|med|high|xhigh|max` changes typed provider-native reasoning effort for subsequent calls; provider-private reasoning remains hidden and only safe activity/timing projection is shown.

While a turn is live, **F6** toggles thinking activity and **F7** toggles typed tool details. Owned Agent Inline and Fullscreen reproject the current frame immediately. Scrollback stops emitting new detail immediately and changes its live reserved-viewport projection to compact hidden labels; already-committed terminal scrollback is never retroactively erased. These toggles never cancel or alter model/tool execution. Ctrl-C remains cooperative cancellation. Provider-private reasoning is never exposed: Thinking is a safe activity/timing projection only. When no visible delta exists and therefore TTFT is unavailable, the settled UI uses measured turn elapsed time for the safe Thinking duration instead of silently omitting the Thinking row.

Conversation projection treats `Turn.answer` as the authoritative visible assistant result and `context_messages` as a richer trace. Intermediate assistant/tool trace is rendered in order, but if a restored/partial trace does not contain the terminal assistant answer, inline and fullscreen append the bounded/sanitized authoritative `Turn.answer` exactly once. A non-completed turn with no visible answer renders its bounded safe error/status instead of leaving an unexplained blank gap; a supposedly completed turn with no visible assistant result renders an explicit warning. This is presentation recovery only and never fabricates provider output.

Transcript styling is intentionally Pi-like and low-chrome. User prompts are full-width muted-blue semantic blocks with one colored padding row above and below content, no `›` marker, and no implicit horizontal inset; configured output padding alone controls horizontal content spacing. Assistant text remains unboxed normal foreground. Tool records have three semantic muted strips in every theme: **success → muted green**, **failed/denied/interrupted/error output → muted red**, and **running/missing/other → muted gray**. Tool output is sanitized and bounded; collapsed tool mode retains a compact status label and omits detailed arguments/output. Core runtime records only raw elapsed/first-token/token-count measurements; `PerformanceExtension` exclusively owns TPS/token/TTFT/waterfall formatting and after-turn placement, and never fabricates token metrics when inputs are unavailable.

## Current Workspace implementation ownership — 2026-10-07

Workspace keeps one authoritative `Ui` state machine, but the implementation is partitioned by responsibility rather than concentrated in one monolithic reducer:

```text
src/workspace/ui.rs                  application façade, view-model types, construction,
                                     core layout/navigation state, typed projections, tests
src/workspace/ui/commands.rs         resolved human command execution
src/workspace/ui/interaction.rs      keyboard/mouse/search/palette/composer/viewport interaction
src/workspace/ui/async_work.rs       bounded Files/search/highlight/background coordination
src/workspace/ui/render.rs           visible-row projection, drawing and exact hit geometry
```

These are child implementation modules of the same Rust `Ui` type, not parallel controllers. They reuse the existing owner, shared `tui::composer::Interaction`, palette/session/tab seams and bounded background executor. No partition owns an independent state store, worker pool, slash registry, Agent engine, provider dispatch path or authorization boundary.

The split is guarded by `cargo test --locked -p aw-workspace`, which checks method ownership and prevents the heavy command/input/async/render reducers from drifting back into the façade. Historical Workspace source checkers inspect the logical source set across the façade and these four partitions so their behavioral assertions remain valid after the move.

See [accepted history](../plans/accepted-history/TODO.md).

## Layout and surfaces

Files/Git/Sessions remain the only navigator kinds. The current default orders them **Sessions / Files / Git**, uses a 22.5% Explorer split, and keeps Agent + Activity in the initial content group. Files and Sessions both expose Tree/List projection; Session tree mode nests every durable branch/fork beneath its Session, while Files loads directories lazily as the user expands them. Git exposes Changes/Worktrees/Branches and Tree/List projection for changes; change groups such as Staged/Unstaged/Untracked/Conflicts can be folded independently. Opening an existing worktree changes only that Git pane's human review context. Every content group accepts Conversation, read-only AgentView, SessionTree, Activity, File, Diff, Terminal, Settings and Help. `aw-workspace::Target` stores bounded opaque IDs for review/terminal references, not an executable command, provider key, root grant or live process. File and Diff each have a separate compatible unpinned preview slot; neither can replace a terminal/conversation/pinned view.

Moving a tab preserves its ID/resource. Hiding/closing terminal tabs does not terminate the process. Layout load restores structure, but unknown old resource IDs are unavailable; it never silently launches a shell or stages a diff. Existing private root-bound opt-in layout save/load, eight-pane/32-tab/two-navigators-per-kind constraints and responsive focused-pane fallback remain.

## Read-only Agent views

In Sessions, select a saved branch and press `o`, or choose **Open read-only
Agent view** in the Workspace palette. Enter keeps its existing activation
behavior. A read-only tab has no composer: opening, splitting, moving or closing
it preserves the current Agent and its draft/caret. Existing group actions let
two differently targeted views sit side by side, with independent scroll,
follow-end, find and selection state. Opening the same target in the same group
selects its existing tab; another group may hold a separate view of it.

A saved view pins exact Store, Session and branch IDs; titles and branch names
are labels. It requests fresh read authorization and grants no execution. An
exact current-Agent pin can follow its existing live projection after a fresh
access check. Other pins use the strict Store transcript reader and respect
active/unreconciled owner-run fences. Readable uncertain committed history stays
labelled for inspection; viewing never acknowledges or replays it.

Views show a bounded recent transcript, with explicit markers when older or
oversized content is omitted. Complete immutable records remain available through
Session history/export. Background reads use the bounded UI executor, independently
of the Agent command queue. Requests carry view/resource identities and unique
request IDs; closing or retargeting discards replies. Refresh rechecks authority
even when the branch head is unchanged. Denied, removed, expired or mismatched
Store content cannot remain available through render, find or copy caches.

Unsaved current Agents can browse an existing configured local SQLite State
without creating a Session. Browsing and later activation reuse one Host-owned
Store and its exclusive owner.lock. Remote and JSONL read-only pins remain
unavailable in this slice; there is no fallback to a different authority/backend.

Native saved layout v2 migrates consistent v1 layouts in memory, retaining tab
IDs, pins, geometry and root binding. Existing Conversation tabs continue to
follow the current Agent. Mixed/unsupported versions fail before installation;
atomic publication, no-follow reads and the 64 KiB limit remain. SQLite v33 and
JSONL v1 are unchanged. Browser containers and docking remain planned separately.

## Services and scope

```text
inline interface <-> native renderer
                         |
                  one owner command queue
                         |
                  existing Agent + journal
                         |
                  current accounts/tools

human file views -> lazy rooted read worker
human Git views  -> lazy bounded Git worker -> real Git
index mutations -> serialized native owner -> exact cached apply
terminal tabs   -> explicit retained PTY pool -> shell + emulator
```

Neither human file/Git views nor terminal output become model context automatically. Review comments are copied into an unsent same-root composer only on `send-review`. Provider/account/payer/model decisions remain tied to admitted prompts, not view movement.

Each Git pane independently selects Changes/Worktrees/Branches, remembers subview filter/selection, and toggles List/Tree without changing the selected underlying record. Enter on an existing worktree changes that pane's review scope; Enter on a branch compares objects. The current agent and human Files root remain fixed. Multi-worktree Agent owner switching/concurrent agents are not implemented by adding these views.

Git indexes are manually writable through explicit `stage`/`unstage` on a freshly verified snapshot. These commands serialize with the native owner, but external shells/PTYS are independent. A result can be stale/unsupported/uncertain; the UI does not auto-retry. [Git details](GIT.md).

A terminal has its own explicit creation root and real child. It keeps running while hidden or while returning to inline, until explicitly stopped or the application owner exits. Ordinary terminal keys are not intercepted as composer/model actions. New shell launch requires `terminal-start`; `terminal`/`Ctrl+\`` only focus or toggle an already-running terminal and never launch one implicitly. [Terminal details](TERMINALS.md).

## Interaction

`Ctrl+Shift+P` or `F1` opens the searchable, context-sensitive Workspace command palette; `:` opens it from navigation mode. The typed namespaces are **`> Actions`**, **`! Windows`**, **`@ Files`**, **`# Sessions`**, **`$ Terminals`**, and **`/ Commands`**. `/ Commands` is exactly the same canonical slash registry used by the Agent composer; canonical slash commands are deliberately omitted from `> Actions` rather than duplicated. Up/Down wraps in both palette and slash menus, and aliases such as `/clone`, `/side`, and `/open` canonicalize without dropping inline arguments. Current slash additions include `/think`, `/theme`, `/settings`, `/title-generate`, `/btw`/`/side`, and `/resume`/`/open`; no-argument `/resume` opens the Session picker/palette. `Ctrl+P/N` are prompt-history keys inside the Agent composer; the Workspace palette accepts the traditional collapsed Ctrl+P terminal fallback only in navigation mode. Metadata-rich Ctrl+Shift+P and F1 remain global, but plain Ctrl+P is never stolen while the embedded Agent composer or a child PTY owns input. `Alt+E` moves focus to Explorer and `Alt+W` to the main content group; Alt+1..9 remains contextual. `Ctrl+Shift+E` / `Ctrl+Shift+G`, terminal toggle, tab cycling, split/refresh/maximize/save-layout shortcuts remain. In Agent, `Esc` leaves editing and `i` re-enters the composer. F6/F7 change presentation only, never execution.

The embedded Workspace Agent now consumes the same `tui::composer::Interaction` reducer as standalone Scrollback/Inline/Fullscreen. Slash completion, history/scratch restoration, multiline motion, Settings and ordinary editor mutation therefore have one implementation rather than parallel per-application reducers. Workspace prompt history is a bounded projection of the latest 128 authoritative turns; it is presentation state only.

Git commands: `git changes|worktrees|branches`, `git list|tree`, `filter TEXT`, `refresh`; navigate with arrows/j/k and Enter. `t` toggles Tree/List in Files, Sessions and **Git Changes only**. Worktrees and Branches do not show a meaningless List/Tree control. The mouse/keyboard display control is icon-only (with ordinary Unicode text-symbol fallback rather than requiring a Nerd Font). Folder/session/change-group Enter folds or expands the corresponding tree. Worktrees are discovered from real Git metadata and selecting one switches only that Git navigator context. `comment ROW TEXT`, `send-review`, `reviewed`, `hunk-next`, `hunk-prev`, `stage`, `unstage` operate on the active snapshot.

Files List mode is a directory browser, not a flattened tree: Enter on a directory descends into it, nested directories show `../` first to return to the parent, and switching Tree → List keeps the current directory. File preview clicks keep Explorer focus while activating the unpinned preview tab. Sessions expose **User / Autonomous / Archived** scopes plus Tree/List projection. Delete in User/Autonomous archives the selected visible Session(s) for the current stable user; Delete in Archived restores them. This is local presentation state in append-only `session.view.changed` events with explicit `sync_scope=local`, not Session deletion or security-audit authority. A multi-select archive/restore is one bounded atomic store transaction: every target is authorized before any view event is committed, so the user action cannot partially apply. The active Agent cannot archive itself; filtering clears multi-select and archive targets are intersected with visible rows; a visible child of an archived parent is promoted to a top-level root; List branch filtering resumes the branch that actually matched. Legacy owner-only `session.hidden/restored` state is a compatibility fallback only for that historical owner. The compact two-line list shows title/state then age/turn count/branch with running/completed indicators. When the current Agent is not yet durable, User Sessions shows muted `New ○` without persisting it. Mouse hit regions derive from rendered geometry; content tabs use target-specific colors, `●` only for actual dirty/pending state, double-click close for non-terminals, and `Ctrl+W` remains the keyboard close.

Read-only File tabs use lazy **Syntect** syntax highlighting after the bounded file has already been loaded by the explicit workspace file reader. The workspace augments Syntect defaults with embedded TOML and Nix grammars; filename/content detection maps `Cargo.lock` to TOML, `flake.lock` and valid JSON-shaped unknown lock/data files to JSON, and `.nix` to Nix. Diff views reuse the same syntax projection for code while retaining Git semantic add/remove backgrounds and colored metadata. Syntect selects a matching light/dark theme from the effective UI theme, and already-open file caches are re-highlighted immediately when the user changes the theme. Highlighting has no filesystem/model/provider authority, is cached/projected from already-loaded human data, and falls back to sanitized plain text when no useful syntax/theme match exists.

Fullscreen chrome intentionally reuses boundaries instead of stacking them: panes keep their real outer bottom border, no empty status row is reserved when no status extension row exists, and the Agent composer draws only its top separator while the pane border supplies the bottom boundary. The fullscreen composer is exactly **one editable display row**, matching the compact inline feel: when empty, `Ask anything…` plus the relevant send/command/navigation hints are rendered as muted placeholder text in that same row; no separate guidance row and no duplicate transcript empty-state hint are emitted. Multiline draft data may still exist, but the fullscreen composer projects only the active one-row editor viewport. Settings is a real fullscreen content tab rather than an overlay inside Agent.

Mouse-wheel scrolling works over main content panes as well as Explorer rows. In navigation mode `/` opens an incremental filter for the focused Sessions/Files/Git list; Files filtering uses the existing bounded rooted `WorkspaceFiles::search`, so matches can come from collapsed/unvisited directories rather than only the lazy tree cache; in a main content view it opens incremental text find. Enter on main-view find exposes flash-style single-key labels (`[a]`, `[s]`, …) for visible/ordered matches, while `n` / `N` cycles next/previous. Filtering and find are human presentation state only and never become model context.

Terminal commands: `terminal-start`, `terminals`, `terminal-open ID_OR_NUMBER`, `terminal-stop`, `terminal-forget [ID]`, `terminal-scroll OFFSET`, `terminal-live`. The group focused by a terminal receives ordinary key/paste events. Ctrl+C, Ctrl+W, Ctrl+\\, Esc, Tab, Enter and shell/editor chords stay with the child; only the small documented global workspace chords (palette/help, terminal toggle, tab cycling and direct navigator focus) are intercepted. Stop does not rewind command effects. `cancel-agent` and terminal Ctrl+C are deliberately different.

The original `split`, `add-nav`, `move-pane`, `move-tab`, `focus`, `resize`, `maximize`, `pin`, layout-save/load, session management, recovery and draft controls remain. Restoring or moving views cannot grant tools, change actor, or authorize a context transfer.

## Lifecycle, limits and source boundary

Input/paint never run blocking Git or PTY launch. Git stdout/stderr/row count/time and review caches are bounded. A watch channel coalesces terminal changes; a reactor drains hidden output. The terminal view renders current cells rather than forwarding child escape sequences to aw's terminal. This is a design/implementation direction, not a tested Rust integration or measured latency claim.

Returning inline keeps the same settled Agent/Host and retained PTYs. Quit actions are context-sensitive: ordinary `quit` appears only when no unsent/recovery text or live terminals need confirmation; `quit-discard` confirms unsent text, `quit-terminals` confirms terminal termination, and `quit-all` is shown only when both confirmations are required. A host/raw-mode cleanup guard restores the physical terminal on ordinary returns/errors; abrupt process death cannot be repaired by Drop. Disowned child processes may require external inspection.

Git comments/markers and terminal histories are memory-only. The current SQLite schema is v32. Phase 38 introduced v21 shared-prompt draft state, Phase 39 added bounded pipeline state at v22, the unnumbered post-Phase-39 repair restored legacy coordination shape at v23, Phase 40 added separate routing/affinity receipts at v24, Phase 41 adds bounded child-Session relations at v25, Phase 42 adds isolated child execution bindings at v26, Phase 43 adds durable root delegation envelopes/budget accounting at v27, Phase 44 adds bounded recursive lineage at v28, Phase 45 adds governed child-routing receipts at v29, and Phase 46 adds bounded team-template/run/join receipts at v30, Phase 49 adds planner operation/task/join receipts at v31, and Phase 50 adds guardian review/approval/release receipts at v32; none of these makes workspace layout/Git/PTY state model context. Manual index/PTY activity is not part of the native model-tool durable journal. The post-Phase37 design adaptation adds a local SecretStore-backed **Sign in with ChatGPT** provider path through OpenAI Responses; it does not make OAuth credentials part of workspace layout state and does not turn the Chat Completions adapter into a subscription adapter. Broader shared workspace geometry/files/PTY control, current-directory shell integration and remote personal-inference expansion remain separate concerns.

## Validation

Run `cargo test --workspace --locked`: the actual SDK consumers, Cargo dependency checks, Ratatui TestBackend tests, real Git/PTY library tests and `cargo test --release -p aw-cli` must pass. The modern inline path additionally has `cargo test -p aw-cli`, which drives the real Ratatui inline composer in a PTY, answers terminal cursor-position queries, verifies Ctrl+C clears only the draft without terminal-wide erase sequences, exercises transient slash/settings UI, and exits through `/quit` without contacting a provider. Python Git/PTY probes are independently executed lower-level checks; no screenshot or generated image is evidence the Rust interface works. [Build status](BUILD-STATUS.md).

## Phase 6 browser counterpart

The native behavior/limits above remain intact. The current browser root is now a SolidJS application using the same existing authenticated owner APIs and human file/Git surface host. Its primary layout is intentionally simpler than the historical three-region Phase-6 shell: a narrow Sessions/Files/Git explorer plus an Agent/content area and bottom composer, with Abird design tokens and the official Round Trip mark. Native and browser layout schemas are still not synchronized. One current Agent owner remains. Browser Git is read-only, while native guarded index updates remain native; retained historical browser surface modules remain compatibility/reference code while advanced surfaces are migrated into the Solid shell.

[Browser workspace](BROWSER-WORKSPACE.md) defines actual controls, site restrictions, terminal lease/polling and memory-only view persistence. [Surface SDK](SURFACES.md) supplies the I/O boundary without adding HTTP/UI to the native pure-layout crate. This is not a remote attach feature, and CLI/web still cannot concurrently own the same state directory.


## Phase 7 ownership note

The new `aw serve` can share an existing Agent/store with an inline native `aw attach` client and the browser. It does not yet make this fullscreen native renderer a remote client. Continue using the local `--workspace` path for native panels; do not start it against a store already held by serve. One current conversation remains in the shared owner, while read-only branch Inspect is independently targeted. Provider/user-device delegation and multi-worktree Agent contexts are later.

[Local owner](LOCAL-OWNER.md).

## Phase 10 context flow in the native workspace

Palette `context-action {JSON}` invokes the shared typed controller at an idle command boundary; `context` opens the bounded ContextFlow result view in the existing group. The action set covers permitted sources/branches/targets, prepare, manual/generate/status, apply/recovery and source-owner read sharing. The richer browser has a selector form; a fully graphical native multi-source picker is not claimed here.

Context imports/compactions render as attributed blocks at their projection positions. Source changes/revocation are not a reason to silently attach to another target or use another account. The same model/tool owner, cancellation, draft controls and explicit execution binding remain; no new navigator kind or second agent is created. [Typed commands](CONTEXT-FLOW-IMPLEMENTATION.md).

## Machine and Fleet target lens — design amendment

Workspace layout and resource identity remain independent from execution placement. Add an `ActiveTarget` to the workspace shell: Local, Machine, or Fleet. Navigators/content tabs may `FollowActiveTarget`, pin a specific machine/workspace, or show a Fleet aggregate. Switching target rebinds followers and defaults for new resources; it never silently moves a running session, terminal, or pinned tab.

A virtual FleetWorkspace is stored on the main host and maps a fleet to member MachineWorkspaces. In fleet scope, Files/Git/Sessions/logs/metrics are machine-labelled aggregate projections; a new terminal still requires a specific member unless the user explicitly chooses a high-risk broadcast action. Selecting one machine in the same workspace makes the UI behave as if aw were local on that machine through the MachineLink.

See [FLEETS](FLEETS.md).

## Phase 38 collaboration boundary

Phase 38 introduced SQLite v21 because shared prompt draft state became separate durable state; Phase 39 added bounded pipelines at v22; the unnumbered coordination-shape repair is v23; Phase 40 schema is v24 for routing/affinity receipts and Phase 41 schema is v25 for bounded subagent child-Session relations and Phase 42 schema is v26 for isolated managed-worktree execution bindings and Phase 43 schema is v27 for root delegation/budget accounting and Phase 44 schema is v28 for bounded recursive subagent lineage and Phase 45 schema is v29 for governed child-routing receipts and current Phase 46 schema is v30 for bounded multi-model team topology/join receipts. This does not make Files/Git/PTY/layout resources collaborative or model context. Phase 38's first product surface is the authenticated team browser composer. The native inline/fullscreen TUI remains unchanged and does not yet join a live shared-draft room.

The UI-neutral `aw-collab`/store contract is deliberately reusable by a later native TUI surface: plain prompt text, explicit target/generation, exact Send and ephemeral relative-presence semantics. Adding that surface later must not broaden model/tool/account authority or turn workspace geometry/terminal input/source files into the draft document.


## Phase 41 Agent families / subagents

The product-facing interactive thread is **Agent** while Session remains the durable object. `--subagents` explicitly registers the native `subagent` Tool only for a durable SQLite single-user owner; `--tools` alone never grants child inference. Phase-41 children are model-only and inherit no Files/Git/PTY view state, parent transcript, local tools or recursive subagent capability.

Sessions Tree mode nests direct child Sessions under their parent with durable lifecycle glyphs and foreground/background mode. When the current Agent is either the parent or a child in such a family, a compact floating **Subagents** sidecar is projected at the top-right of the Agent content region. It lists direct children and, from a child, a parent switch row; clicking a row resumes that Session's `main` branch through the existing owner command path. The sidecar is transient presentation state and is not persisted layout geometry or model context.


## Phase 42 isolated child markers

Phase-42 tool-enabled children remain ordinary nested Sessions. Sessions Tree and the floating Agent-family sidecar project their durable child tool mode as `read-only · isolated` or `workspace · isolated`; Phase-41 `off` children keep their foreground/background label. The managed filesystem path is secondary host metadata and is never injected into model context by the UI. Git Worktrees naturally lists the real managed checkout for human inspection; selecting it does not rebind an Agent automatically.


## Phase 43 delegation budget projection

The floating Agent-family sidecar consumes the same durable delegation root as the child runtime. When a current parent/child belongs to a Phase-43 root, its title includes a compact summary such as `Subagents active 2/32 · active 1/4 · calls 3/64`. These are host/accounting counters only and are not injected into model context. `aw subagent budget SESSION_OR_CHILD_SESSION` exposes the fuller root object. Depth remains fixed at 1 and no recursive Session tree is created by Phase 43.


## Phase 44 recursive Agent families

Sessions Tree recursively nests descendants by durable `parent_session` relation instead of stopping after one child level. Each child retains its state/tool-isolation marker and durable recursive depth. The floating Agent-family sidecar is intentionally **local** around the current Agent: when viewing a descendant it offers one direct-parent row plus the current Agent's direct children, while retaining the shared root-budget header. This provides stepwise up/down navigation without flattening an arbitrary recursive tree into the sidecar.

Default max depth 1 preserves the previous Phase-43 UI shape. Recursive lineage and managed paths remain host/UI metadata and are not injected into model context.


## Phase 45 child-routing observability

Child routing does not change the Agent-family navigation model. `aw subagent list` / `show` include the durable allocation object, which exposes routing operation ID, target ID, selection source and selected InvocationIdentity; `aw subagent budget` exposes the root's pinned child routing revision and shared counters. The current fullscreen sidecar continues to show relation state/tool-isolation and shared root-budget counters rather than inventing a routed-model badge. Routing metadata remains host/UI state and is not injected into model context automatically.


## Phase 46 team candidates in the existing Agent tree

Phase 46 deliberately adds no new navigator. Team candidates are ordinary child Sessions, so the existing recursive Sessions Tree and Agent-family sidecar already expose their lifecycle, tool isolation, routing and shared-root budget context. `aw team-template` manages immutable topology definitions; the root parent turn's `team` tool result carries the joined evidence and team-run ID. A richer Team/Pipeline/Logs content view is deferred to the later observability phase.


## Phase 47 command palette, hit geometry and clipboard feedback

The command palette uses title-case categories with natural prefixes: `> Actions`, `@ Files`, `# Sessions`, `% Windows`, `$ Terminals`. Typing a prefix or clicking its category activates the same parsed filter; clicking the active category returns to all-category search. No prefix searches all categories.

Explorer Sessions / Files / Git and main content tabs derive mouse hit rectangles from their actual rendered labels. When width is insufficient they stack vertically one-per-line rather than clipping. Existing User / Autonomous and Git Changes / Worktrees / Branches segmented controls retain the same responsive behavior. List hit indices use the actual `ListState` visible offset, so scrolled Files/Sessions/Git rows remain clickable at the item actually shown.

Scrollable main content, Explorer lists, Git lists and command-palette results show a minimal right-edge `│` track / `┃` proportional thumb only when content overflows. The scrollbar is render-only and persists no authority/state.

The empty fullscreen Agent composer remains exactly its top separator plus one editable row. Placeholder text degrades cleanly from the full contextual hint to `Ask anything…  / commands` and finally `Ask anything…` as width shrinks.

Read-only main content supports mouse drag selection and bounded OSC-52 copy. Selection is visible, Unicode/cell-width aware and viewport-bounded. Mouse release copies the selected text; Ctrl+C copies an active selection before falling back to the preexisting Agent-cancel/draft-clear behavior. Terminals, composer editing, palette controls, Settings and Explorer action rows keep their existing mouse/input semantics.


## Phase 48 saved-Agent switching and composer visibility

Selecting a durable Agent from Sessions, the command palette or `/resume` now reactivates that Session using **its saved binding** rather than trying to apply the currently selected Agent's directory/tool-root binding to it. This is not a Session migration: the original saved root/system prompt/profile/model/destination fingerprint remain authoritative. The owner rebuilds the local Agent/tool registry against that saved binding and only commits the UI switch if reconstruction succeeds. A changed/missing destination/profile fails visibly and leaves the previous Agent active.

After a successful switch the snapshot refreshes Session/title/branch plus root, tool mode, profile and model so Explorer/status surfaces no longer display the previous Agent's root. Ordinary explicit Session open under a caller-supplied different root remains denied by `check_binding`.

The empty fullscreen composer continues to use the Phase-47 adaptive single-line hint, but its semantic muted style no longer adds `DIM`. This specifically avoids terminals/themes where only the cursor-covered leading `A` remained visibly apparent.


## Phase 49 planner visibility

Phase 49 does not add a separate mandatory planner pane. Planned workers are ordinary child Sessions and therefore appear through the existing Session/subagent family projection. Planner/task/join receipts remain host/state data and are not injected into model context except for the explicit bounded final `plan` tool result. The current Agent remains the one publisher of the parent continuation.

Dependencies are displayed/recorded as task topology only; Phase 49 does not create child-to-child transcript sharing. A dependent child receives predecessor task IDs in its bounded task prompt but not predecessor answer text. Future coordination/peer-message UI requires a separate phase.


## Phase 51 native Agent transcript/fullscreen convergence (visual provenance; popup/physical mode details superseded by Phases 54, 57 and 58)

Phase 51 changes presentation/input only; SQLite remains **v32** and every Phase-50 provider/tool/routing/review authority boundary remains unchanged. Inline and fullscreen now decode the same semantic native palette: unboxed Pi-like user prompts, ordinary assistant foreground, muted italic Thinking activity, and full-width tool strips where **tool success** is muted green, failure/denial/interruption is muted red, and running/neutral state is muted gray. Tool output remains bounded and terminal-sanitized; F6/F7 remain projection-only controls.

Slash completion now shares the same semantic menu typography in both presentations. At Phase 51, Inline kept the popup below its two-rule composer while Fullscreen placed it above; current Agent presentations place slash completion above the composer consistently, using a neutral menu surface with bold command usage, muted description text and a deterministic accent-backed selected row in the Codex-like style rather than generic reverse video. The workspace command palette uses the same selected-row accent while keeping its richer category/search layout. The existing Codex-like fullscreen status projection is unchanged.

Presentation switching is explicit and lossless. `/inline` and `/minimal` set the live/persisted TUI mode to Inline **before** the alternate screen is left, so the next ordinary prompt cannot immediately reopen fullscreen. `/fullscreen` and `/workspace` explicitly enter the fullscreen alternate screen. Fullscreen no longer performs a second `terminal.clear()` after entering that fresh buffer; the first Ratatui draw owns the complete frame, reducing visible clear/flicker without weakening alternate-screen restoration. Inline never enters the alternate screen and never clears terminal-wide scrollback. The actual PTY workspace smoke asserts both directions: fullscreen → inline → next turn stays inline, and only the later explicit `/workspace` emits a new alternate-screen entry.


## Phase 52 safe native Markdown answer rendering

Phase 52 keeps SQLite at **v32** and changes only settled assistant presentation. Inline finalized scrollback and fullscreen Agent history now use one bounded native Markdown renderer for headings, bold/emphasis, inline code, lists, quotes, horizontal rules, fenced code blocks and textual links. User prompts, tool commands/results, Thinking/status/performance rows, files/diffs and terminal content remain outside the Markdown path. Live partial model output intentionally remains sanitized plain text until settlement, so incomplete Markdown is never reparsed on every token.

The renderer is terminal-native and effect-free: input first passes the existing display sanitizer, raw HTML is shown literally, link destinations are visible text only, no OSC-8 hyperlink is emitted, no ANSI/OSC/DCS/APC/PM from model text reaches the terminal, and rendering performs no filesystem/network/process action. Fenced code uses a restrained neutral full-width strip rather than executable terminal escape handling or heavyweight syntax highlighting. Width accounting uses Unicode graphemes/cell widths and expands tabs to bounded visible cells.

Fullscreen find/search/copy row geometry is derived from the exact same styled Markdown rows that are painted. Internal assistant markers are decoded before the selectable text projection and never enter clipboard text. Authoritative `Turn.answer` fallback uses the same renderer, while Phase-51 tool strips and presentation switching remain unchanged. Full CommonMark, tables, footnotes, images, HTML rendering, OSC-8 links, browser Markdown convergence and partial-token incremental Markdown remain explicit future work.


## Phase 53 shared native fenced-code syntax highlighting

Phase 53 remains native presentation-only on SQLite **v32**. Default/full builds now syntax-highlight **settled assistant fenced code** using the same lazy Syntect syntax/theme registry already used by human file/diff previews. The pure catalog/theme/highlight core lives in the shared TUI layer; the workspace file/diff wrapper keeps only file/Git presentation semantics, eliminating the duplicate-registry risk.

Fence language tokens are bounded local display metadata. Common aliases such as Rust, JavaScript/TypeScript, Python, shell, JSON, TOML, Nix, YAML and Markdown map only to the compiled Syntect catalog; there is no repository sniffing, grammar download, file lookup or command execution. Unknown/malformed/oversized tokens fall back to the Phase-52 neutral code strip without losing code text. Syntax foreground/font modifiers sit on the existing full-width neutral code background and follow effective System/Light/Dark syntax themes.

Syntect remains behind the existing `workspace` feature. Tiny no-workspace/stateless builds keep the Phase-52 neutral code-fence fallback and do not gain the syntax-highlighting dependency. Live partial model output remains plain/sanitized until settlement, and tools/prompts/status/file authority boundaries are unchanged.


## Phase 54 Agent/Workspace mode separation and terminal ownership — historical numbered substrate

> Historical checkpoint note: this section records the Phase-54 implementation as it existed then. The current Inline primary-screen/history model is the one documented at the top of this file and in `POST-PHASE58-UX-COMMAND-HARDENING-2026-10-07.md`; later unnumbered hardening supersedes conflicting physical-terminal details below.

Phase 54 keeps SQLite at **v32** and changes native application/presentation ownership only. The default native application is **Agent**, not Workspace. `aw` and `aw --mode=agent` run the Agent application; `aw --mode=workspace`, `aw workspace`, and `aw w` open the richer multi-pane Workspace application. `aw workspace open NAME` and the existing Workspace resource subcommands remain valid.

Agent presentation is separately selected with `--tui-mode=scrollback|inline|fullscreen` and persisted as `[ui].tui_mode`. The default is **inline**.

- **scrollback** keeps terminal-owned history and one persistent Ratatui inline viewport for the entire Agent lifetime. Prompt editing, slash completion, streaming and turn settlement reuse that same presenter; settled prompt/answer rows are inserted above it once.
- **inline** at Phase 54 owned the complete visible **primary** terminal screen without entering the alternate screen and pushed the prior visible shell screen into scrollback. This specific reservation model is historical: current Inline uses the dynamic `Viewport::Inline`/CPR model described at the top of this file, with a bounded fixed-bottom compatibility fallback only when CPR is unavailable.
- **fullscreen** is the same Agent renderer/state on the alternate screen. It is no longer an alias for the Workspace application.
- **workspace** remains the richer Explorer/Agent/Activity/Files/Git/Diff/terminal application and is selected as an application mode. `/workspace` explicitly hands the current Agent/Session into it; `/agent` returns. `/inline`, `/scrollback`, and `/fullscreen` switch only Agent presentation.

The ordinary Agent loop owns exactly one persistent `AgentPresenter` across prompt → popup → provider stream → tool stream → settlement → next prompt. There are no separate per-call `read_prompt` and `stream_turn` Ratatui terminals. Recoverable provider errors/cancellation are projected inside an owned Agent frame; slash/status commands that intentionally write physical terminal output first release presentation ownership and reconstruct it deterministically afterward.

The Phase-54 PTY regression originally proved owned Inline/Fullscreen separation. Phase 58 supersedes its fixed-Scrollback-viewport assumption: one aw Scrollback presenter persists while the internal Ratatui scratch viewport may re-anchor only when its required row count changes. A supplemental tmux visible-screen capture additionally proves that pre-aw shell output remains in scrollback, the slash menu stays attached to the lower composer before/after resize, settled prompts are not duplicated, and reopening `/` after a turn preserves the transcript instead of overwriting top rows.

Native scrollbar semantics are also unified: `┃` is the **active/thumb/current viewport** and always uses the brighter accent; `│` is the **inactive track** and uses the dim/faint gray. Individual panes no longer choose track/thumb ordering themselves.

Phase 51 remains the visual styling foundation while Phase 54 supersedes its old physical ownership/application-mode coupling.


## Phase 55 terminal lifecycle and shared Agent chrome

Phase 55 keeps SQLite at **v32** and changes native terminal/presentation behavior only. Agent Scrollback/Inline/Fullscreen, Workspace and onboarding now acquire the same generation-safe `TerminalSession` lifecycle guard. The guard centrally owns raw mode, cursor visibility, bracketed paste, mouse capture, alternate-screen state and primary-screen exit positioning. Normal Drop/error paths, the process panic hook and catchable Unix SIGHUP/SIGTERM/SIGQUIT shutdown all converge on one idempotent best-effort restore path. SIGINT is deliberately not intercepted so Ctrl+C remains an application key while raw mode is active. SIGKILL, host power loss and unrecoverable process abort remain impossible to repair in-process and are not claimed.

Idle Agent Ctrl+C now matches the visible help: the first press clears/arms a one-second quit window and displays `Ctrl+C again to quit`; the second idle/empty press inside that window returns a typed `Quit` outcome through the Agent run loop so presenter Drop restores the terminal before process exit. A delayed second press simply re-arms. Live-turn Ctrl+C remains cooperative cancellation. Actual PTY proof covers Scrollback/Inline/Fullscreen, SIGTERM, panic-after-terminal-entry, and a shell continuing on the same PTY after `/quit` with `icanon` + `echo` restored.

Owned Inline reconciles its physical size before every frame, and composer/status geometry is pinned to the physical bottom; terminal growth/shrink no longer leaves an unused band beneath the status rows.

Workspace chrome is intentionally quieter: navigator/content tabs no longer show numeric `[N]` prefixes and the left pane no longer has a redundant `Explorer` title. Alt+1..9 shortcuts still select by position. Scrollbar semantics remain Phase-54 bright thumb / dim track.

Phase 61/66 extend the same Skills projection without changing Workspace ownership: registered/installed skills remain inert until explicitly selected for the Agent run, and portable bundles remain instruction-only local resources. Phase 67 schedules operate through durable pipeline/store authority and do not add a second Workspace executor. Phases 64/65 filesystem-indexing/LSP work were intentionally skipped.

Agent-only and Workspace Agent-tab content call the same `tui::agent_chrome` projection for startup header, user prompts, Thinking, tools, settled/live assistant text, Markdown/syntax rendering and composer placeholder semantics. Agent-only uses the **Full** Pi-like header (`aw v…`, compact help, `[Context]`, `[Skills]`); Workspace uses the **Embedded** form, which omits only redundant outer product/help lines. **Phase 59 makes Context/Skills provenance typed and runtime-authored; Phase 60 activates Skills with explicit named resources.** `[Context]` reports the actual system-prompt origin/size, explicit instruction-file source names/sizes, and durable import/compaction metadata. `[Skills]` reports only successfully admitted repeatable `--skill NAME=PATH` selections as `NAME · PATH · N B`. Skill contents are sent only as explicit system instructions; local paths remain local provenance. No renderer/project/config-directory scans for skills. Inline/Workspace cache identity already includes the resource summary; Scrollback emits resources-only updates when provenance changes.


## Phase 56 non-blocking UI work graph and fast scrolling

Phase 56 keeps SQLite at **v32** and changes human Workspace scheduling/caching/viewport presentation only. The UI event/render thread is now treated as a reducer: terminal input mutates local intent, typed blocking/CPU work is submitted through one bounded shared `workspace::background::Executor`, result/progress events update caches, and paint reads those caches. Files list/read/search, Git inventory/review/worktree selection, file/diff syntax highlighting, persisted layout load/save, local terminal launch, large-view find and settled Workspace-Agent rich transcript projection all use that architecture. The Agent/model/durable owner worker remains a separate serialized authority and is never routed through the human UI pool.

The shared executor uses a small fixed worker set and bounded queue with `try_send`; queue-full returns immediately with a retryable UI error instead of blocking input. Worker panics are isolated into typed failure completions. Files and Git no longer own private worker threads. Layout restore is scheduled after the first Workspace frame, so startup paint is not blocked on disk. Trusted shell **launch** is backgrounded; ongoing PTY input/resize/terminate are already bounded driver messages and snapshots are in-memory parser state. Git stage/unstage intentionally remains on the serialized owner worker because it is a repository mutation/effect, not a human read-only preview.

Syntax is progressive. A file read publishes sanitized plain rows immediately, then the shared Syntect worker emits bounded 128-line chunks keyed by generation/theme/path. Diff highlighting follows the same cache/chunk path. Replacing a generation drops its cache and actively sets the worker cancellation token; stale generation/theme chunks are ignored. Theme changes invalidate to plain rows immediately and schedule new progressive work. Workspace Agent fenced-code highlighting also no longer runs Syntect in paint: the shared Agent chrome renders identical plain Markdown geometry immediately and the event loop schedules a rich background projection keyed by settled-turn identity, UI config and width. Text/search/copy geometry therefore does not move when syntax styling arrives.

Scrolling is local and immediate. `workspace::viewport::Viewport` replaces sentinel/integer offsets for main read-only views, uses a six-row wheel step, page movement based on visible height, saturating bounds, and follow-end semantics for Agent/Activity. Files/Diffs additionally allow bounded tail overscroll (up to roughly one third of the viewport, capped) so EOF does not pin the final source line to the bottom. Search/jump uses default three-row scrolloff when room exists. Overscroll rows are renderer blanks only: no fake source rows enter search/copy/line identity or scrollbar source positions. Explorer/Git/palette/terminal wheel handling uses the same fast local step where compatible and never submits background work.

Large File/Diff paint now projects only the visible requested slice. Git status is already background-parsed/sorted and the UI no longer redundantly re-sorts up to 4096 changes on each paint. Background-result storms are bounded by channels and the existing ~16 ms dirty-frame cadence; results mark dirty but never draw directly.


## Phase 57 natural-flow Agent layout and Scrollback exit — historical numbered substrate

> Historical checkpoint note: Phase 57 introduced natural-flow geometry. Current status rows, Inline overflow semantics, prompt bands, archive behavior and shared composer interaction are defined by the 2026-10-07 unnumbered hardening above.

Phase 57 keeps SQLite at **v32** and changes Agent layout/terminal handoff only. **Inline** and **Scrollback** no longer pin the composer/status stack to the physical terminal bottom merely because the terminal is tall. One pure flow-layout projection measures actual wrapped transcript/live rows plus transient status/notice/menu/composer rows. When that total is shorter than the terminal, the active Agent chrome grows downward naturally from the transcript/header and unused rows remain **below** the composer. Once content naturally fills the terminal, transcript projection scrolls while the composer stays at the physical bottom. Agent **Fullscreen** intentionally keeps the full-screen-owned geometry from Phase 54.

At the Phase-57 checkpoint the default status hierarchy was one compact `AboveComposer` row plus a dedicated FullscreenStatus row. That default is historical. Current defaults use the typed two-row left/right composition documented at the top of this file: directory + Agent identity opposite tools/state, then trustworthy run-average TPS opposite model/provider/effort. Both the older three-row default and the Phase-57/58 default migrate exactly; arbitrary custom left/right layouts are preserved. Legacy singular `status_line=[...]` remains accepted as one compatibility `above-composer` row.

Scrollback still uses one Ratatui `Viewport::Inline` scratch region for transient editing/settings/menu/live state, while settled prompt/turn rows are inserted into terminal-owned history. Phase 57 top-aligns all visible scratch content so the fixed reservation no longer creates a visible gap between committed history and the composer; unused reservation rows are below the composer. On every Scrollback presenter release the current inline viewport origin is captured from the last completed Ratatui frame, the scratch viewport is cleared with Ratatui's Inline clear semantics, and the cursor is placed at that exact origin before the shared `TerminalSession` restores modes. `/quit` first commits `› /quit` above the scratch viewport, then collapses it. Double-Ctrl+C collapses without inventing a prompt. The shell therefore resumes immediately after committed aw output with no stale composer/status rows underneath it.

The Phase-57 tmux proof covers short Inline natural flow, notice-above-composer ordering, long-content saturation/bottom pinning, compact Scrollback startup, settled Scrollback forward flow, and `/quit` commit + clean resumed shell. Phase-55 terminal lifecycle restoration and Phase-56 non-blocking UI/scroll architecture remain unchanged.


## Phase 58 dynamic Scrollback scratch viewport — historical numbered substrate

> Historical checkpoint note: Phase 58 remains the immutable numbered terminal substrate. Current default status-row counts and later Inline history ownership are superseded by the unnumbered 2026-10-07 contract; the Phase-58 Scrollback saturation/reanchor/no-replay principles remain authoritative.

Phase 58 keeps SQLite at **v32** and changes Scrollback presentation only. It supersedes one physical detail from Phase 57: Scrollback no longer keeps a fixed-height Ratatui `Viewport::Inline` scratch reservation. The aw-level Scrollback presenter/session remains persistent, but its internal Ratatui scratch terminal is rebuilt only when the actual active row requirement changes because Ratatui 0.30 exposes no public setter for an existing Inline viewport's configured height.

`scratch_required_rows(...)` measures the real transient UI: live Thinking/tool rows, Above/BelowComposer status rows, one notice row when present, slash-popup rows, and composer/settings/keymap rows. Idle default Scrollback therefore reserves only **4 rows** on a normal terminal (one compact AboveComposer status row plus the three-row composer with separators), not 18. Slash completion, live output, notices, keymap and Settings expand the scratch region only by their actual visible rows and it shrinks again when they close/settle. Settled prompt/turn output still uses Ratatui `insert_before` and becomes ordinary terminal-owned history exactly once.

The presenter observes the real current viewport rectangle. Before committed output naturally reaches the terminal bottom, a scratch-height change stays anchored immediately after committed history. Once `viewport.area.bottom() >= terminal_height`, saturation becomes sticky for that presenter. Later scratch-height changes and physical terminal resizes reanchor the compact viewport against the physical bottom, so a subsequent short/error turn cannot jump the composer back toward the middle. No blank fixed reservation exists above or below the active UI.

Ratatui's public API requires constructing a new internal `Terminal`/`Viewport::Inline(height)` when the configured height changes, which may issue a cursor-position query. This is an implementation detail inside one persistent aw presenter/session. The PTY contract therefore allows reanchor/CPR only when row count actually changes; ordinary redraw/navigation at a stable height must not create another anchor.

Phase-57 clean exit remains unchanged: `/quit` is committed into terminal history, transient scratch rows are cleared/collapsed, the cursor returns to the actual viewport origin and Phase-55 `TerminalSession` restores terminal modes before the shell resumes. The 50-row Phase-58 tmux proof shows natural unsaturated startup, real-content advancement to bottom saturation, sticky bottom placement for a later HTTP-error turn, and clean saturated `/quit` shell handoff.

## Post-Phase-58 native UX and terminal hardening — 2026-10-06 historical unnumbered snapshot

> Superseded where it conflicts with the 2026-10-07 contract at the top of this file and in `POST-PHASE58-UX-COMMAND-HARDENING-2026-10-07.md`. It remains useful provenance for the first editor/prompt/tab/terminal cleanup.

This unnumbered hardening keeps source `0.15.0` and SQLite **v32**. It does not define Phase 59. It supersedes only the current presentation/input details below; the durable authority/ownership model and the Phase-58 saturation contract remain intact.

The shared `aw-editor` / `tui::editor` path now accepts the familiar shell/readline word and line editing set in every Agent composer and compatible single-line input: Ctrl/Alt+Left/Right and Alt+B/F move by word; Ctrl+W and Alt+Backspace delete the previous word; Alt+D and Alt+Delete delete the next word; Ctrl+A/E/B/F, Ctrl+U/K, Ctrl+D/H, Home/End and ordinary character deletion retain their familiar meanings. Shift+Enter joins Alt+Enter and Ctrl+J as a composer newline chord. Actual tmux input is tested, not just synthetic `KeyEvent`s.

Current Pi source was used as the geometry/reference check. Pi's editor renders a top border, the visible editor line(s), and a bottom border, with horizontal `paddingX` only; a one-line editor is therefore three terminal rows. aw already matched that geometry, so no extra vertical composer padding was added and the existing vertically centered placeholder/cursor placement is retained. Pi's user-message component applies horizontal and vertical Markdown padding inside a full-width themed user-message background; aw now mirrors the visual distinction with a full-width muted-blue user-prompt band and no `›` marker.

Live model work no longer consumes a separate `Thinking…` row immediately above the composer. The composer's top rule carries one tiny shared three-box activity animation (`▪▫▫` cycling) and, when width permits, elapsed wall time aligned at the right. Provider-private reasoning remains hidden. F6 still controls the safe settled Thinking/timing projection; F7 still controls typed tool detail. The same three-box primitive marks running Agents in the Sessions list.

This checkpoint first moved the default back to two rows. The current 2026-10-07 form keeps two rows but is fully typed/composable: left/right groups share one renderer for defaults and custom rows; Agent, Provider, Effort and SessionTps are first-class items; TPS is admitted only for trustworthy single-invocation/no-tool timing; and both historical default shapes migrate exactly.

Padding is now presentation-specific: `editor_padding_x` / `output_padding` belong to standalone Agent modes, while `workspace_editor_padding_x` / `workspace_output_padding` independently control the embedded Workspace Agent/content geometry. Native Settings exposes all four; config/API validation keeps each bounded to `0..=3`. Workspace defaults to one cell of horizontal breathing room while standalone Agent remains zero by default.

This checkpoint introduced the two-line Sessions List and multi-select UX. Its original global visibility tombstone has since been replaced by the current **per-user archive view**: `session.view.changed` semantic events preserve immutable Session history, members do not inherit another user's archive preference, Archived scope restores Sessions, active-Agent and filtered-selection safety are enforced, and visible children of archived parents remain discoverable.

Content tabs now use one `draw_group_tab_strip()` seam that both paints labels and emits mouse hit rectangles from the exact same geometry. This removes the historical drift where Help/Explorer-style labels could only be activated by clicking slightly to their left. Escape on an active Help tab closes that Help view as advertised.

Terminal cleanup is stricter. Scrollback clears the old physical scratch footprint with `Clear(FromCursorDown)` before rebuilding an Inline viewport on resize, preventing stale old-size rows from surviving above/below the new viewport. Owned-primary Inline publishes the first transient row after settled transcript content to the centralized `TerminalSession`; restore clears from that row down and resumes the invoking shell there instead of forcing the cursor to the physical bottom. This removes the terminal-height blank gap after short/resized Inline sessions while preserving settled transcript/history. The post-Phase-58 tmux smoke exercises real modifier editing, repeated Inline/Scrollback resizes, duplicate-artifact detection, and clean shell resumption.
