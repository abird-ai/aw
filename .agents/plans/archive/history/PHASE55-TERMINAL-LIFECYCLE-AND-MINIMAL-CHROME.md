# Phase 55 — terminal lifecycle, bottom anchoring and minimal Workspace chrome

Status: authorized 2026-10-06 by the user after Phase 54 review.

## Boundary

Phase 55 is **native terminal/presentation lifecycle only**. SQLite remains **v32**.

It must not change provider/account/tool/routing/subagent/planner/guardian authority, Session persistence, model context, billing/retry behavior, filesystem grants or browser ownership.

## Goals

1. Owned Agent Inline/Fullscreen chrome stays physically bottom-anchored as the terminal grows/shrinks: the last configured status row reaches the last usable terminal row and the composer flows immediately above it.
2. Workspace chrome is cleaner: remove numeric `[1]`, `[2]`, ... prefixes from navigator/content-tab labels and remove the redundant `Explorer` pane title while retaining Alt+1..9 keyboard selection semantics.
3. Idle double Ctrl+C really exits, matching the UI/help text.
4. All native terminal ownership uses one idempotent lifecycle guard so normal exit, error return, panic and catchable Unix termination restore the terminal consistently.

## Shared terminal lifecycle

Create one native terminal-session guard used by:

- Agent Scrollback;
- Agent owned Inline;
- Agent Fullscreen;
- Workspace.

The guard tracks only physical terminal modes and restores them idempotently:

- raw mode;
- cursor visibility;
- bracketed paste;
- mouse capture;
- alternate-screen ownership;
- primary-screen owned exit position/style.

Normal Drop/error paths and explicit release use the same restore implementation.

Install a process panic hook that invokes the same best-effort restore before delegating to the previous panic hook.

On Unix, catchable termination signals used for process shutdown (SIGHUP/SIGTERM/SIGQUIT) are handled on a safe signal-wait thread which invokes the same restore routine before terminating. SIGINT remains an in-app key while raw mode is active so Ctrl+C semantics remain deterministic.

No program can restore terminal state after SIGKILL, power loss, kernel termination, or an unrecoverable process abort; docs/tests must not claim otherwise.

The guard must be single-owner and generation-safe so dropping an old presenter can never restore a newer active presenter.

## Owned Inline bottom anchoring

The primary-screen Agent must self-heal geometry:

- reconcile the Ratatui/backend physical size before every frame;
- redraw one complete owned frame;
- compute transcript bottom-scroll from wrapped visual rows;
- place composer + popup + below-status + notice stack against the physical bottom edge;
- when no popup/notice exists, the final below-composer status row is the last physical row;
- terminal growth cannot leave an untouched band below the status stack.

## Double Ctrl+C

Idle Agent input uses a bounded second-press window (target 1 second):

- first Ctrl+C clears a non-empty draft, or when already empty arms quit and displays `Ctrl+C again to quit`;
- second Ctrl+C while idle/empty within the window returns a typed Quit outcome and exits cleanly;
- any ordinary edit/paste/submit/mode/settings action disarms the pending quit;
- Ctrl+C during live model/tool activity remains cooperative cancellation, never immediate process exit.

The same idle behavior applies to Scrollback, Inline and Fullscreen Agent modes.

Workspace embedded terminals keep owning their own Ctrl+C as before. Workspace global Agent-input Ctrl+C semantics remain separate from shell PTY input.

## Shared Agent chrome

Agent-only mode and Workspace's `Agent` tab use one shared native Agent chrome projection.

Shared code owns:

- Pi-like startup resource header;
- `[Context]` and `[Skills]` sections based on truthful host-loaded metadata;
- user prompt rows;
- settled assistant/Markdown/code rendering;
- safe Thinking rows;
- tool success/failure/neutral strips;
- live answer/tool projection;
- composer placeholder semantics.

Agent-only uses the **Full** startup header, including `aw v...`, concise real key/help hints and loaded-resource summary. Workspace uses the **Embedded** header: same Context/Skills and transcript chrome, but omits redundant outer product/help lines because Workspace already supplies application-level chrome.

Do not scan arbitrary files and claim they are loaded skills. Until aw has an explicit skill-loading runtime, the Skills section may truthfully report `none loaded`. Context may identify the system prompt and other context sources actually admitted by the host, but never expose secret prompt contents merely for decoration.

Workspace remains free to use tab-local pane geometry: its composer may use the pane border as the lower boundary while Agent-only uses its full-screen rule geometry. The semantic placeholder/menu/status/transcript projection should be shared wherever geometry does not require a different container.

The legacy private marker-based Workspace conversation renderer is removed; painted rows and search/copy plain rows derive from the same shared Agent chrome.

## Workspace minimal chrome

Remove numeric prefixes from all visible navigator/content-tab titles:

- Sessions / Files / Git remain plain labels even while Explorer is focused;
- Agent / Activity / File / Diff / Terminal / Settings / Help tabs remain plain labels;
- Alt+1..9 remains contextual keyboard selection and is documented without requiring visible numbered prefixes.

Remove the literal `Explorer` pane title. Pane border/focus styling remains enough to communicate the region.

## Validation

At minimum:

- one shared terminal-session guard is used by Agent and Workspace;
- duplicated raw/alternate/paste/mouse restore implementations are removed;
- panic-hook restore is installed before interactive run;
- Unix catchable termination restore is wired without intercepting Ctrl+C SIGINT semantics;
- guard restore is idempotent and owner-generation safe;
- normal `/quit`, double Ctrl+C, provider error, Workspace→Agent return and mode switches restore terminal modes cleanly;
- PTY exit checks show canonical mode restoration sequences and a usable shell afterward;
- a panic-path subprocess/PTY regression proves cursor/raw/alternate restoration before panic output when possible;
- owned Inline status stack reaches the physical bottom before and after terminal growth/shrink;
- provider-failure settlement keeps one composer at the bottom;
- double Ctrl+C exits within the bounded window and a delayed second press does not;
- Workspace has no visible `[N]` navigator/tab prefixes and no `Explorer` title;
- Agent-only and Workspace Agent tab call the shared Agent chrome transcript/header projection;
- full Agent header includes product/help + Context/Skills; embedded Workspace header includes Context/Skills but omits outer product/help lines;
- shared Agent transcript search/copy projection has no second marker-based renderer;
- Alt+1..9 keyboard selection tests remain green;
- bright-thumb/dim-track scrollbar semantics remain green;
- Phase-54/53/52/51/50 checkers remain green;
- schema remains v32;
- whole workspace/all targets, Clippy `-D warnings`, doctests/rustdoc, feature cuts and native PTY smokes remain green.

## Deferred

- perfect restoration after SIGKILL/host power loss/process abort;
- terminal-emulator private snapshot protocols;
- browser presentation parity;
- Workspace layout redesign beyond the requested chrome removal;
- execution/orchestration authority changes.
