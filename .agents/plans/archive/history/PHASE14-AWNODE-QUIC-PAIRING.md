# Phase 14 — stable AwNode identity and direct QUIC pairing

**Source 0.15.0 native-validated on 2026-09-30.** A genuine Cargo.lock was resolved; the full workspace and isolated net Rust tests ran; the actual-binary two-process QUIC pairing smoke passed; and the corrected Crane package built successfully as aw-0.15.0. Phase 14 implements the first AwNet data-plane slice on top of the Phase 13 Domain/Workspace/Machine/Fleet foundation. It establishes a stable cryptographic AwNode identity and persistent direct peer trust over certificate-pinned QUIC. It deliberately does **not** expose Workspace, Session, file, Git, PTY, model, tool, or execution RPC over the network yet.

## Implemented boundary

Phase 14 adds:

- optional `net` feature, enabled in the normal `aw` build;
- a persistent Ed25519 AwNode signing identity;
- a stable Node ID defined as `node_` + SHA-256 of the 32-byte Ed25519 public key;
- a SHA-256 key fingerprint bound into the existing local Machine record;
- a private `node-identity.json` stored beside the owned SQLite state, with Unix owner/mode/link checks matching the existing private-state threat model;
- a stable self-signed TLS certificate/key used only to bootstrap the temporary direct QUIC pairing listener;
- one-use, short-lived `awpair1_...` invitation capabilities;
- certificate pinning from the invitation before the application pairing exchange;
- signed request/response transcripts proving possession of both stable Ed25519 identities;
- persistent trusted peer rows in the existing schema4 `awnet_peers` table;
- store-level verification that a persisted peer Node ID hashes from its public key;
- refusal to silently rotate a local identity when the database says one existed but the private identity file is missing;
- refusal to silently re-key a Node ID or automatically re-trust a revoked peer;
- a bounded localhost actual-binary QUIC pairing smoke test in the native validation profile.

No schema migration is required: schema4 already contains `machines.key_fingerprint` and the `awnet_peers` identity/trust fields.

## Identity ownership

The Ed25519 secret seed and QUIC bootstrap private key are **not** SQLite rows, exports, peer records, Domain metadata, or Session history. They live only in the private local state file:

```text
$AW_STATE_DIR/
├── owner.lock
├── sessions.sqlite3
└── node-identity.json    # 0600, same UID, one regular-file link, no symlink
```

The stable AwNode identity is the Ed25519 public key. IP addresses, hostnames, UDP ports and the temporary TLS certificate route are not the durable trust identity.

`aw net status` is observational: if identity has never been initialized it reports pairing as available without creating a secret. The first explicit pairing operation creates and binds the identity. If the store is already bound to an identity but the private identity file disappears, networking fails closed and asks for the original identity to be restored rather than silently becoming another node.

## Direct pairing protocol

Listener side:

```bash
aw net pair
```

Optionally:

```bash
aw net pair \
  --listen 0.0.0.0:0 \
  --advertise 192.0.2.10:43117 \
  --name workstation-a \
  --timeout 300
```

The command prints a short-lived invitation then waits for one successful peer. Invalid attempts are bounded and the listener expires. A concrete `--advertise` address is useful when automatic LAN-route discovery is not the route the other peer should dial.

Joining side:

```bash
aw net pair 'awpair1_...'
```

The invitation contains the inviter's stable public identity, one-use random capability, expiry, direct endpoint, and exact temporary TLS certificate. The client pins that certificate before sending the pairing frame. The client then signs a transcript containing the capability, inviter Node ID, requester Node ID and fresh nonce. The listener verifies the Node ID/public-key relationship and signature before persisting trust. Its response is signed over both identities and both nonces; the requester verifies it before persisting the inviter.

The capability is sensitive until it expires or is consumed. It is trust-bootstrap material, not a permanent credential.

## Persistence and authority

Pairing creates **peer identity trust**, not Domain authorization. The peer row records identity and trust state. It does not grant Workspace discovery, Session reads, execution, provider credentials, filesystem access, or Domain membership.

This distinction is deliberate:

```text
paired AwNode identity
        !=
Domain/Workspace capability
        !=
remote execution authority
```

A later typed MachineLink layer must authenticate against this peer identity and then evaluate current Domain/Workspace policy before exposing any resource operation.

## Failure semantics

- Losing an IP/port does not change peer identity.
- Losing `node-identity.json` after binding does not authorize key rotation; networking fails closed.
- A Node ID already associated with another public key is rejected.
- A revoked peer is not automatically re-trusted by re-pairing.
- A client crash after the listener commits peer trust but before the signed response can leave one-sided trust. Re-pairing the same key is idempotent; no remote operation is authorized by this phase, so there is no hidden external execution effect to replay.
- Pairing retries are **not** Phase 12 model/tool retries and do not alter Session history.

## Explicit non-goals

Phase 14 does not implement:

- remote `--on MACHINE` execution;
- Workspace/Session/file/Git/PTY/model/tool RPC;
- rendezvous, STUN or NAT hole punching;
- relay or TCP/TLS fallback;
- `aw net join`, `leave`, `serve`, or `relay` runtime;
- Domain federation/capability leases over the network;
- direct multi-process SQLite writers;
- shared provider credentials.

`owner.lock` remains. Personal provider credentials remain with the executor that already owns them.

## Validation boundary

The source adds Rust tests for stable identity persistence, tamper rejection, store-level public-key/Node-ID binding, and localhost two-node QUIC pairing. The native gate also includes a two-process actual-binary `phase14_net_smoke.py` test.

Those tests are authored but **not executed here** because this environment still has no Cargo/rustc/Nix and no real `Cargo.lock`. Python/static/reference success cannot substitute for the native gate.

Before the next networking slice, a real host must:

1. resolve and retain a genuine `Cargo.lock`;
2. run formatter/compiler/clippy/docs for the whole workspace and the isolated `net` feature;
3. run the Rust unit/integration tests including the localhost QUIC test;
4. run the actual-binary Phase 14 two-process pairing smoke;
5. verify schema1→4 migration and all inherited Phase 12/13 durability/resource acceptance tests still pass.

Only then proceed to temporary rendezvous/STUN candidate discovery and direct QUIC hole punching, followed by relay/TLS-TCP fallback. Authenticated typed MachineLink/control and read-only Machine/Workspace services come after route establishment. Remote execution remains later and must continue using Phase 12 durable effect/retry semantics rather than transport-level replay.
