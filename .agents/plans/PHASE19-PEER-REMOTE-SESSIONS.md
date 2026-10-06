# Phase 19 — durable PeerWorkspaces and RemoteSessionRefs

Phase 19 adds the first durable remote-Session creation operation on top of Phase 18's read-only MachineLink services.

## Implemented

- schema version 5 with source-side `remote_session_refs` and remote-side `remote_session_origins`;
- stable PeerWorkspace creation keyed by source store/Domain/Workspace identity rather than connection identity;
- exactly one remote Session for a source logical operation via `UNIQUE(source_store_id, source_operation_id)`;
- source-side immutable operation IDs and request digests;
- explicit pending → unknown → active RemoteSessionRef state, with no automatic replay after an uncertain dispatch;
- authenticated source AwNode binding to the negotiated peer identity;
- typed `remote-session` MachineLink capability only—no generic command RPC;
- `aw net remote-session-serve`, `remote-session-create`, and `remote-sessions`;
- validation-only post-create response drop to prove retry/idempotency behavior.

## Authority

The remote Session and transcript are canonical on the remote PeerWorkspace. The source stores only a RemoteSessionRef. Disconnect, response loss, and reconnect never authorize a second canonical transcript or an automatic retry of an unknown effect.

## Next

Phase 20 adds Fleet/FleetWorkspace fan-out planning and per-member durable execution records pinned to a fleet membership revision.
