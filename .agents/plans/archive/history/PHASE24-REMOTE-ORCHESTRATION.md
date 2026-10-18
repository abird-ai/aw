# Phase 24 — persistent typed remote orchestration

**Checkpoint:** `9cddaeb / phase-24`
**Schema at the time:** v9
**Validation at the time:** focused Phase-24 checker **43**, actual-binary remote-control and Fleet-dispatch smokes green; full-host Clippy/Nix gate remained environment-pending.

## Goal

Phase 24 moved the previously read-oriented MachineLink/AwNet stack into **explicit durable remote execution orchestration** without turning AwNet into a generic shell/tunnel and without weakening the Phase-12 unknown-effect/no-replay model.

The two main product operations added were:

1. typed remote Session execution/control;
2. pinned Fleet dispatch over the already-durable Phase-20 Fleet operation snapshot.

## Durable remote Session execution

Phase 24 added the durable remote execution ledger and typed control path for a Session delegated to a remote AwNode/PeerWorkspace.

The operation surface includes:

- create/locate the remote Session under the intended PeerWorkspace;
- execute a prompt/work item against that exact remote Session;
- query execution status;
- read bounded transcript/result state;
- cancel a still-running remote execution;
- retain uncertainty durably when transport/process state cannot prove the remote effect stopped.

The implementation is capability-specific. Remote execution is not a generic arbitrary reverse shell.

Relevant source/checker anchors:

```text
crates/aw-store/migrations/009_remote_session_execution.sql
src/awnet/remote_control.rs
src/awnet/machinelink.rs
src/resources_cli.rs
the removed test_phase24_remote_orchestration helper
the removed phase24_remote_control_smoke helper
```

## No-replay / uncertainty rule

Phase 24 preserves the durable external-effect rule established in Phase 12:

> A lost connection or dead process does not prove a remote effect did not occur.

Before the remote Session effect can be entered, the durable local operation records the intent/identity needed to recover the attempt.

If completion cannot be proved, the operation becomes unknown/interrupted according to the durable state machine. It is not silently replayed merely because the transport reconnected.

## Fleet dispatch

Phase 24 added explicit Fleet dispatch over the pinned Phase-20 Fleet operation/member snapshot.

The dispatch path:

- consumes the exact persisted Fleet operation snapshot;
- does **not** re-resolve current Fleet membership during retry/recovery;
- derives a stable per-member remote operation identity;
- maps each member to the expected Machine/AwNode route;
- records unknown before crossing a remote Session effect boundary;
- stores member-specific result/unknown state durably;
- activates the corresponding `RemoteSessionRef` only through the typed operation.

Relevant source/checker anchors:

```text
src/awnet/fleet_dispatch.rs
crates/aw-store/src/resources.rs
the removed phase24_fleet_dispatch_smoke helper
the removed test_phase24_remote_orchestration helper
```

## Capability/authority separation

Phase 24 does not make pairing, relay or route reachability sufficient execution authority.

The operation still depends on the already-established stack:

```text
AwNode identity / pairing
  -> route
  -> MachineLink authenticated peer
  -> exact typed capability
  -> Domain/Workspace/Session authority
  -> durable remote execution operation
```

Transport reconnect is therefore not an authority upgrade and cannot authorize replay.

## What Phase 24 did not do

Phase 24 did not:

- replicate SQLite pages/WAL;
- introduce shared mutable cross-host database ownership;
- make FleetWorkspace a merged filesystem;
- make relay/rendezvous policy-authoritative;
- move personal provider credentials to the source host;
- replace typed MachineLink capabilities with a generic reverse tunnel;
- claim that process death means an effect is safe to retry.

## Validation evidence

The current repository still carries:

```text
the removed test_phase24_remote_orchestration helper
the removed phase24_remote_control_smoke helper
the removed phase24_fleet_dispatch_smoke helper
```

The focused source/schema/reference checker reports **43 checks**.

The original tag message records:

```text
phase-24
9cddaeb Complete Phase 24 remote orchestration
Phase 24 typed remote orchestration; native/reference validated,
host clippy/Nix gate pending environment
```

## Relationship to later phases

Phase 24 is the remote execution/fleet-dispatch foundation used by later phases:

- Phase 25: explicit semantic materialization;
- Phase 26–27: Domain authority quorum/failover;
- Phase 28–29: Workspace writer epochs and semantic handoff;
- Phase 30+: stable-user/auth continuity;
- Phase 34–35: local SQLite process/execution coordination.

Later authority/storage hardening does not change Phase 24’s central rule: **remote effects are typed, durable, and never silently replayed when outcome is unknown.**
