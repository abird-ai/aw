# Phase 16 — route selection, QUIC relay and TLS/TCP fallback

Phase 16 adds the fallback route ladder on top of Phase 15 reachability. Peer trust remains the stable Phase 14 Ed25519 identity.

## Implemented

- explicit route kinds and ordered attempt traces;
- route selection in direct QUIC -> QUIC relay -> TLS/TCP relay order;
- one-pair raw UDP relay that forwards end-to-end QUIC datagrams without terminating peer TLS;
- one-pair raw TCP relay that forwards opaque bytes while the peers establish end-to-end rustls;
- signed peer route registration remains the authority check; relay registration itself grants no trust;
- `aw net relay --transport udp|tcp`;
- `aw net connect` with direct/rendezvous, UDP-relay and TCP-relay options;
- actual-binary failure/fallback tests for both relay tiers.

## Authority boundary

Relays are connectivity infrastructure only. They do not create AwNode trust, Domain membership, Workspace access or execution authority. Route failure does not mutate a Session target or replay an unknown model/tool effect.

## Route migration

Route selection is connection-scoped and repeatable. A later persistent MachineLink can re-run selection after route loss while preserving the same peer identity and durable Session/effect state. This phase does not reinterpret a transport reconnect as permission to replay work.

## Next

Phase 17 adds authenticated typed MachineLink capability negotiation over an established route. Generic reverse shell/tunnel semantics remain prohibited.
