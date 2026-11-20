# AW + dotlink 0.17.x next-agent entrypoint — 2026-11-19

Use dotlink---l5 only.

Continue primarily in:

    /home/pvl/spaces/abird/src/dotlink

Cross-repo AW:

    /home/pvl/spaces/abird/src/aw

Read-only release reference:

    /home/pvl/spaces/abird/src/dotlink-017-mounts

Do not use A1. Do not create another worktree. Preserve all unrelated work.
Do not inspect or touch AW .agents/plans/ux/**. Do not implement the frozen
sandbox/mount/config-v13 plan unless the user explicitly promotes it.

Keep each implementation turn to roughly 30 minutes. Aim for the next coherent
checkpoint, update durable docs, then pause.

## Read first

1. /home/pvl/spaces/abird/src/aw/AGENTS.md
2. /home/pvl/spaces/abird/src/aw/.agents/docs/AW-DOTLINK-017-FULL-COMPREHENSIVE-HANDOFF-2026-11-19.md
3. /home/pvl/spaces/abird/src/aw/.agents/docs/CURRENT_HANDOFF.md
4. /home/pvl/spaces/abird/src/aw/.agents/plans/017-shared-runtime-convergence-2026-11-18/README.md
5. /home/pvl/spaces/abird/src/aw/.agents/plans/017-shared-runtime-convergence-2026-11-18/DESIGN.md
6. /home/pvl/spaces/abird/src/aw/.agents/plans/017-shared-runtime-convergence-2026-11-18/AUDIT.md
7. /home/pvl/spaces/abird/src/aw/.agents/plans/017-shared-runtime-convergence-2026-11-18/TODO.md
8. /home/pvl/spaces/abird/src/dotlink/AGENTS.md
9. /home/pvl/spaces/abird/src/dotlink/.agents/docs/CURRENT_HANDOFF.md
10. /home/pvl/spaces/abird/src/dotlink/.agents/plans/017-main-shared-consumer-backport-2026-11-18/README.md
11. /home/pvl/spaces/abird/src/dotlink/.agents/plans/017-main-shared-consumer-backport-2026-11-18/TODO.md
12. /home/pvl/spaces/abird/src/dotlink/.agents/plans/017-main-shared-consumer-backport-2026-11-18/D4-D7-SOURCE-CUTOVER-MAP.md
13. live source/tests for every owner you touch.

## Expected refs

AW:

    HEAD == origin/main
    4234447612421069a746c925c76a25e3ba27774a

Final AW implementation pin consumed by dotlink:

    1a5d0f65208e4a05b737c804a55a3aaad3e2fa59

dotlink:

    HEAD == origin/main
    4c3b96fd5ba9e6772bc159f10a3ad893c9729887

release:

    release/0.17 == origin/release/0.17 == v0.17.5
    3995448d7bf33794ef84c7bb51150d77cc260ada

All direct/transitive AW Git packages in dotlink must still resolve to
1a5d0f65..., with no path override and no registry package/version drift.

## Completed work — do not reopen without a real regression

D0 repin is complete.

D1-D3 are complete, signed, pushed and qualified:

- AW ManagedProcessGuard is the process-tree containment owner;
- ProcessExit is physical Code/Signal/Unknown only;
- StopReason/StopEvidence/StopRecord are canonical stop facts;
- StopTracker preserves reason through Unknown retry;
- old stop_proven/private Guard/synthetic Cancelled-or-Deadline ProcessExit is gone;
- TaskTerminalDetail preserves terminal facts behind later failures;
- one product mapper drives signal/stopped_by/prose;
- reasoned final StopRecord -> exactly one LOG job stop ...;
- outside physical Signal with no reasoned stop -> exactly one
  LOG job exit ... from=outside;
- the two lifecycle lines are mutually exclusive;
- real timeout/external-SIGKILL tests pass at quiet/default/verbose.

Latest dotlink qualification:

    main 701 passed / 0 failed / 1 intentional ignored
    integrations 42/42
    locked check green
    strict Clippy green
    rustfmt/diff green
    Markdown lint green
    NixOS module check green

## Remaining active implementation

Only D4-D7 remain:

    D4 typed JobRef + shared receipts + bounded diagnostics
    -> D5 RawPageBytes / SerializedResultBytes
    -> D6 aw-mcp-protocol result/receipt projection
    -> D7 shared progress heartbeat
    -> full v0.17.5 equivalence rerun

Start with D4 only.

## D4 exact scope

Read the full map first:

    dotlink/.agents/plans/017-main-shared-consumer-backport-2026-11-18/D4-D7-SOURCE-CUTOVER-MAP.md

D4:

- replace local StandaloneRef/raw flat ref parsing with
  aw_tools::execution_ref::JobRef;
- use typed JobRef as trusted finite identity;
- stop reparsing rendered receipt prose as semantic replay identity;
- use shared Receipt::JobStatus / NodeStatus / Handoff / Cancel / Ack for
  shared-backed Jobs;
- remove local ControlReceipt only after focused parity is green;
- keep the tiny product-specific jobs_send receipt;
- replace generic bounded_job_error with
  aw_tools::diagnostic::bounded_diagnostic;
- preserve outer transport/MCP error-envelope fitting as product-owned.

Required compatibility tests include:

    stdio_huge_job_ref_is_rejected_without_reflection
    local_http_huge_job_ref_is_rejected_without_reflection
    remote_start_duplicate_graph_replays_receipt_without_second_effect
    remote_cancel_replay_precedes_revision_recheck

Add/update focused tests for shared receipt grammar and typed ref construction.

Before editing:

    git status --short
    git diff --cached --name-status
    git rev-parse HEAD
    git rev-parse origin/main
    git diff --check

Expected:

    HEAD == origin/main == 4c3b96fd5ba9e6772bc159f10a3ad893c9729887
    clean working tree
    nothing staged

Do not start D5 until D4 is fully green and documented.

## Non-regression rules

Never:

- invent a signal from timeout/cancel;
- merge NoTarget with Unknown;
- derive physical exit from Job status;
- erase terminal facts after later infrastructure failure;
- restore a dotlink-private Guard or stop model;
- parse arbitrary rendered prose as trusted Job identity;
- let response-envelope fitting mutate retained raw offsets/EOF/ACK truth;
- convert typed/structured MCP facts to text;
- mint a new replay timestamp;
- let progress backpressure delay operation completion;
- pull the full aw-mcp client adapter into dotlink;
- implement sandbox/config-v13 without explicit promotion.

## External/deferred gates

Native Windows Job Object/exit-signal execution remains external because L5 has
no Windows Rust std target.

Sandbox/mount/config-v13 remains explicitly deferred.
