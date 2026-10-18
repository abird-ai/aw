# Phase 27 — Domain authority standby failover and open AwNet coordination

Phase 27 turns the Phase 26 AuthoritySet/quorum primitive into a real **Domain authority-core standby/failover path** and adds reusable self-hosted/managed AwNet rendezvous coordination on the same open protocol.

It deliberately does **not** turn SQLite into a multi-master database, replicate arbitrary application rows, or make an AwNet coordinator into Domain policy authority.

## 1. Phase boundary

Phase 27 implements two independent planes:

1. **Domain authority-core replication/failover**
   - the current authority leader exports a bounded signed semantic snapshot;
   - an explicit AuthoritySet standby imports it as immutable evidence;
   - promotion requires a fresh signed majority term after the old leader lease is no longer valid;
   - promotion materializes only the minimal Domain authority core on the standby store.
2. **Reusable AwNet coordination**
   - `aw net serve --role rendezvous` is a reusable signed multi-room rendezvous registry;
   - the same implementation is appropriate for self-hosted or managed deployment;
   - it validates AwNode-signed reachability registrations but has no Domain policy/quorum authority.

The existing reusable relay remains the backward-compatible default `aw net serve` role.

## 2. Schema v12

Migration `012_domain_authority_replica.sql` adds:

### `domain_authority_replica_snapshots`

Immutable signed standby snapshots containing:

- Domain ID;
- source store and source AwNode;
- exact AuthoritySet revision/hash/quorum/current term;
- source commit-log cursor;
- canonical snapshot SHA-256;
- source signature;
- exact accepted AuthoritySet grant used for import.

A snapshot is immutable after import.

### `domain_authority_replica_heads`

A tiny local projection selecting the current standby/promoted snapshot for one foreign Domain:

```text
standby -> promoted
        -> superseded
```

### `domain_authority_replica_elections` + `domain_authority_replica_votes`

A fresh promotion election is separate from the source leader election. It pins:

- snapshot ID;
- AuthoritySet revision/hash/quorum;
- monotonically newer term;
- candidate AwNode;
- bounded lease TTL.

Remote votes reuse the Phase 26 signed `domain-authority-vote` transcript and MachineLink capability. The store independently re-verifies vote signatures.

### `domain_authority_promotions`

Immutable promotion provenance records:

- source snapshot/election;
- previous authority store;
- promoted authority store and AwNode;
- promoted term;
- bounded leader lease expiry.

## 3. Exact replicated authority core

A `DomainAuthorityControlSnapshot` contains only:

- Domain ID, parent ID, name, policy revision, security epoch and creation time;
- safe stable User metadata required by Domain memberships:
  - `user_id`;
  - realm identifier;
  - status;
  - display JSON;
  - user security epoch;
  - creation time;
- Domain membership role/status/revision/timestamps;
- exact AuthoritySet;
- source store/node and commit-log cursor;
- the current live source leader term/election/expiry for HA Domains.

It does **not** contain or materialize:

- `user_identities` / local Unix or OIDC login bindings;
- password/passkey/device credentials;
- auth sessions or bearer material;
- OAuth/provider secrets or provider accounts;
- Sessions, branches, nodes or model/tool receipts;
- Workspaces or Fleets;
- generic federation rows;
- SQLite pages, WAL records or arbitrary tables.

This is an authority-control semantic snapshot, not database replication.

## 4. Snapshot export/import authority

### Export

`export_domain_authority_snapshot` requires:

- current Domain owner/admin membership;
- current Phase 26 authority-write fence;
- for q>1, this exact AwNode must hold the current live quorum leader lease.

The snapshot is SHA-bound and signed by the source AwNode Ed25519 identity before transport.

### Import

The standby requires:

- trusted authenticated source peer;
- an already accepted exact Phase 26 AuthoritySet grant;
- local AwNode membership in that set;
- source AwNode membership in that set;
- matching snapshot SHA and AuthoritySet hash;
- a live source leader lease for HA snapshot import;
- valid source Ed25519 signature.

The MachineLink client verifies the source signature and `aw-store` verifies it again before durable import.

Repeated identical snapshots are idempotent. For one source store, the commit cursor must advance. If the source authority store changes after failover, the new snapshot is accepted only with a **strictly newer authority leader term**.

## 5. Quorum-fenced promotion

A standby promotion is never inferred from route failure.

The flow is:

1. import the latest signed authority snapshot;
2. begin a fresh local promotion election at a term greater than the replicated source term;
3. collect Phase 26 signed votes from AuthoritySet members;
4. require strict-majority quorum;
5. **refuse activation while the source leader lease recorded in the replicated snapshot remains live**;
6. after that lease expires, activate the fresh promotion quorum lease;
7. explicitly promote the standby authority core.

This gives the failover rule:

> no overlapping valid source leader lease + promoted leader lease.

Promotion also refuses to merge into a Domain that already exists locally. Generic authority-store merge is intentionally unsupported.

## 6. Promotion materialization

Successful promotion creates only:

- the Domain row with `authority_store_id` changed to this store;
- safe stable Users from the snapshot when absent and non-conflicting;
- Domain memberships;
- exact AuthoritySet membership/quorum;
- the fresh promotion election/votes as the canonical Phase 26 leader certificate;
- promotion provenance.

The promoted Domain advances:

- `policy_revision += 1`;
- `security_epoch += 1`.

This makes failover visible to later authority/lease checks and prevents stale pre-failover authority from being silently equivalent to the new epoch.

No Workspace is created as part of authority-core promotion, and no Session/conversation/provider state is copied.

## 7. Authentication boundary after failover

Stable User IDs and Domain memberships are replicated, but **authentication bindings are not**.

Therefore a promoted authority store does not automatically assume that a local Unix principal, email, display name, or unrelated local login corresponds to a replicated Domain User. That mapping belongs to explicit identity federation / managed identity authority work.

This is intentional: the design already forbids assuming `unix:1000` on one host is the same human as `unix:1000` on another host.

Node-level AuthoritySet/quorum authority can fail over now; human login/SSO continuity remains a later identity-plane boundary.

## 8. MachineLink services

Phase 27 adds a separate typed capability:

```text
domain-authority-replicate
```

It carries one signed authority-core snapshot over a MachineLink explicitly granting/requesting that capability.

Promotion votes reuse:

```text
domain-authority-vote
```

There is no new generic remote execution or database protocol.

CLI surfaces:

```text
aw net authority-replica-serve PEER AUTHORITY_DOMAIN ...
aw net authority-replica-sync PEER AUTHORITY_DOMAIN ...
aw net authority-replica-vote PEER ELECTION ...
```

Local standby/promotion inspection and control:

```text
aw domain authority-replica-show AUTHORITY_DOMAIN
aw domain authority-promotion-begin AUTHORITY_DOMAIN --ttl N
aw domain authority-promotion-show ELECTION
aw domain authority-promotion-activate ELECTION
aw domain authority-promote ELECTION
```

## 9. Open managed/self-hosted AwNet coordination

`aw net serve` now supports:

```text
aw net serve --role relay ...       # existing default
aw net serve --role rendezvous ...  # Phase 27 reusable coordination
```

The reusable rendezvous role:

- accepts bounded signed AwNode registrations;
- independently verifies node-ID/public-key/signature binding;
- stores only ephemeral room/reachability state;
- expires rooms;
- handles many sequential rooms during one service lifetime;
- returns the peer's signed registration and observed address;
- applies bounded room/packet budgets.

It does **not** load a `SessionStore`, inspect Domain membership, issue capability leases, vote in AuthoritySets, create federation policy, or decide promotion.

Managed AwNet can operate this same role globally. Self-hosters run the same `aw` binary/protocol. Hosted differentiation is deployment/HA/operations, not proprietary policy semantics.

## 10. Native validation

Phase 27 validation in this connector environment:

- focused schema/source/reference checks: **76 passed**;
- `aw-store` resource integration tests: **17/17 passed**;
- full workspace/default CLI compilation/tests: passed;
- default `aw-cli --all-targets`: **87 + 48 + 6 tests passed**;
- minimal/web/net configurations: passed;
- actual compiled-binary signed authority snapshot/failover smoke: passed;
- actual compiled-binary reusable multi-room rendezvous smoke: passed.

The authority smoke proves:

- q2 canonical source leadership;
- signed source snapshot replication;
- standby cannot activate while source leader lease is live;
- fresh q2 promotion votes survive until source expiry;
- activation succeeds only after source expiry;
- promoted authority store differs from source;
- promoted term is newer;
- policy/security epochs advance;
- no Workspace state is replicated by authority-core promotion.

The coordination smoke proves one reusable `aw net serve --role rendezvous` instance successfully coordinates multiple signed rooms while retaining no Domain-policy role.

The full-host clippy/Nix release gate remains environment-pending: this connector still lacks the `cargo-clippy` subcommand and `/nix/store` is read-only.

## 11. Next distributed consistency boundary

The remaining distributed write-authority design that most directly follows this phase is **Workspace mutable-ref writer epochs/CAS failover**:

- one logical writer epoch owns a shared Workspace/branch ref;
- immutable content can replicate independently;
- quorum-approved failover advances the writer epoch;
- a stale returning writer cannot advance refs under an old epoch;
- partitions preserve divergent immutable descendants rather than last-write-wins.

That should remain separate from trusted direct multi-process SQLite work. `owner.lock` still stays per local store until its dedicated WAL/lease/migration stress gates pass.
