# 0.17 stop provenance convergence

Owner: @pvl
Design: [README.md](README.md)
Release reference: dotlink `release/0.17 @ 130bb60`

## A. AW shared physical exit

- [x] Refresh release lane and final stop-provenance design.
- [x] Change shared `ProcessExit::Signal` to `Signal(i32)`.
- [x] Remove shared `ProcessExit::{Cancelled,DeadlineExceeded}`.
- [x] Keep Job lifecycle classification orthogonal to physical exit.
- [x] Carry Unix `ExitStatus::signal()` through AW adapter
      `Outcome::ExitedBySignal(i32)`.
- [x] AW-requested cancel/deadline remains lifecycle failure but exports physical
      `Unknown` unless a real reaped signal was observed.
- [x] Shared status/detail/receipt rendering preserves exact signal identity as
      `exit=signal:<n>`.
- [x] Existing cancellation/revocation Job status remains `cancelled` /
      `revoked`; old duplicated `exit=cancelled` becomes `exit=unknown`.
- [x] Real Unix regression distinguishes SIGKILL 9 from SIGSEGV 11.
- [x] AW termination regression proves requested cancellation does not invent a
      signal number.

## B. AW qualification

- [x] `aw-tools` full suite green.
- [x] `aw-tools-adapter` full suite **253/253** green.
- [x] strict all-target/all-feature Clippy green.
- [x] minimal/no-default `aw-tools` check green.
- [x] downstream `aw-mcp`, `aw-host`, `aw-cli --lib` checks green.
- [x] rustfmt + `git diff --check` green.

## C. Shared checkpoint

- [x] Review owned diff vs pre-existing dirty AW docs; only this plan/queue and
      the shared runner/adapter signal-identity files enter the checkpoint.
- [x] Create signed AW checkpoint.
- [ ] Publish only with explicit authorization.
- [ ] Repin dotlink to the exact published AW commit.

## D. Dotlink final stop provenance

- [ ] Map reaped launcher signal to `SharedProcessExit::Signal(n)`.
- [ ] Remove legacy shared `Cancelled` / `DeadlineExceeded` mapping.
- [ ] Port final `KillReason`, `StopCause`, `StopOutcome`, `StopTrace`.
- [ ] Guard returns a stop claim only on delivered signal.
- [ ] Stop lifecycle line survives quiet mode and records
      `id/reason/outcome` in fixed order.
- [ ] Drop path emits `reason=dropped`.
- [ ] Distinguish caller cancel from connector shutdown/restart/authority loss.
- [ ] Carry final release signal/stop evidence through synchronous result,
      Jobs status, internal failure and failed native Task data using one
      renderer.
- [ ] External signal => signal number + no `stopped_by`.
- [ ] Connector stop => `stopped_by` only when delivered; no fabricated signal.
- [ ] Preserve latest release platform-specific stop evidence and deadline race
      fixes through `130bb60`.
- [ ] Real subprocess tests: SIGKILL vs SIGSEGV; external kill vs connector stop;
      stop line present only when connector attempts stop, including Drop.
- [ ] Full dotlink binary/integration/Clippy/fmt/diff qualification.

## E. Deferred

- [ ] Generic explicit mounts/config v13.
- [ ] Any version/tag/release/deploy action.
