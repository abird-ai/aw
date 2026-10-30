# Real terminal service — Phase 5 / 0.6.0

## Live migration of an original Workspace PTY — Linux, 0.15.0

Focus an existing locally controlled terminal, cancel unused docking offers,
then invoke **`terminal-promote`** in the Workspace palette. Copy the printed
`aw workspace attach INVITE` command into another native terminal; F12 detaches.
After successful release the original Workspace may exit: the same shell PID,
parser, inactive/main/alternate buffers and bounded scrollback belong to the
independent broker. The old view is a frozen source snapshot. `terminal-forget`
removes that released source resource without stopping the independent shell.

`aw workspace persistent inspect DIRECTORY` reports recorded custody without
retrying input or activating anything. After detaching, use the printed
`aw workspace persistent stop INVITE` to stop the exact shell. Stop success
requires observed exit; escaped/disowned jobs can remain.

**Verified on `9d82f1a`:** real original Workspace exit and post-release
SIGKILL, real `vim`/`less`, Unicode/attributes/partial CSI, one new DSR response,
exclusive input, Ctrl+C, F12/termios restoration, explicit stop, 3,000-line
stream cutover and 26 admission/crash cases. SIGKILL cannot restore the killed
source terminal; the attached recipient still restores its own terminal.
Real different-UID FD admission rejection also passes in both directions
using an isolated user namespace (`65f04ca`); host privileges are unchanged.
Linux trusted-local scope only; no broker crash/reboot restoration or
remote/distinct-Actor grant is claimed.

See [custody protocol](../plans/workspace-views/NATIVE-PTY-CUSTODY.md) and
[canonical verification](../plans/workspace-views/TODO.md). The older
persistent-broker limitation below records its pre-migration checkpoint.


## Current TUI integration amendment

The current inline/fullscreen TUI is compile/test/PTY-smoke validated; the older “authored source only” wording below is historical. Embedded PTYs still own ordinary terminal keys. The Conversation-only **F6/F7** thinking/tool visibility toggles are intentionally handled only when a Conversation is focused and therefore are **not intercepted from a focused child terminal**. Tool-result coloring described in the current TUI is aw-owned transcript chrome, not passthrough of child/model escape sequences.

Agent **Scrollback** uses one persistent aw presenter with a dynamically sized Ratatui `Viewport::Inline` scratch terminal plus `insert_before`, so settled prompt/model/tool output enters true terminal scrollback exactly once above transient editing chrome. Agent **Inline** also uses Ratatui `Viewport::Inline` on normal CPR-capable terminals, but with a different policy: loaded historical rows may leave the active viewport without being re-emitted, and only newly settled runtime turns evicted after actual physical saturation are committed with `insert_before`; live/composer/status rows never become settled history. A CPR-less compatibility path uses a bounded fixed-bottom viewport and deliberately never advances overflow history as if a commit succeeded. Agent **Fullscreen** owns the alternate screen. Workspace separately uses the alternate screen and may host retained PTY resources inside tabs. None of these presentations forwards model/tool ANSI as host actions, and hiding/collapsing tool display does not stop or replay a tool.

Post-Phase-58 resize/exit hardening clears the old physical Scrollback scratch footprint with `Clear(FromCursorDown)` before rebuilding its Inline viewport, because Ratatui 0.30 has no public mutable Inline-height/anchor API. Owned Inline dynamically recreates its Inline viewport only when required geometry/physical size changes; after real saturation it remains bottom-anchored and can commit only settled runtime-turn overflow. Both presenters publish/retain exact physical boundaries so the shared `TerminalSession` restores the invoking shell adjacent to aw content rather than at the terminal bottom. `TerminalSession` also owns raw mode, cursor visibility, bracketed paste, alternate-screen state, signal/panic restoration and opportunistic keyboard-enhancement push/pop. On a direct known-capable terminal, Kitty-style modifier reporting may be enabled directly. Once tmux is the immediate terminal peer, only tmux's positive capability report is authoritative; inherited outer-terminal markers such as `KITTY_WINDOW_ID` or `WEZTERM_PANE` never bypass an unsupported tmux transport.

**Historical Phase 5 note (superseded):** `aw-terminal` owns a real PTY,
child process and bounded screen; it is not a coloured log view.
Current compiled Rust/PTY host acceptance is recorded above.

## Persistent broker-owned terminal — Unix, 0.15.0

For a new terminal that should survive closing the launching CLI or a
Workspace frontend, explicitly run:

- `aw workspace persistent start /absolute/root /absolute/path/to/bash`
- `aw workspace attach /tmp/aw-pty-<nonce>/invite` (use the printed path;
  it follows `TMPDIR` if configured)
- **F12** to detach; subsequent attaches reuse the **same Bash PID**.
- `aw workspace persistent stop /tmp/aw-pty-<nonce>/invite` after
  detaching to terminate the shell and remove its private endpoint.

This new broker owns one **separately launched** PTY. It runs detached
from the initiating CLI in a new Unix session with null stdio and
whitelisted HOME/PATH/locale/TMPDIR only. No provider/Agent/Store
environment, model/tool admission or SQLite/JSONL run is created.
Invitation tokens are one-use, 90-second, private (0700 directory,
0600 file) and rotated after detach or receiver loss. Same-UID clients
still require the current token; old/racing controllers fail closed.
The owner terminates after 24 hours *idle between attachments*; an
active session is not cut off by this timer. Child exit stops the broker,
while lost broker ownership causes clients to exit without replay.

**Verified:** signed `bd87e5d`, real Linux `tests/native_persistent_pty.py`
scenarios `reconnect`, `competition`, `workspace-exit`, `shell-exit` and
`broker-loss`: same original Bash PID across three frontends,
independent Workspace exit, token rotation, no competing controller,
credential filtering, private TMPDIR, F12/termios restoration, explicit
shutdown and endpoint cleanup. Full split Rust, Node, web-only,
feature-boundary, formatting and scoped Clippy gates passed; see
[Workspace plan](../plans/workspace-views/TODO.md).

**Limit:** Already-running Workspace-owned PTYs are **not transferable**
to the new broker. They still require the original Workspace owner.
Linux-only, non-user-facing checkpoint/escrow primitives (`207c552`)
can stage parser state and actual master/pidfd while the original
reader is paused; this is **not** a committed child-custody handoff,
and there is no `terminal-promote` command.
No shell survives broker SIGKILL or OS reboot by promise, and
remote/multi-user control and Windows/macOS host acceptance remain open.

## Native second-terminal docking — Unix, 0.15.0

Focus an existing running native PTY and invoke **`terminal-dock`** in
the Workspace command palette. Copy the displayed command to a
**second local terminal**:

`aw workspace attach /tmp/aw-pty-<nonce>/invite`

F12 detaches the second frontend. The first aw process **must remain
alive**; this is not a background daemon or a second Agent.

- **One original shell:** Same PTY, Bash/process PID, Pool, Actor, terminal
  output parser and OS execution authority. Original view remains visible
  but its input/paste/resize/termination are fenced while docked.
- **One use:** 90-second invitation, 0700 private directory, 0600
  invitation file and local Unix socket. No persistent token, root
  capability, provider credential, browser transport, model call or saved
  layout migration.
- **Safe control return:** Monotonic, non-cloneable lease; queued input
  drains before the original gains control. Only the original owner
  acknowledges successful F12 detach. An unknown input/detach response
  is never replayed or called successful.
- **Failure:** Receiver crash returns to original owner. Original-owner
  loss makes the receiver fail closed with no shell revival. On SIGKILL,
  inert invitation/socket entries may remain in /tmp until cleanup.
- **Bounds:** Existing four-terminal cap, VT100 2–120 × 4–300,
  bounded input/paste and socket frames, no read of another Session,
  File root or Git checkout through this feature.

**Verified on signed `c98987a`:** Real independent Linux PTY sessions
normal detach, same shell PID, 65×20 and 120×40 receiver resizes, Ctrl+C
ownership, unmodified source shell during remote control, F12 and
owner `quit-all` restoration, receiver crash and owner loss; no
model/tool run or effect records (the shell modified only private fixtures). Native wire, lease, expiry and race tests
passed. Full Rust, Node, formatting, scoped Clippy and minimal/Workspace
feature checks passed as recorded in
[Workspace plan](../plans/workspace-views/TODO.md).

**Not yet supplied:** owner-independent durable PTY survival, cross-device
or different-Actor control, Windows-native implementation or host-run
macOS acceptance. Windows target compilation was blocked by missing
target std/core in the connector, not demonstrated successful.

## Ownership and explicit launch

The native workspace retains a `Pool` in its existing application Host. `terminal-start` is a deliberate request to launch the configured absolute shell. No shell starts on application launch, a layout restore, opening Git, or merely asking for terminal help. Model tools may remain off while a human deliberately starts a terminal: these are different authorities.

Default launch uses the selected Git review root if one exists, otherwise the original agent root. It labels that creation root and actor. Selecting another worktree afterward does not change the PTY; the shell can change its own directory normally. The service does not infer a new agent context from shell output or escape sequences.

Up to four retained resources exist, independent of tab views. `close-tab` hides a view; `terminal-open ID_OR_NUMBER` reattaches. `terminal-stop` stops the selected resource; `terminal-forget` removes an exited one. A single locally focused view owns keyboard input; one visible view (preferably the focused duplicate) supplies size, avoiding repeated conflicting resizes from duplicate tabs.

Leaving workspace for the same inline Host retains live PTYs and output draining. Re-entering can reopen them; switching presentation does not rerun commands. Application quit asks for explicit terminal termination; unsent drafts are separately protected. Restart cannot restore a dead process from SQLite or a saved view ID. The separate web-owner launch has no terminal transport added in this phase.

## Public Rust API

```rust,no_run
use aw_terminal::{Options, Terminal};
fn launch() -> aw_terminal::Result<Terminal> {
    let options = Options::bash("/project", "/absolute/trusted/bash", "verified-local-actor");
    let terminal = Terminal::spawn(options, || {
        // Notify a coalescing host observer; do not block this callback.
    })?;
    Ok(terminal)
}
```

Spawn on a blocking worker. `Terminal::input`, `resize`, `terminate`, `status`, `input_modes` and `snapshot` are available to a trusted host. `Arc<Terminal>` duplicates a view reference, not a process. `Options` is trusted native launch input—not a deserializable web command or a permission grant. The [standalone consumer](../../examples/sdk-terminal/src/main.rs) requires `--run-fixture` before it launches its clearly labelled test process.

The service depends on portable-pty0.9.0 and vt1000.16.2 behind its own API. It does not depend on Agent, SQLite, Tokio, HTTP, Ratatui or crossterm. The frontend maps its own keyboard events and cell rendering to the service's simple types.

## Input, rendering and control

A nonblocking reactor per explicitly launched terminal polls the PTY and a control pipe, drains output while hidden, incrementally updates the parser, writes bounded queued input, resizes the child and reaps its exit status. A250ms reap interval is used only while that resource exists; input/control wakes it immediately. Dirty events are coalesced by the frontend; no fixed full-screen idle animation is required.

The displayed viewport comes from terminal cells and styles, not raw escape sequences written to aw's own terminal. UTF-8 text, basic colour/attributes, cursor state, alternate screen, application cursor mode, standard control/function keys and bracketed paste are represented. The current subset does not claim full kitty/graphics/extended-keyboard/mouse/clipboard/hyperlink/OSC shell-integration compatibility. Very long combining sequences inherit the parser's cell limits.

OSC/DCS/APC/PM payloads are discarded by a bounded filter. A small allowlist of device/cursor report replies serves ordinary terminal clients. No child title/clipboard/image/window request becomes an aw command, host clipboard operation, provider credential read or UI action. Unhandled parser callbacks do not implement arbitrary host effects. Display controls/bidi instructions are not emitted as physical-terminal actions.

The focused terminal receives ordinary Ctrl+C, Ctrl+W, Ctrl+\\, Tab, Escape, Enter, arrows and text; these remain child input rather than workspace/editor commands. The only native workspace chords intercepted before PTY passthrough are the small documented global set: `Ctrl+Shift+P`/`F1` for the command palette, `Ctrl+\`` to return to/focus workspace terminal context, `Ctrl+Tab` / `Ctrl+Shift+Tab` for content tabs, and the contextual Explorer/main focus family: `Alt+E`, `Alt+W`, and `Alt+1..9`. Number keys select Sessions/Files/Git while Explorer is focused or content tabs while the main group is focused. Other layout/agent actions are available from the palette instead of an obscure terminal prefix. Terminal PageUp/PageDown go to the child; use the palette's terminal scroll/live commands for aw's scrollback.

A paste is at most64 KiB, forbids embedded control characters other than line endings/tabs, and requires the child to enable bracketed paste for multiline content. Otherwise multiline paste is explicitly refused rather than executing accidental shell commands. Queued input is bounded to128 KiB; full queues reject with a visible result, not an implicit replay. Not every modifier/keyboard-protocol combination is supported; unsupported encodings are not treated as agent commands.

## Size, retention and shutdown

Screen bounds: 2–120 rows,4–300 columns; scrollback1000 rows per terminal. Tiny or hidden views retain the last valid PTY size; they never resize it to zero. Only visible view demand creates screen snapshots. A stored Terminal descriptor with no live resource shows unavailable and requires deliberate launch.

Termination first signals the unreaped shell/foreground process groups, escalates after a short interval and reports uncertain shutdown if necessary. Launch/worker failure uses a child guard to avoid orphaning a spawned child. Closing the controlling PTY is not a guarantee to stop disowned/escaped jobs or an entire arbitrary process tree. Power loss/SIGKILL and hostile native code remain outside local cleanup guarantees.

Input/output and terminal process effects are not durable model-tool journal entries. A terminal may modify files, run network commands or leave side effects after interruption. No transcript replay executes old terminal bytes. Human terminals can race with the agent/Git/external editors; they are not protected by the model's per-run mutation lock.

## Trust and credentials

This is **an unsandboxed shell with the native user's OS permissions**. Initial environment inherits only selected ordinary PATH/HOME/locale values; token variables, BASH_ENV/ENV, loader overrides and agent sockets are not inherited. Bash is launched without profile/rc scripts. Those choices reduce surprising startup behavior but do not confine same-user file/process/network access or prevent deliberately reading local credentials.

Keeping provider keys out of core descriptors and transport does not protect them from arbitrary same-UID shell code. Strong isolation requires an OS/container/runner boundary, not a terminal widget or LLM guardian. No remote browser or multiplayer viewer gains terminal authority through the new native service.

## Host acceptance

Run the actual [Rust PTY tests](../../crates/aw-terminal/tests/pty.rs), native rendering tests and compiled-aw Phase5 harness. Test the real desired applications (shell, less, vim/nvim, SSH/tmux paths) on the target machine before claiming compatibility. Python PTY probe exercised real controlling-TTY, input, resize, signals, hidden output and exit codes here, but not the Rust emulator or UI. See [build status](BUILD-STATUS.md).

## Phase 6 browser view

An explicitly enabled `aw web --web-workspace --allow-web-terminals` owner may launch this same native PTY resource after user confirmation. The browser renders safe serialized cells/styles, not raw escape streams. `aw-surfaces` adds one expiring client control lease per terminal and exact-last-input sequence receipts. Authentication remains the private paired owner; leases only coordinate its tabs. Launch/control receipts are not durable execution journals.

Browser view close/Chat only does not kill the process. Logout, session expiry and owner shutdown request cleanup. The browser polls only visible views and renews a held control lease; hidden PTYs still drain in their native service. Resize is explicit under control, not inferred from whichever view last repainted. [Complete browser behavior](BROWSER-WORKSPACE.md).
