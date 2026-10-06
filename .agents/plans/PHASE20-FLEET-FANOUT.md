# Phase 20 — Fleet fan-out operation snapshots

Phase 20 adds durable Fleet/FleetWorkspace fan-out planning without turning a Fleet into a hidden scheduler or replay engine.

## Implemented

- schema version 6 with `fleet_operations` and `fleet_operation_members`;
- immutable logical operation IDs and request digests;
- the exact Fleet `membership_revision` is pinned when the operation is created;
- active member machines are snapshotted once and ordered deterministically;
- `one` narrows to one deterministic member while `all`, `any_healthy`, and `quorum` retain the candidate set;
- exact retries return the original member snapshot even after Fleet membership changes;
- per-member pending/active/completed/failed/unknown/skipped state;
- optional RemoteSessionRef and result-digest binding per member;
- aggregate pending/running/partial/completed/unknown state derived from member records;
- `aw fleet plan`, `aw fleet runs`, and `aw fleet run-update`.

## Authority and replay

Fleet membership is not re-evaluated for an existing logical operation. A reconnect, retry, or membership change cannot silently fan the same operation out to a different machine set. Unknown member effects remain unknown until explicitly reconciled.

## Next

Phase 21 turns the bounded one-pair relay foundation into a reusable self-hosted AwNet relay service with bounded rooms, expiry, and no peer authority.
