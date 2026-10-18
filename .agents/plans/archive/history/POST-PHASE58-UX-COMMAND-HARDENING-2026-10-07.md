# Post-Phase-58 UX / command / architecture hardening — 2026-10-07

**Status:** implemented and validated; ready for user review
**Numbered phase:** none. **Do not call this Phase 59.**
**Source version:** 0.15.0
**SQLite schema:** v32 (unchanged)
**Authority:** this document is the newest narrow authority for the native Agent/Workspace UX, command palette, Session-list presentation state, Agent editor/status/persistence and Inline terminal behavior described here. The 2026-10-06 comprehensive handoff, Phase 58 and earlier phase documents remain authoritative for everything else.

## Why this checkpoint exists

A fresh post-Phase-58 review found several real correctness/maintenance issues beneath the previously green UX checkpoint:

1. owned Inline still manufactured a terminal-height block of history before it had real overflow;
2. the immediately previous Phase-57/58 default status layout did not migrate to the newer default;
3. “delete Session” used security audit as mutable UI state and could hide a running Agent / orphan visible children;
4. standalone Agent output padding was exposed but not actually consumed;
5. word movement/deletion did not match the familiar shell/readline run semantics used as the product reference;
6. richer readline behavior was split across presentation modes and modifier disambiguation was incomplete;
7. displayed average TPS could include tool/model-loop wall time and therefore mislabel Agent-loop latency as model throughput;
8. Session List branch search could display one matching branch and resume a different one;
9. multi-select could retain invisible destructive selections after filtering;
10. default status rendering was a magic special-case instead of the same typed composition used by custom rows;
11. System theme semantic surfaces guessed RGB light/dark from terminal hints;
12. Web Settings lagged the separate Agent/Workspace padding model;
13. src/workspace/ui.rs and src/tui/inline.rs continued accumulating duplicated policy/input reducers.

The same user request also defined the command-palette namespace:

- `> Actions`
- `! Windows`
- `@ Files`
- `# Sessions`
- `$ Terminals`
- `/ Commands`

and required wrapping selection plus the slash commands/aliases documented below.

## Current command model

### One slash registry

src/tui/commands.rs is the canonical slash-command vocabulary used by:

- Agent composer slash autocomplete;
- Workspace Agent composer slash autocomplete;
- command-palette / Commands;
- alias canonicalization and help text.

Current additions:

- /think instant|low|med|high|xhigh|max
- /theme auto|dark|light
- /settings
- /fork, alias /clone
- /title-generate
- /btw QUESTION, alias /side
- /resume [SESSION [BRANCH]], alias /open

Aliases resolve to one canonical command and preserve inline arguments in both composer completion and command-palette Tab/Enter.

### Palette category contract

src/workspace/palette.rs owns category prefix/query/cycling policy:

- > Actions — direct Workspace actions that do not require another specialized UX;
- ! Windows — windows/views;
- @ Files — files;
- # Sessions — saved Sessions/Agents;
- $ Terminals — retained terminal resources;
- / Commands — exactly the composer slash-command vocabulary.

Canonical slash commands are deliberately excluded from > Actions; the same intention is not duplicated under two categories.

Up/Down selection wraps at both ends in the command palette and slash menus.

### /think

ReasoningEffort is a provider-neutral typed value: instant, low, med, high, xhigh, max.

The command changes actual provider request shape, not only UI text:

- OpenAI Chat uses the adapter’s native reasoning-effort field;
- OpenAI Responses uses the native Responses reasoning object;
- Ollama remains truthfully boolean: only efforts representable as off/on are accepted; unsupported precision fails explicitly rather than silently remapping.

Changing effort rebuilds the provider runtime while preserving the durable Session/history/authority path. The selected effort is available to the typed status projection.

### /theme

/theme auto|dark|light changes the shared typed UiConfig and uses the same persistence path as Settings.

System/auto rendering leaves terminal base foreground/background authoritative. Semantic prompt/tool surfaces use terminal/ANSI semantics rather than guessed fixed RGB values. COLORFGBG remains only a best-effort syntax-theme hint where Workspace syntax highlighting needs an explicit light/dark palette.

### /resume / /open

With explicit arguments, the existing durable Session/branch resume path is used.

With no argument:

- standalone Agent opens a searchable/wrapping Ratatui Session picker;
- Workspace opens the # Sessions palette/filter UX.

The picker/filter is presentation only and never changes Session authority until the user chooses a concrete target.

### /btw / /side

A side question is not an ordinary Agent turn and never executes tools.

It uses the new AuxiliaryExecutor:

- tool-free provider invocation;
- same actor/account/model authority and context projection as the current Agent;
- durable auxiliary.started intent committed before provider dispatch;
- cancellation/deadline before intent is not a paid attempt;
- account/lease authorization is revalidated before dispatch and during streamed output;
- bounded complete-candidate output;
- auxiliary.finished settlement is durable and not raced by UI cancellation;
- proposed tool calls are rejected;
- result is shown as an explicit out-of-band BTW projection and never enters ordinary conversation history;
- unknown/failed settlement is never automatically replayed.

The auxiliary input digest binds kind, public prompt/context, output budget and timeout.

### /title-generate

Uses the same journaled tool-free auxiliary path. The model result is settled first; only then may the durable Session title be mutated. Blank/unusable output fails rather than silently changing metadata.

## Shared Agent editor / composer interaction

aw-editor::Editor owns bounded Unicode-safe editor state:

- grapheme-safe ordinary movement/deletion;
- three-class word runs: whitespace, word-like, punctuation;
- undo stack;
- kill ring;
- yank / yank-pop.

Current familiar key behavior includes:

- Ctrl+A / Ctrl+E — line start/end;
- Ctrl+B / Ctrl+F — one grapheme left/right;
- Ctrl+Left / Alt+Left / Alt+B — previous word/punctuation run;
- Ctrl+Right / Alt+Right / Alt+F — next word/punctuation run;
- Ctrl+W / Alt+Backspace — delete previous word/run;
- Alt+D / Alt+Delete — delete next word/run;
- Ctrl+U / Ctrl+K — kill to line start/end;
- Ctrl+Y — yank;
- Alt+Y — rotate kill ring;
- Ctrl+_ / Ctrl+- — undo;
- Ctrl+P / Ctrl+N — previous/next prompt history;
- Up/Down — slash selection, multiline vertical movement, otherwise history;
- Shift/Alt+Enter or Ctrl+J — newline;
- Ctrl+D — delete next character; empty-input quit remains an outer Agent behavior.

TerminalSession opportunistically enables terminal keyboard-enhancement flags only for known direct protocol-capable terminals or when tmux positively reports support. Portable fallbacks remain Ctrl+W / Alt+Backspace etc.

src/tui/composer.rs is now the shared Agent interaction state machine for Owned Inline idle/live, Owned Fullscreen idle/live and Scrollback idle/live. It owns slash selection/completion/wraparound, history/scratch restoration, multiline movement, /settings, submit semantics and ordinary editor mutation. Presentation-specific code keeps only real terminal lifecycle / busy-turn / quit differences.

## Status rows and truthful performance

StatusLineConfig now has typed left and right item groups. Default and customized rows use the same renderer; there is no “exact default gets magic layout” path.

New typed status items include Agent, Provider, Effort and SessionTps.

Current default two-row structure:

1. left: compact directory + agent <title-or-main>; right: tools + state;
2. left: measured run-average TPS when trustworthy; right: model + provider + effort.

The exact historical three-row default and the immediately previous Phase-57/58 default migrate to the current default. Arbitrary custom left/right layouts are preserved.

TPS is recorded only when elapsed/first-token timing belongs to a single model invocation with no tool execution interleaved. Multi-round/tool-loop wall time is excluded, so tool latency and later TTFT cannot be mislabeled as model generation throughput. It remains current-process/run measurement, not reconstructed historical truth.

## Agent / Workspace padding

Four independent bounded settings remain authoritative:

- editor_padding_x
- output_padding
- workspace_editor_padding_x
- workspace_output_padding

All are 0..=3.

Standalone output_padding is now actually consumed and is symmetric horizontal inset. Workspace output padding uses the same symmetric presentation model. Native Settings and Solid Web Settings both expose all four values.

## Session list: per-user archive, not security audit state

“Delete from Sessions list” is now modeled as per-user archive presentation state, not Session deletion and not security-audit authority.

SessionStore::set_session_archived writes semantic session.view.changed commit-log events scoped by Session, stable viewer/user identity and archived boolean.

Consequences:

- immutable Session history/receipts/nodes remain untouched;
- another member’s Session list is not hidden by the owner’s presentation choice;
- exact authorized Session access remains possible;
- legacy owner session.hidden/session.restored audit state is read as a compatibility fallback only for that historical owner and is superseded on first explicit archive/restore change.

Workspace has User / Autonomous / Archived Session scopes. In Archived scope the same list action restores the selected Session(s).

Safety/presentation rules:

- the currently active Agent cannot archive itself from its own active view;
- selected action targets are intersected with the currently visible filtered set;
- changing Session filter clears multi-select to avoid invisible destructive targets;
- if a parent Agent is archived while a child remains visible, the visible child becomes a top-level root instead of disappearing;
- List branch filtering binds Enter to the branch that actually matched the query.

The compact two-line List presentation and animated running/completed indicators remain.

## Inline / Scrollback terminal ownership

### Normal owned Inline

Owned Inline no longer reserves a terminal-height block of blank history on entry.

Normal terminals use Ratatui Viewport::Inline:

- historical turns loaded from durable storage may fall out of the active viewport but are never re-emitted into shell history;
- the header is the first presentation element dropped when geometry tightens;
- after real physical saturation, only newly settled runtime turns that must be evicted are committed to terminal scrollback through insert_before;
- pending/live/composer/status rows are never committed as settled terminal history;
- later resize remains bottom-anchored after saturation;
- exit boundary restores the shell adjacent to aw content, not at the physical bottom.

### CPR-less compatibility fallback

Bare PTYs / limited terminals that cannot answer cursor-position reports cannot support Ratatui Viewport::Inline. In that case aw uses an explicit bounded fixed-bottom compatibility viewport.

That fallback is deliberately degraded:

- it may reserve the visible primary screen so the shell remains usable;
- it never advances runtime-turn eviction as if insert_before had succeeded;
- it therefore never falsely claims a terminal-history commit.

Real tmux/terminal acceptance tests exercise the true Inline path.

### Scrollback

Phase-58 dynamic scratch semantics remain authoritative:

- one persistent presenter;
- committed settled output above one Ratatui Inline scratch viewport;
- scratch rows sized only to active human-visible chrome;
- old physical scratch footprint cleared on resize;
- sticky bottom anchoring after real saturation;
- deterministic scratch collapse and clean shell handoff on quit.

## Shared coalescing application config persistence

src/config_persistence.rs provides one bounded coalescing application persistence service:

- UI state mutates in memory immediately;
- latest complete FileConfig snapshot wins;
- a one-token bounded wake channel prevents unbounded queues;
- ordinary saves are background/coalesced;
- flush() is an explicit durability barrier for mode handoff/process exit and identity-sensitive OAuth bootstrap;
- full and minimal Agent hosts share the same service.

Workspace does not enqueue every settings tick as a normal owner command. It has a latest-value pending_ui snapshot plus PersistUiWake, so rapid Settings changes remain nonblocking and do not flip ordinary state_pending.

## Workspace decomposition completed in this hardening

New policy is behind real typed seams instead of continuing to grow src/workspace/ui.rs:

- src/workspace/session_nav.rs — Session scope/list/tree/subagent row projection;
- src/workspace/palette.rs — palette category/prefix/query/cycle/alias resolution;
- src/workspace/tab_strip.rs — one paint + exact hit-map geometry implementation;
- src/workspace/agent_view.rs — settled/live/BTW Agent projection and shared Agent-chrome delegation;
- src/tui/composer.rs — shared Agent input reducer;
- src/config_persistence.rs — shared bounded coalescing app persistence.

workspace/ui.rs remains the façade/application reducer/render shell and should continue to be decomposed only behind similarly typed behavior-preserving seams.

## Web parity

Solid Web Settings now round-trips Agent editor padding, Agent output padding, Workspace editor padding, Workspace output padding, typed status row items/right_items and the new status fields.

The committed embedded web/dist/app.js was rebuilt from current source.

## Validation evidence

Green on this checkpoint source:

- cargo fmt --all -- --check;
- cargo test --locked --workspace --all-targets;
- full native cargo test --locked --bin aw: 236/236;
- cargo clippy --locked --workspace --all-targets -- -D warnings;
- cargo test --locked --workspace --doc;
- RUSTDOCFLAGS='-D warnings' cargo doc --locked --workspace --no-deps;
- SDK dependency-boundary checks;
- minimal/no-default cut;
- individual web, surfaces, workspace, client, device, team, multiplayer, pipelines, routing, net and oidc cuts;
- combined team,multiplayer and web,oidc cuts;
- provider-native reasoning-effort request-shape tests;
- auxiliary inference adversarial tests and durable-store receipt/history regression;
- per-user archive/history/legacy-owner migration regressions;
- Solid/Vite Web Settings build;
- current reference/static validation profile: passed, including the refreshed post-Phase-58 UX checker at 163 assertions;
- Phase-55 actual PTY lifecycle smoke: 8/8;
- Phase-57 real tmux natural-flow/overflow smoke: 6/6, including proof that owned Inline commits only later settled overflow after genuine saturation;
- Phase-58 Scrollback saturation smoke: 4/4;
- post-Phase-58 real terminal UX smoke: 4/4;
- all SDK consumers, including explicit terminal PTY and in-memory inference fixtures;
- aw-store --lib parallel suite repeated twice after one non-reproducible Phase-35 direct-lock failure in a long validation wrapper.

### Validation wrapper / release-link limitation

The monolithic validation_gate.py run --profile rust is too long/resource-intensive for the current dotlink bridge. One run reached the Workspace suite and hit a single Phase-35 direct-lock test failure; that exact test passed alone and the complete aw-store --lib parallel suite then passed twice. Later wrapper attempts were terminated by the connector while progressing through otherwise green feature-cut logs.

The optimized full release link could not be completed through the current connector: repeated cargo build --release --features full calls terminate the local bridge before a new binary replaces the stale Sep-29 artifact, even with one Cargo job. That stale artifact is not counted as tested. Fresh full-feature debug binaries are the artifacts used by the actual PTY/tmux tests above. Release-independent compiler, Clippy, docs, feature-cut, static and runtime proofs are green.

Do not convert this environment limitation into a false release claim. Re-run the release build/install smoke on a host/bridge that can sustain the optimized link before publishing a release.

## Explicit non-goals / unchanged authority

- no Phase 59;
- no schema v33;
- no historical tag rewrite;
- no push;
- no destructive Session purge;
- no new provider/tool/routing authority;
- no model-selected authority;
- no change to immutable history / CAS / writer epochs / no-replay invariants;
- no raw SQLite/WAL sync;
- no change to parent-only publication or guardian authority boundaries.

## Next explicit work, only if authorized

The previously recommended architecture work is now partially completed: Session navigation, palette policy, tab geometry, Agent view, composer reducer and config persistence have typed seams.

Remaining high-value decomposition should target the still-large production portion of workspace/ui.rs behind:

1. command execution;
2. file/Git async coordination and cache ownership;
3. main interaction reducer;
4. renderer/hit-map façade boundaries.

Do not start a numbered Phase 59 without explicit user authorization.
