# Live Workspace PTY → persistent broker

## Current Linux custody — `9d82f1a`

- **Implemented and native-tested:** `terminal-promote` moves the running
  Workspace shell into an independent broker. The exact shell PID and full
  bounded parser state survive original Workspace exit/SIGKILL; the broker
  becomes the sole reader/controller after authenticated source release.
- Source and recipient bind exact process lifetimes, terminal/root/Actor/site,
  epoch, kernel PTY/pidfd identity and checkpoint digest. The original worker
  signs release only after reader join, descriptor closure and ChildGuard
  disarm. Missing proof stays UNKNOWN; no input/shell replay or source resume.
- Real native parser/`vim`/`less`/source-crash flows and 26 adversarial process
  cases pass. Real different-UID FD admission rejection also passes in both
  directions in an isolated user namespace (`65f04ca`), without sudo or host
  privilege changes. Remote/distinct-Actor and non-Linux acceptance,
  broker-SIGKILL/reboot execution survival remain separate.
- Current protocol, limits and recovery: [Linux custody](NATIVE-PTY-CUSTODY.md).
  The prerequisite record below describes its named historical revisions;
  its earlier “unimplemented” notes are superseded for this Linux path.


Status: **FD+pidfd escrow and reversible reader gate verified; production migration NOT implemented**
Linux source: `2feaa78bb4927ed0a3e9d0658fd7eff2cdeb505a`, `a1100a9d344c5835eb73dedc673e55570ad17219`, `635e1ec6e6763caf438999b1551932b5e37bfa81`
Owner: @pvl (Workspace views)

## Decision

Maintain two supported behaviors without conflating them:

- `terminal-dock`: transfers *presentation/control* to another terminal while the original Workspace process retains the PTY.
- `workspace persistent start`: launches a *new* broker-owned PTY that survives its launcher and Workspace frontend.
- Migration of an **already-running** Workspace-owned PTY into a detached broker: **open**. Do not invent a shell, replay input, or close its original process as a substitute.

## Verified kernel prerequisite

Linux `aw-platform-unix::fd_transfer::{send_one,receive_one}` carries one descriptor; `{send_pair,receive_pair}` carries a **PTY master and child pidfd together in one `SCM_RIGHTS` message**. The wire version distinguishes one and two FDs; both peers verify `SO_PEERCRED` UID and a 256-bit correlation proof. Wrong proofs, wrong cardinality, missing or malformed control, and truncated messages fail closed, dropping received FDs. Both received FDs are `CLOEXEC`; incomplete sends/unknown replies never imply source release. Same UID plus ticket still does not grant Actor or tool authority.

Linux `pid_identity::{open,exited}` provides an owned kernel pidfd and nonblocking process-exit observation instead of a reusable numerical PID. This is a **stable identity primitive**, not an adopted-process owner, signal/reap protocol or `ChildGuard` disarm. The sender must independently prove that the process it opens is its exact still-live child.

`cargo test --locked -p aw-platform-unix --test live_pty_fd_handoff` exercises actual independent owner/recipient processes. An already-running `portable-pty` Bash survives the original process exit when the recipient holds the master FD, retaining the **same shell PID**. The new two-FD test also transfers its **stable pidfd** atomically, confirms the original child remains alive after the owner exits, and observes the child's subsequent exit through the pidfd. A rejected offer leaves the original shell accepting input. All test files and processes are synthetic/private; **none of these escrow APIs is exposed through the shipping AW CLI**.

`aw-terminal` confirms that `vt100::Screen::state_formatted` reconstructs the visible screen and bracketed-paste mode but **does not recreate alternate-screen ownership**. It cannot be used as a lossless migration checkpoint by itself. Scrollback and partial parser/input escape state need equally explicit treatment.

## Verified original-reader quiescence

`aw-terminal::Terminal::quiesce` is a Linux-only, internal Rust API.
It atomically refuses new original-owner input/resize, honors earlier
FIFO commands and waits for the **only existing reader** to pause. All
previously delivered VT bytes and replies finish processing before ACK.
The reader polls a duplicated CLOEXEC descriptor, not a potentially
recycled original FD.

Only one `Quiesced` guard is admitted. A timeout (at most five seconds),
cancellation or guard drop resumes the source. Stale-generation ACKs
cannot consume later receipts. The **same original PTY, parser and child**
remain owned by the source, and original-owner termination stays possible.
No FD or process capability moves to a broker.

Real PTY tests verify shell PID unchanged, held output read **after**
resume, 24 cancellation/retry generations, denied input/resize,
shutdown and original in-process alternate-screen/SGR/bracketed-paste
state. The child can still emit output into the kernel PTY buffer while
paused; this does **not** complete parser serialization, child effect
acknowledgment or migration between processes.

## Ownership invariants

1. **One reader:** exactly one process may consume output from the master FD at any instant. Duplicating the FD is not transferring the driver, its blocking reader thread, or VT100 parser.
2. **One writer:** input, resize, protocol replies and termination are serialized behind the existing monotonic terminal-control generation. Pending input may be accepted-but-unconfirmed; never duplicate or replay it.
3. **One process owner:** a Linux pidfd can now be transferred atomically with the PTY master, preserving a stable process identity. But the old `ChildGuard` still kills its child on drop. The broker needs an explicit adopt/terminate/reap policy, and the source must disarm `ChildGuard` only after proven owner release. Stable identity alone is **not** custodial transfer; do not trust a naked PID.
4. **Full screen state:** preserve foreground/alternate buffers, scrollback, cursor, Unicode, attributes, modes, pending VTE parser bytes, protocol responses and original terminal identity within a bounded checkpoint. A visible-only frame is insufficient.
5. **No authority expansion:** exact owner, Actor, source/site, terminal ID and migration epoch are revalidated; same-UID plus a random ticket are *necessary*, not sufficient for changing Agent/tool privileges. No provider, SQLite State or tool permission moves.
6. **Rollback:** before final release the original process owns the child. Destination must hold transferred FD inert (no reads/writes) until an unambiguous release receipt. A missing, delayed or duplicate receipt remains UNKNOWN and cannot automatically activate a second driver.
7. **No persistence fiction:** only broker-owned PTYs survive the Workspace frontend. Broker SIGKILL or OS reboot cannot revive process effects without a separately designed durable execution owner.

## Implementation gates

- [x] Add reversible Linux single-reader quiescence, deny input/resize, drain queued commands/output/replies before pause ACK and resume on guard drop, failure or timeout. Signed `635e1ec`. **Not yet wired** to the broker adoption or owner-release transaction.
- [ ] Define a bounded **complete parser checkpoint** with round-trip tests for alternate screen, preexisting main screen, scrollback, partial CSI/UTF-8, color and input modes. Reject unsupported state without discarding the original process.
- [x] Linux-owned `pidfd_open`/poll observation and **atomic two-FD `SCM_RIGHTS` master+pidfd escrow** verified in real two-process Bash tests. Neither is wired to the product migration path.
- [ ] Introduce a separately tested **adopted-process lifecycle** using transferred pidfd, signal/reap responsibility and `ChildGuard` disarm only after signed original-owner commit. Broker must reject wrong root/terminal/epoch/identity.
- [ ] Implement explicit `Prepared → Quiesced → FD Received → Original Released → Broker Active` protocol. The recipient holds the FD but **does not read** before source's release; source retains complete rollback until then. Expiring offers are one-use and scoped to one exact source lifetime.
- [ ] Integrate a deliberately invoked `terminal-promote` command **only after** complete safety gates; retain current running shell PID and terminal contents throughout; no fallback new shell.
- [ ] Real two-process Linux/PTy verification: `bash`, long output, `less`/`vim` alternate screen, Unicode, scrollback, resize, paste, Ctrl+C, in-flight writes, source crash at each admission boundary, receiver crash, lost/duplicate ACK, wrong UID/proof, stale generation, source exit after release, explicit stop and clean terminal restoration.
- [x] On signed `635e1ec`, strict `aw-platform-unix --all-targets`, `aw-terminal --all-targets` and `aw-cli --lib` Clippy and `cargo fmt --all -- --check` passed. `cargo test --workspace --locked --exclude aw-store -- --test-threads=4`: **1,338 passed, 4 ignored**; Store library **201**, five Store integrations **156**, Node **174**, web-only Rust **139**, Workspace/minimal feature builds and 8/8 byte-identical frontend assets passed. Split package Cargo coverage, **not** one uninterrupted workspace command.
- [ ] Non-Linux/macOS/Windows process-ownership and final live-migration acceptance remain separate; no migration-complete claim until all protocol barriers pass.

## Acceptance criterion

**No migration-complete claim** until a real child started by the legacy Workspace is observed running with the **identical PID** and parser-visible state *after that Workspace process exits*, the new broker is the only reader/controller, and ambiguous failures are fenced without command replay. The Linux descriptor test is a prerequisite, not this acceptance.
