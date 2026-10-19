Author: GPT-6 (Codex)
Owner: @pvl

# Adopted PTY child lifecycle

Status: **local source release, passive reaping and adopted reactor tested; production custody protocol pending**.
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

## Local source release — implemented Rust primitive

Only a trusted original host holding the non-cloneable `Staged` capability
can call `release_source`, after a successful kernel send. The caller must
independently authenticate recipient readiness and commit its owner intent
before calling. Broker integration is a separate checkpoint. This primitive does not
implement authentication, signed cross-process release or broker activation.

- Queue admission changes the exact source epoch from Retained to Pending.
  Source Stop/Drop and pause-guard rollback are fenced from then onward.
- `PendingSourceRelease::wait` has a five-second maximum. Timeout or lost
  ACK is UNKNOWN, not cancellation; inspect `Terminal::source_release`.
- Worker verifies the acknowledged pause, epoch, input drain and live child,
  retires and joins the only reader, then closes master/writer descriptors.
  Sender-held escrow duplicates close before admission.
- Only afterward, move the armed `ChildGuard` into a distinct passive
  `ChildWaiter`. The existing worker retains original-parent wait/reap
  responsibility with no signaling or additional reaper thread.
- `SourceReleaseReceipt` and `Status::Released` are factual local results
  after descriptor closure/disarm. Observer panic cannot undo release or
  strand the passive waiter. Receipt is not a transferable bearer grant.
- Linux uses a plain CLOEXEC master-FD writer. The pinned `portable-pty`
  writer injects newline + VEOF on Drop; that is forbidden during release.
  Other platforms retain their existing backend; release is Linux-only.

Definite pre-admission failure leaves original custody intact. Once admitted,
there is no automatic rollback on timeout or lost reply. A worker rejection
before disarm may return custody to the exact original epoch; a failed
reader join stays fenced/UNKNOWN with the armed guard's failure cleanup.

## Adopted reactor — implemented

- `Adoption::prepare` holds inert CLOEXEC master/pidfd escrow and replays
  the checkpoint without I/O or signals. Validate live kernel pidfd PID and
  PTY-master number against the original staged identity; reject mismatches.
- Trusted host activation requires independently authenticated source release.
  Reuse the existing reactor, filter, parser, bounded queue and snapshot API;
  no executable argument, replacement shell or extra execution owner.
- Retain exact terminal ID/root/owner/site. Continue the original checkpoint
  journal and pending sanitizer state; replay never emits historical replies.
- Adopted stop uses exact-pidfd TERM, then KILL after 500 ms. Never signal a
  numeric PID/group. Do not steal the original parent's wait receipt; preserve
  `ExitedUnknown` for nonchildren and factual signal status when available.
- `Status::exited` excludes Failed/Released. Uncertain teardown cannot count
  as successful process termination.
- Optional source release publication runs on the original worker after
  reader join, descriptor closure and guard disarm. ACK loss or publisher
  panic cannot restore custody or strand the passive parent waiter.

## Required production custody transaction — pending

1. Inert recipient validates exact source lifetime, terminal, root/site,
   Actor and epoch, pidfd identity, parser checkpoint and escrow. It may
   inspect lifecycle but cannot read, write, resize or signal the child.
2. Source keeps its armed `ChildGuard` and reversible reader pause until
   the authenticated release transaction commits. Receipt of FDs is not
   that commit. Pre-release cancellation closes recipient escrow and
   resumes the source; no shell/input recreation.
3. Reuse the local source primitive to prevent every old input,
   resize, protocol reply, termination and RAII reader-resume path. Join
   the paused reader and relinquish all source master/writer duplicates
   before publishing release authority. A timeout cannot revive it.
4. Use that primitive to disarm original **termination** responsibility
   only through that transition, preserving a non-signaling parent waiter
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

Authenticated production release-record ownership/publication, recipient
identity and crash recovery still need integration and real process tests. Source-side
passive waiting alone does not make the broker the shell's parent. No
public promotion command.

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
- `cargo test --locked -p aw-terminal --all-targets`: 20 unit, 17 PTY,
  5 replay/staging and 7 source-release tests passed; strict terminal
  all-targets Clippy passed in the pinned shell.
- Local source tests cover unsent/failed escrow refusal, pre-release Stop,
  passive parent reaping, lost ACK/guard-drop fencing, observer panic and
  no injected newline/EOF. Restoring the old portable writer reproduces a
  failing live-shell survival test; the plain FD writer passes it.
- An independent source using the real `Terminal`/`ChildGuard` exits after
  release; receiver retains the identical Bash PID, replayed alternate
  screen/Unicode/bracketed-paste modes and new explicit input/output. This
  is a synthetic native-owner test, **not** production Workspace/broker
  integration or complete crash/UNKNOWN acceptance.

References: Linux [waitid](https://man7.org/linux/man-pages/man2/waitid.2.html)
and [pidfd_open](https://man7.org/linux/man-pages/man2/pidfd_open.2.html).

- Adopted backend tests: inert drop and wrong PID/master identity preserve
  original custody; activated reactor retains exact ID, alternate screen,
  Unicode/input modes and live DSR replies; old reader stays retired;
  resize, exact stop, lost publication ACK and publisher panic are covered.
- Pinned platform + terminal all-target tests: **66 passed**. Both crates'
  strict all-target Clippy passed. This checkpoint does not certify the
  production broker protocol or Workspace command.
