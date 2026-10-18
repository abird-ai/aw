# Phase 22 — Domain federation capability leases

Phase 22 makes federation authority explicit and short-lived. A paired AwNode is still only an identity; a Domain federation and a current lease are required before federated actions are authorized.

## Implemented

- schema version 7 with `federation_capability_leases`;
- directional durable Domain federation creation against an already-trusted AwNet peer;
- explicit JSON resource scope and bounded action-list permissions;
- optional child-Domain inclusion and re-delegation flags, both false by default;
- short-lived leases (5–3600 seconds) that snapshot:
  - federation policy epoch,
  - Domain policy revision,
  - Domain security epoch,
  - peer security epoch,
  - scope and permissions;
- validation requires the authenticated peer Node ID, an unexpired active lease, current federation/Domain/peer epochs, and an explicitly permitted action;
- revoking a lease stops that lease;
- revoking a federation increments its policy epoch and revokes all active leases;
- `aw domain federate`, `federations`, `lease`, `lease-check`, `revoke-lease`, and `revoke-federation`.

## Authority

Effective authority remains an intersection. A lease cannot widen the Domain federation, Domain policy, peer trust, Workspace/resource policy, principal authority, or provider/tool policy. Federation is directional and non-transitive unless re-delegation is explicitly granted.

## Next

Phase 23 adds semantic cross-store synchronization of append-only commit/ref facts under a federation lease, without turning SQLite into a multi-writer replicated database.
