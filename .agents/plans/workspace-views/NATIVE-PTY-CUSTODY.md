Author: GPT-6 (Codex)
Owner: @pvl

# Linux original-PTY custody

Status: **implemented; real Workspace exit and crash tests passed**.
Current continuation of [migration prerequisites](LIVE-PTY-MIGRATION.md) and
[child lifecycle](ADOPTED-CHILD-LIFECYCLE.md). Exact commits and final gates:
[canonical TODO](TODO.md).

## Native operation

- Focus an existing locally controlled Workspace terminal; invoke
  `terminal-promote` in the palette. Cancel pending docking invitations first.
- The bounded background executor pauses input while custody settles. Success
  prints a shell-quoted `aw workspace attach INVITE` command. The old view is a
  final source snapshot; `terminal-forget` removes that released source resource
  without stopping the broker or creating a replacement shell.
- Attach from another native terminal; F12 detaches. Existing one-use tickets,
  controller generations, bounded input and 24-hour idle broker policy apply.
- After detaching, `aw workspace persistent stop INVITE` stops the exact shell.
  Success requires observed process exit. Disowned/escaped descendants may
  survive; shell exit is not whole-job termination.
- `aw workspace persistent inspect DIRECTORY` is read-only. It reports recorded
  intent/preparation/release/activation. An activation record never proves
  perpetual process liveness; connecting checks the current endpoint.

## Custody transaction

| Boundary | Original Workspace | Independent broker |
| --- | --- | --- |
| Local fence | Existing Control registry excludes docking, input, resize and stop | No resource |
| Quiesced | Sole reader pause acknowledged; earlier input/output/replies drained; ChildGuard armed | No resource |
| Prepared | Bounded checkpoint and one kernel-atomic master/pidfd escrow sent once | Replay and identity validation only; no PTY I/O, resize or signals |
| Release pending | Admission fences Stop/Drop/guard rollback; caller timeout cannot cancel | Inert escrow |
| Original released | Reader joined; every source PTY duplicate closed; guard converted to passive waiter | Still inert until verified signed publication |
| Broker active | Source never resumes; passive original-parent reaping continues while alive | Existing reactor owns exact original PTY/parser; existing Pool/Control admits one frontend |

- Same-UID SO_PEERCRED is checked at both FD-transfer ends. Source accepts only
  its exact spawned broker lifetime; broker accepts its original source peer.
- Preparation binds protocol version, random 256-bit transaction nonce, source
  and broker process IDs, original terminal ID/root/Actor/site, source epoch,
  kernel child PID, PTY-master number and SHA-256 checkpoint digest.
- Receiver verifies the transferred live pidfd's kernel metadata and master
  identity. No numeric-PID reopening/signaling fallback, replacement process,
  subreaper, ptrace or SIGCHLD policy is introduced.
- Source generates one ephemeral Ed25519 signing key in memory; only its public
  key enters the private transaction. The seed is explicitly zeroized. The
  original worker signs the domain-separated exact transaction record only
  after actual descriptor closure and ChildGuard disarm, then atomically
  publishes/syncs it in the private endpoint. ACK loss cannot prevent that
  worker from publishing. The private signing key is never persisted.
- Broker verifies this exact release signature before activation. FD receipt,
  readiness, elapsed time and source death grant no activation right. An old
  release or changed root/Actor/site/epoch/recipient/checkpoint fails admission.
- Dedicated broker has null stdio, its own OS session, and only whitelisted
  HOME/PATH/locale/resolved TMPDIR. It never constructs an Agent, opens State,
  admits a Tool, invokes a provider, restores a layout or retargets an Agent.
  Workspace selects the existing pinned signing dependency without enabling net.

## Failure and recovery

- Definite pre-release failure drops inert escrow and resumes the exact source
  reader. Source worker rejection may restore Retained; only that factual
  result can settle an abandoned registry fence back to local control.
- Pending/Released is irreversible to the caller. Lost/duplicate ACKs, caller
  destruction, old handles, source Stop and view closure cannot resume it.
- Source worker publication survives a missing caller. Lost transport after
  signed release is reconciled from the authoritative record; the receiver can
  activate once even after the original process exits.
- Death before publication is UNKNOWN, even if source I/O may have closed.
  Receiver never activates from death alone. Without a valid release within
  60 seconds, it closes inert escrow without sending signals and retains the
  private diagnostic record. Last-master closure can hang up an orphaned shell;
  this is not successful migration or authorization to recreate it.
- After validated preparation, errors retain transaction/release diagnosis.
  Cleanup follows an observed normal shell exit/explicit successful stop;
  wrong-recipient/reused endpoints cannot become generic directory cleaners.
- Broker loss never reactivates the source or replays input. Its record may
  remain stale. There is no broker-SIGKILL/reboot execution-restoration promise.
- Adopted lifecycle uses exact-pidfd TERM/KILL and non-consuming exit observation.
  Sibling brokerage does not confer parentage. Preserve unavailable exit status;
  never steal the original parent's wait receipt. Linux reparents on source exit.
- Transfer wire buffers are transient. A passive source waiter keeps a weak
  projection reference, so forgetting the released resource frees its parser
  and history while original-parent reaping continues independently.
- Reader teardown also closes reader/monitor handles when a silent descendant
  retains the slave; a full output channel cannot deadlock the join.

## Tests and verification

- [x] Platform + terminal real-process/PTY gates: wrong identity, inert refusal,
  guard disarm, passive reaping, lost caller/ACK, publisher panic, exact stop.
- [x] Streaming cutover: 3,000 lines written during source pause, each drained
  once after activation; original reader retired, inactive main and bounded
  scrollback retained. Held-slave shutdown regression fails without the fix.
- [x] `tests/native_live_migration.py parser|vim|less|source-crash`: real original
  Workspace Bash PID survives source exit/SIGKILL, alternate screen, Unicode,
  attributes/partial CSI, one new DSR response, exclusive input, foreground
  Ctrl+C, native detach/termios restoration and exact explicit stop.
- [x] `tests/native_migration_protocol.py`: 26 real-process scenarios covering
  wrong version/root/Actor/site/terminal/epoch/hash/master/pidfd/proof/peer,
  bounded/incomplete frames, absent/forged/stale release, lost/duplicate ACK,
  receiver loss and source death at intent/escrow/prepared/released boundaries.
- [x] Pinned split Rust: **1,731 passed, 4 existing ignored**; Node **174**,
  web-only Rust **139**; platform/terminal/CLI strict all-target Clippy,
  formatting, minimal/Workspace checks and `aw 0.15.0` build passed.
  Workspace retains two existing feature-only dead-code warnings.
- [x] Actual different-UID runtime FD admission rejection — `65f04ca`:
  mapped UIDs 0/1 in an isolated user namespace, both transfer directions,
  peer lookup plus single/pair send/receive. Protocol bytes remain untouched
  and original PTY/pidfd handles remain usable. No host privilege changes.
- [ ] macOS/Windows, remote/distinct-Actor custody, broker crash/reboot recovery
  and arbitrary terminal-application compatibility — separate unimplemented
  acceptance, not inferred from Linux fixture success.

## Different-UID runtime gate

Requires Linux user namespaces, `unshare`, subordinate UID/GID mappings and
`newuidmap`/`newgidmap`. The test refuses initial-namespace root. Only its
isolated child changes UID; no persistent account or host configuration changes.

```sh
nix --extra-experimental-features 'nix-command flakes' develop --no-update-lock-file -c \
  unshare --user --map-auto --map-root-user -- \
  cargo test --locked -p aw-platform-unix --test cross_uid_fd_handoff -- \
  --ignored --exact different_uid_is_rejected_before_descriptor_io --nocapture
```

This explicit host gate passed. Ordinary `cargo test` intentionally leaves it
ignored on runners lacking mappings; that skip does not certify the UID gate.
It tests the exact product FD admission helpers, not cross-user custody or an
Actor delegation grant.

AW 0.15.0; SQLite v33; JSONL v1; native layout v4 unchanged.
