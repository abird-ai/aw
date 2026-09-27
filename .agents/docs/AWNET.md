# AwNet — decentralized node connectivity with federated Domain control

**Status:** authoritative network/federation design through Phase 23. The current source implements stable AwNode identity/pairing, rendezvous/STUN direct reachability, QUIC and TLS/TCP relay fallbacks, typed MachineLink negotiation, capability-bound remote read services, durable PeerWorkspaces/RemoteSessionRefs, Fleet fan-out records, a reusable self-hosted UDP relay service, directional Domain federation leases, and append-only semantic cross-store sync. Phase 14–18 are native-validated; Phase 19–23 still require the final native Cargo/Nix gate.

## 1. Decision

Aw remains **one distributable Rust binary: `aw`**. Networking, agent endpoint, self-hosted coordination and relay roles are subcommands/modes of the same binary rather than separate products/daemons.

Canonical commands:

```text
aw [RUN OPTIONS] [PROMPT...]      local/immediate durable agent run
aw workspace open NAME            native/full Workspace UI
aw web                            browser Workspace presentation
aw agent                          long-lived AwNode/Machine runtime
aw attach                         local presentation attached to a running aw agent
aw device ...                     personal inference executor
aw net join ...                   join a managed/self-hosted AwNet
aw net pair ...                   persistent direct peer pairing without AwNet membership
aw net serve ...                  self-hosted AwNet coordination/control service
aw net relay ...                  optional self-hosted relay role
```

`aw --workspace NAME "prompt"` remains a run-selection flag for immediate work; it is not the Workspace UI command. Historical `aw serve` is superseded in the target CLI by `aw agent`. See [CLI](CLI.md).

Hosted deployments may run many `aw` instances/roles behind orchestration, but protocol semantics and the open-source self-host path stay the same.

## 2. AwNet is flat connectivity, not global ownership

An **AwNet** is an independently administered connectivity network of **AwNodes**.

```text
AwNet
├── AwNode A
├── AwNode B
├── AwNode C
└── AwNode D
```

An AwNode has a stable cryptographic Node ID/public key. Hostnames, IPs and connection routes are ephemeral metadata.

A Node may join several AwNets. There is no requirement for one planetary trust domain.

AwNet answers:

> Which AwNode am I talking to, and how can I establish an authenticated encrypted route to it?

Domains answer:

> What may this Principal/peer discover, read, execute, create, share or administer?

Workspaces answer:

> Where do these Sessions and their durable state belong?

## 3. Federated control plane, decentralized data plane

Do not require one global aw controller.

```text
               AwNet connectivity/data plane

     Node A  <===========>  Node B  <===========> Node C
       │                       │                     │
    Domains                 Domains               Domains
       \________________ federation _______________/
```

Each Domain has its own AuthoritySet. A personal Domain can use one node; a high-availability Domain can use a quorum of authority nodes.

Domain authority strongly orders security-sensitive control-plane changes: membership, policy, federation, provider grants, authority membership and revocation. Existing bounded capability leases may be cached and locally enforced until expiry.

This is intentionally different from a single centralized network controller.

## 4. Direct pairing

`aw net pair` creates persistent peer trust without requiring AwNet membership. Phase 14 implements the first direct form: a short-lived `awpair1_` capability carries the inviter identity, exact temporary TLS certificate and reachable endpoint; certificate-pinned QUIC plus signed Ed25519 request/response transcripts establish possession of both stable Node identities before either peer is trusted. See [accepted history](../plans/accepted-history/TODO.md). Pairing itself grants no Domain/Workspace capabilities.

Pairing establishes stable node identity first. Routes are discovered separately.

Strict offline/direct pairing may exchange QR/text bundles in both directions containing short-lived pairing material, public keys and candidate endpoints. No third-party service is required if the peers can actually reach one another.

Assisted pairing may use a temporary rendezvous/STUN service to exchange candidates. That does not create an AwNet or make the rendezvous service a Domain authority.

Peer trust survives IP/Wi-Fi/route changes.

## 5. QUIC-native transport

The core aw peer protocol is application-level and QUIC-native rather than a virtual Layer-3 VPN.

One authenticated connection multiplexes typed streams:

```text
control/capabilities
workspace RPC
session/event subscriptions
files/artifacts
Git/review
PTY
logs/metrics
preview forwarding
agent operations
```

Reliable QUIC streams are used for RPC/files/PTYS/session data. QUIC datagrams may be used for loss-tolerant presence, latency probes, cursor updates or wakeup hints. Durable truth remains in Workspace/Domain stores and semantic commit logs.

## 6. Route ladder

Preferred route order:

```text
1. same-LAN direct QUIC
2. public IPv6 direct QUIC
3. mapped/public IPv4 QUIC
4. NAT-hole-punched QUIC
5. private/self-hosted QUIC relay
6. Managed AwNet QUIC relay
7. TLS/TCP :443 relay fallback for UDP-blocked networks
8. offline
```

Direct paths are preferred. Relays forward end-to-end encrypted aw traffic and do not become Workspace/Domain authorities.

WireGuard is **not required for the core** and is not a useful fallback for UDP-blocked QUIC because WireGuard is also UDP-based. A later optional `Network Extension` may expose IP/subnet connectivity for legacy services, but Files/Git/PTY/Sessions/Agents use typed aw capabilities instead of broad network access.

## 7. Node-to-node cryptographic session

Bind peer identity above the physical route. A route may migrate from direct QUIC to relay or TCP fallback without changing who the authenticated AwNode is.

```text
AwNode identity / E2E session
            ↑
       aw typed protocol
            ↑
      transport adapter
       /      |       \
 direct    relay    TCP/TLS
```

The relay sees routing/traffic metadata required to operate the service, not plaintext Workspace/session/provider content.

## 8. Managed and self-hosted AwNet

There is one open protocol/control-plane implementation with deployment choices:

```text
AwNet protocol
├── Managed AwNet      operated globally by us
├── Self-hosted AwNet  `aw net serve`
└── Direct peers       no persistent coordination service
```

Do not create a separately branded Headscale-like compatibility project. The self-hosted service is simply the open-source AwNet control plane.

Managed AwNet provides operational value:

- globally available rendezvous/discovery;
- STUN/NAT traversal assistance;
- global QUIC/TCP relay edges;
- optional managed Domain authority/identity/policy hosting;
- HA, backups, monitoring, billing, support and admin UI.

The hosted control plane and relay plane are logically separable. A customer may self-host Domain/control authority while using managed global relays, or use managed control with private relays, or self-host everything.

## 9. Relay economics and openness

Most traffic should be direct, so managed bandwidth cost is near zero for direct peer paths and proportional to bytes only when relaying.

Self-hosters can run relay roles with the same `aw` binary. Later, explicitly trusted ordinary AwNodes may advertise relay capability under policy, allowing private/Domain relays before hosted fallback.

## 10. Domain federation over AwNet

A Domain federation relationship is not owned by a single tunnel. It is durable policy that may travel over any authenticated path between authorized AwNodes.

```text
Domain A AuthoritySet        Domain B AuthoritySet
        \                         /
         \____ DomainFederation _/
                 over AwNet
```

A physical tunnel/connection is an ephemeral route. Federation survives reconnect, failover and route changes.

Federation grants may expose an entire Domain or selected Workspaces/Machines/Fleets with action-level restrictions. Child Domains are explicit. Re-delegation is off by default.

## 11. Capability leases

Domain authorities may issue signed short-lived capability leases that nodes can enforce locally:

```text
CapabilityLease {
  domain_id
  principal_or_peer
  resource_scope
  actions
  policy_epoch
  issued_at
  expires_at
  signer_authority_epoch
}
```

This avoids a synchronous controller check for every operation. Revocation/policy change advances epochs; sensitive leases should be short-lived. Authority outages therefore degrade safely: existing unexpired authority can continue bounded work; authority-expanding operations fail closed.

## 12. Consistency model

Do **not** make all aw databases globally multi-master.

Use strong/ordered authority for:

- Domain policy/membership/identity changes;
- credential grants and billing authority;
- Workspace writer ownership/epoch;
- canonical mutable branch refs.

Use replication/eventual distribution for:

- immutable conversation/context/artifact objects;
- large object caches;
- semantic commit-log events;
- audit mirrors according to policy.

Use best effort for:

- presence/cursors;
- machine online status;
- latency/route hints.

Offline divergent work creates explicit branches/results rather than last-write-wins.

## 13. Workspace authority and failover

A Workspace may be single-node authoritative or backed by a replica set. Immutable objects can replicate broadly; one logical writer epoch owns mutable refs at a time.

If writer authority fails and quorum permits promotion, increment `writer_epoch`. A stale returning writer cannot advance refs under an old epoch.

FleetWorkspace authority remains on its configured Workspace authority set; fleet members do not become canonical replicas merely by executing tasks.

## 14. Failure domains

### Node failure
Other nodes/domains continue. Machine-target work becomes unavailable/unknown as appropriate. Unknown effects stay machine-scoped and require reconciliation.

### Route/tunnel failure
Peer trust, Workspaces and Sessions remain. Redial or choose another route; never infer that a provider/tool call did not occur.

### Rendezvous outage
Known peers/direct routes can continue. New discovery may wait. Rendezvous is convenience, not Domain authority.

### Domain authority minority failure
Quorum continues.

### Domain authority quorum loss
No new authority/policy mutation. Existing unexpired capability leases may continue bounded operations; sensitive operations eventually fail closed.

### Workspace authority loss
Reads may continue from replicas/caches. Writes resume only after valid writer-epoch failover; otherwise temporarily read-only.

### Network partition
Only the side with valid writer/authority can advance canonical shared refs. Other sides may create explicitly offline/local forks. Rejoin preserves both descendants and requires deliberate reconciliation/context import.

## 15. Availability principle

Use **consistency for authority, availability for already-admitted bounded work**.

Do not invent membership, permissions, branch ownership or credential grants when authority is unavailable. Already-admitted model computation/autonomous work may continue while its signed/delegated authority remains valid. Phase 12 durable operation/attempt semantics remain the one retry/recovery model.

## 16. One-binary rule

The public/local distribution remains one binary, `aw`. No `awnetd`, separate relay binary, separate remote agent binary or second LLM engine is required.

Roles are modes of the same program and reuse the same typed protocol/domain models. Hosted infrastructure may scale these roles independently operationally without changing the open protocol.
