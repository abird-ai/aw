Author: GPT-6 (Codex)
Owner: @pvl

# Adopted PTY child lifecycle

Status: **exit observation/reap primitives tested; custody transfer pending**.
Continuation of [live migration](LIVE-PTY-MIGRATION.md).

## Kernel contract

- `SCM_RIGHTS` transfers the pidfd, not the parent/child relationship.
- The original Workspace remains the parent while alive. After its exit,
  Linux reparents the shell to the existing eligible subreaper or init;
  an independently launched sibling broker does not become its parent.
- Broker custody means exclusive I/O and exact-pidfd signaling, not a
  promise that the broker can recover a nonchild's wait status.
- Do not install a process-wide subreaper, change SIGCHLD disposition,
  ptrace/reparent the shell, or fall back to numeric-PID signaling/waiting.

`aw-platform-unix::pid_identity` exposes separate operations:

| Operation | Evidence / effect |
| --- | --- |
| `inspect_exit` | Nonblocking `waitid(P_PIDFD, WNOWAIT)` for a child. Preserves the parent's wait receipt. On ECHILD, poll the exact pidfd; ECHILD alone never proves exit. |
| `reap_child` | Explicit nonblocking exact-child reap. ECHILD remains an error for nonchildren/already reaped children; `None` means no waitable exit yet. |
| `signal` | Existing exact-pidfd TERM/KILL; successful delivery is not exit. |

`ExitObservation::Exited { status: None }` means exit observed but status
unavailable. Preserve that distinction from `ChildExit::Code(0)` and from
`ChildExit::Signal`. These operations grant no custody, Actor, root, Tool,
or PTY-reader permission. Unsupported wait APIs fail closed.

## Required custody transition — not implemented

1. Inert recipient validates exact source lifetime, terminal, root/site,
   Actor and epoch, pidfd identity, parser checkpoint and escrow. It may
   inspect lifecycle but cannot read, write, resize or signal the child.
2. Source keeps its armed `ChildGuard` and reversible reader pause until
   the authenticated release transaction commits. Receipt of FDs is not
   that commit. Pre-release cancellation closes recipient escrow and
   resumes the source; no shell/input recreation.
3. The source's irreversible transition must prevent every old input,
   resize, protocol reply, termination and RAII reader-resume path. Join
   the paused reader and relinquish all source master/writer duplicates
   before publishing release authority. A timeout cannot revive it.
4. Disarm the original guard's **termination** responsibility only through
   that transition, preserving a non-signaling original-parent waiter
   while the Workspace lives. Avoid blocking the UI on shell exit or
   leaking an unreaped child if the broker outlives its source.
5. The recipient activates once against the exact committed epoch. It
   uses pidfd exit observation and exact signals; it reaps only if Linux
   actually makes it the parent. Descendant cleanup and process-group
   identity need separate proof; shell exit is not whole-job termination.
6. Source crash before release does not authorize activation. After
   release, source never resumes; missing release/activation ACK remains
   UNKNOWN until the authoritative transition is inspected. Broker loss
   must not cause automatic shell/input replay or source reactivation.

Release-record ownership, publication ordering, retained source waiter,
wrong-pidfd admission, stop escalation and crash recovery still need their
own implementation and real process tests. No public promotion command.

## Tests and verification

- `cargo test --locked -p aw-platform-unix --all-targets`: 10 unit tests,
  4 integration tests passed in the pinned Nix shell.
- Direct synthetic children: live/nonblocking state, repeated non-consuming
  observation, exit 37, exact once-only reap, untouched sibling, factual
  SIGTERM, ordinary parent wait after inspection, invalid-file rejection.
- Transferred original Bash pidfd: receiver denied reap before source exit,
  denied reap after source exit, same shell PID continues, later exit
  observed with **no fabricated status**. Kernel fixture only; this does
  not exercise production `ChildGuard` disarm or broker activation.
- Strict platform all-targets Clippy passed. Broader candidate gate results
  are recorded in the [canonical TODO](TODO.md).

References: Linux [waitid](https://man7.org/linux/man-pages/man2/waitid.2.html)
and [pidfd_open](https://man7.org/linux/man-pages/man2/pidfd_open.2.html).
