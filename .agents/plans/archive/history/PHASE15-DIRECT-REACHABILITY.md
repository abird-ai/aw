# Phase 15 — direct reachability, rendezvous and UDP hole punching

**Native-validated source checkpoint.** Phase 15 extends Phase 14 peer identity trust with ephemeral route discovery. It does not add Domain authority or remote execution.

## Implemented

- host UDP candidate discovery;
- RFC 5389 STUN Binding discovery for XOR-MAPPED-ADDRESS using the same UDP socket intended for QUIC;
- a bounded one-shot UDP rendezvous helper;
- signed peer rendezvous registrations, verified against already-persisted AwNode trust;
- rendezvous-observed server-reflexive peer addresses;
- symmetric UDP punch packets from the eventual QUIC socket;
- deterministic QUIC client/server role selection by stable Node ID;
- certificate pinning to the peer's signed ephemeral QUIC certificate;
- Ed25519 signed direct-route hello/ack transcripts;
- `aw net room`, `aw net candidates`, `aw net rendezvous`, and `aw net punch`;
- an actual-binary two-peer Phase 15 smoke.

The rendezvous helper is deliberately untrusted. It only reflects observed addresses and relays signed registration material. It cannot create peer trust, Domain membership, Workspace access, credentials or execution authority.

## Route identity

Stable peer identity remains the Phase 14 Ed25519 AwNode key. Host, STUN and rendezvous-observed addresses are ephemeral route candidates. QUIC certificate material is signed route/bootstrap material, not durable identity.

## Failure boundary

A failed rendezvous/STUN/punch attempt changes no Session target and performs no model/tool work. Reconnect or route changes never replay Phase 12 unknown operations.

## Next

Phase 16 adds route candidate racing/selection and bounded fallback transports. Typed MachineLink capability negotiation remains a later layer.
