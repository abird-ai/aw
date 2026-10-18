# Phase 21 — reusable self-hosted AwNet relay

Phase 21 promotes the Phase 16 one-pair relay primitive into a reusable bounded self-hosted UDP relay service.

## Implemented

- `aw net serve` for a reusable multi-room UDP relay;
- explicit service lifetime, per-room TTL, and max-room admission bounds;
- deterministic room pairing by the existing signed AwNet rendezvous registration;
- opaque end-to-end QUIC datagram forwarding after pairing;
- independent room expiry and source-route cleanup;
- service statistics for rooms created, pairs matched, forwarded packets, and expirations.

## Security boundary

The relay is connectivity infrastructure only. It does not call `trust_awnet_peer`, does not terminate application QUIC/TLS, does not receive Workspace/Session capabilities, and cannot mint Domain or execution authority. Full rooms are refused by bounded admission rather than expanding memory without limit.

## Next

Phase 22 makes Domain federation grants explicit durable capability leases with epochs, expiry, scope, and revocation.
