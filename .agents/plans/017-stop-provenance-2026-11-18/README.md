# 0.17 stop provenance convergence

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-5.6 Sol
Owner: @pvl

Shared-first port of the finalized dotlink `release/0.17` stop/signal semantics
onto current AW + dotlink main.

Reference release lane refreshed at:

- `release/0.17 = 130bb60e12154715ab7a91c4641aff6290488e94`

This work is separate from the later generic mount/config-v13 port.

## Design

Keep three concepts separate:

1. **physical process exit**
   - `Code(i32)`
   - `Signal(i32)`
   - `Unknown`

2. **Job lifecycle**
   - cancelled / revoked / deadline / failed / completed live in Job status and
     stop reason, not inside the physical exit enum.

3. **dotlink stop provenance**
   - product-specific `stopped_by` is present only when dotlink can prove its
     containment signal was delivered to the waited command/tree.
   - stop lifecycle logging is emitted at the actual signal site and includes
     reason + outcome.

Do not infer a signal number merely because AW/dotlink requested termination.
Only the waited process' reaped OS status may supply `Signal(n)`.

## AW shared contract

`aw-tools::filesystem::runner::ProcessExit` is physical-only:

- `Code(i32)`
- `Signal(i32)`
- `Unknown`

The previous `Cancelled` / `DeadlineExceeded` variants duplicated lifecycle
classification and are removed.

AW adapter retains its private lifecycle outcome long enough to compute the
separate shared Job `Status`. A connector-requested cancel/deadline may
therefore settle as lifecycle `Fail` while its physical exit remains
`Unknown` when no reaped signal was captured.

Shared compact rendering uses `exit=signal:<n>`. This is presentation-only;
dotlink's JSON result projection later uses separate `signal` and
`stopped_by` fields.

## Dotlink next slice

After publishing the AW shared checkpoint:

1. repin dotlink to the exact AW commit;
2. map reaped launcher status to `SharedProcessExit::Signal(n)`;
3. remove main's legacy `stop_proven -> Cancelled/DeadlineExceeded` mapping;
4. port final release `KillReason`, `StopCause`, `StopOutcome`,
   `StopTrace` and Guard semantics;
5. emit one unconditional lifecycle line:
   `LOG job stop id=<id> reason=<reason> outcome=<delivered|gone|failed>`;
6. include Drop as `reason=dropped`;
7. carry `signal` and `stopped_by` through one renderer across synchronous
   result, Jobs status, internal-failure payload and failed native Task data;
8. external signals report `signal` with no `stopped_by`;
9. connector stops report `stopped_by` only when the signal was delivered;
10. use the release lane's final platform evidence/race fixes, including the
    actual deadline and sealed-writer teardown attribution.

Do not begin generic explicit mounts/config-v13 in this plan.
