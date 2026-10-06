# Phase 26 — Domain AuthoritySet quorum fencing and signed leader leases

Phase 26 implements the first concrete high-availability control-plane primitive for Domains. It follows the established design rule:

> **Consistency for authority; availability for already-admitted bounded work.**

This phase does **not** make SQLite multi-writer and does **not** yet promote a remote store into a full canonical Domain-control replica. It adds durable AuthoritySet membership, strict-majority quorum voting, signed cross-node votes, short leader leases, and fail-closed fencing for authority-changing writes on the canonical Domain store.

## 1. AuthoritySet model

Every Domain now has a durable AuthoritySet:

```text
DomainAuthoritySet {
  domain_id,
  set_revision,
  members[],
  quorum,
  members_sha256,
  current_term
}
```

Existing and newly created Domains bootstrap as a one-node set:

```text
members = [local AwNode]
quorum = 1
```

That keeps ordinary personal/local use zero-coordination and backwards-compatible.

A configured HA AuthoritySet:

- contains 1..=32 unique AwNode IDs;
- requires a strict-majority quorum (`quorum > n/2`);
- requires real cryptographic AwNode identities for explicit HA configuration;
- requires every remote member to already be a trusted, active remote Machine in the same Domain;
- keeps the current canonical authority store's local AwNode in the set in Phase 26.

Changing the AuthoritySet increments its revision, recomputes its deterministic hash, increments the Domain policy/security epochs, supersedes old elections, and revokes federation leases tied to the old authority epoch.

## 2. What requires a live quorum lease

For `quorum > 1`, new authority/control-plane mutations fail closed unless the local canonical authority node has a live leader lease for the current AuthoritySet and term.

The Phase 26 fence is applied to existing owner/admin mutation paths including:

- child-Domain creation under an HA parent;
- Workspace creation/archive;
- explicit remote-Machine registration;
- Fleet creation/membership changes and FleetWorkspace creation;
- federation creation;
- federation lease issuance/revocation and federation revocation;
- semantic-materialization policy creation/revocation and fact materialization;
- AuthoritySet reconfiguration itself.

Read paths, immutable history inspection, and already-admitted bounded execution are not turned into quorum operations.

## 3. Election and leader lease

A local authority member may begin a bounded election with TTL 5..=300 seconds.

The election pins:

- Domain ID;
- AuthoritySet revision and member hash;
- quorum;
- monotonic term;
- candidate AwNode ID;
- requested TTL.

The candidate creates one durable local self-vote. Remote AuthoritySet members may add votes through the typed MachineLink vote service.

Activation requires at least the configured quorum of live votes from current members. The leader lease expiry is the **minimum expiry across the quorum votes**, so the candidate cannot outlive the authority granted by its voters.

Authority writes check the current AuthoritySet revision/hash/term and live candidate lease. If quorum disappears or the lease expires, new authority-changing writes stop.

## 4. Remote voter opt-in

A node does not become a foreign Domain voter merely because it is paired.

The canonical Domain authority exports an exact grant bundle containing:

- foreign Domain ID;
- AuthoritySet revision;
- quorum;
- ordered member set;
- AuthoritySet SHA-256.

A remote node must explicitly accept that bundle locally. Acceptance verifies:

- exact deterministic AuthoritySet hash;
- the grant source is a trusted AwNet peer and member of the set;
- the local AwNode is also a member of the offered set.

A different active grant for the same foreign Domain is not silently replaced; it must be explicitly revoked first.

## 5. Typed signed MachineLink vote

Phase 26 adds the explicit MachineLink capability:

```text
domain-authority-vote
```

The voter service authenticates the candidate as the MachineLink peer, checks the locally accepted foreign AuthoritySet, journals the vote, and signs a canonical Ed25519 transcript binding:

- Domain ID;
- set revision/hash/quorum;
- term;
- election ID;
- voter node;
- candidate node;
- requested TTL;
- exact vote expiry.

The candidate verifies the remote AwNode identity and signature before recording the vote. `aw-store` independently verifies the same signature again before accepting it into durable quorum state; durable authority does not depend solely on a transport caller claiming that verification already happened.

Votes are immutable.

## 6. Split-brain fencing

Strict-majority quorum plus durable voter rules fence overlapping leaders:

- a voter may cast only one identity for a given `(Domain, AuthoritySet revision, term, voter)`;
- an exact retry reuses the existing durable vote;
- stale terms are rejected;
- while a vote remains unexpired, that voter refuses a later overlapping election for a **different** candidate;
- same-candidate renewal can proceed into a higher term.

Because any two strict majorities intersect, two different candidates cannot both obtain overlapping valid quorum leases if voters obey these durable rules.

## 7. CAP behavior

Authority is CP. Already-admitted bounded work remains available.

Native validation explicitly proves:

1. a q2 Domain without a live leader lease rejects a new control-plane mutation;
2. a valid signed quorum activates a short leader lease and the mutation succeeds;
3. after that leader lease expires, a new control-plane mutation is rejected again;
4. a federation capability lease that was admitted while authority was valid continues to validate until its own independent expiry.

`validate_federation_lease` therefore deliberately does **not** require a currently live authority leader lease. Authority loss does not retroactively erase already-admitted bounded capability.

## 8. CLI

Local Domain authority controls:

```text
aw domain authority-show
aw domain authority-configure --member NODE ... --quorum N
aw domain authority-bundle
aw domain authority-accept SOURCE_NODE BUNDLE.json
aw domain authority-grants
aw domain authority-revoke-grant GRANT
aw domain authority-election-begin --ttl SECONDS
aw domain authority-election-show ELECTION
aw domain authority-election-activate ELECTION
```

Cross-node signed voting:

```text
aw net authority-vote-serve CANDIDATE_NODE --room ... --rendezvous ...
aw net authority-vote VOTER_NODE ELECTION --room ... --rendezvous ...
```

Pairing remains identity trust only; voter participation remains a separately explicit grant and MachineLink capability.

## 9. Validation

Phase 26 validation in the connector environment:

- focused schema/source/reference checks: **53 passed**;
- `aw-store` resource integration suite: **16/16 passed**;
- full workspace `cargo check --locked --workspace --all-targets`: passed;
- default-feature `aw-cli --all-targets`: **87 + 48 + 6 tests passed**;
- minimal/web/net configurations: passed;
- actual compiled-binary two-node AuthoritySet/MachineLink vote smoke: passed;
- the store suite proves the authority-CP / admitted-work-availability behavior across quorum expiry.

The complete source-bound reference profile is part of the final Phase 26 checkpoint gate.

The separate full-host release gate remains environment-pending: this connector has no `cargo-clippy` subcommand and `/nix/store` is read-only.

## 10. Deliberate boundary / next phase

Phase 26 provides real quorum **fencing** and cryptographically signed leadership on the canonical Domain authority store. It does **not** yet claim that another authority member possesses a complete replicated Domain control state and can automatically take over the canonical SQLite authority store after host loss.

The next phase should add authority-state replication/promotion and managed/self-hosted AwNet coordination on top of this primitive:

- replicate only the required Domain control-plane state semantically;
- preserve AuthoritySet revision/term and signed quorum proofs;
- promote a replica only after quorum fencing proves it is the current leader;
- never make the managed rendezvous/relay/coordinator itself implicit policy authority;
- continue using the same open protocol for self-hosted and managed deployments.
