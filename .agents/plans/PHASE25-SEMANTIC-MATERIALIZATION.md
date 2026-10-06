# Phase 25 — Explicit semantic materialization

Phase 25 closes the first safe projection boundary above Phase 23 semantic sync.

Semantic sync remains an **evidence transport**. Importing a `SemanticSyncBatch` only appends immutable source events to `semantic_sync_inbox` and advances the per-source cursor. It does not create or mutate local Sessions, branches, nodes, execution targets, tools, providers, Fleet membership, or other canonical resources.

Materialization is a separate local authority decision.

## 1. Contract

A remote semantic event may become a local materialized fact only after both of these explicit operations:

1. a current Domain owner/admin creates an exact materialization policy selecting:
   - federation ID;
   - source store ID;
   - event kind;
   - entity type;
   - entity ID;
2. a current Domain owner/admin explicitly materializes one chosen imported source sequence under that policy.

There is no wildcard policy in Phase 25 and no automatic inbox consumer.

The selected event must already exist in the immutable semantic inbox before a policy can be created. A policy is therefore authorization for a known class of already-observed evidence, not permission for an unseen peer to synthesize arbitrary future local state.

## 2. Current authority checks

Creating or using a materialization policy requires:

- current membership in the federation's local Domain;
- Domain `owner` or `admin` role;
- an active local Domain;
- an active Domain federation;
- current federation permission containing `sync.write`.

Policy listing and materialized-fact inspection require current Domain membership, but historical facts remain readable after policy/federation revocation. Revocation blocks new projection; it does not rewrite or delete immutable evidence already accepted while authorized.

## 3. Schema v10

Migration `010_semantic_materialization.sql` adds two tables.

### `semantic_materialization_policies`

An exact, revocable local authorization record containing:

- `policy_id`;
- `federation_id`;
- `local_domain_id`;
- `source_store_id`;
- exact `event_kind` / `entity_type` / `entity_id` selector;
- `active | revoked` state;
- monotonic policy revision;
- creating local user and timestamps.

Only one active policy may exist for an exact selector. Repeating creation of the same active selector is idempotent and returns the existing policy.

### `semantic_materialized_facts`

An immutable local projection containing:

- local Domain and federation IDs;
- exact source store and sequence;
- source event ID and SHA-256;
- source event kind, entity type, entity ID, actor, payload, and source timestamp;
- policy ID and policy revision that authorized projection;
- local materializing user and timestamp.

Both `(federation, source store, source sequence)` and `(federation, source store, source event ID)` are unique. UPDATE and DELETE triggers reject mutation of a materialized fact.

Before insertion, aw reconstructs the imported `CommitEvent` and recomputes its semantic hash. A mismatch fails closed.

## 4. What materialization means in Phase 25

A materialized fact is a **read-only local remote-fact resource**. It is deliberately not a Session, branch, node, branch ref, execution target, policy grant, Machine, Workspace, Fleet member, or provider/account record.

The operation emits a local `semantic_materialization.fact_created` commit-log event containing provenance metadata, but it does not copy the remote event into local canonical history and does not replay the source event's original mutation.

This preserves the central rule:

> replicate evidence and semantics deliberately; never turn semantic sync into generic SQLite multi-writer replication.

Future use of a materialized fact as model/context input must be another explicit operation with its own target authorization and provenance. Phase 25 does not auto-attach remote facts to a conversation.

## 5. CLI

The local Domain CLI exposes the boundary directly:

```text
aw domain materialization-policy-create FEDERATION SOURCE_STORE EVENT_KIND ENTITY_TYPE ENTITY_ID
aw domain materialization-policies FEDERATION
aw domain revoke-materialization-policy POLICY
aw domain materialize POLICY SOURCE_SEQ
aw domain materialized-facts FEDERATION SOURCE_STORE [--after N] [--limit N]
```

All commands use the ordinary local store/actor options and the existing Domain authorization model.

## 6. Failure and retry semantics

- Sync import alone never materializes.
- Policy creation is idempotent for the exact active selector.
- Materialization is idempotent for the exact source event identity.
- A different source identity cannot rebind an existing materialized fact.
- A revoked policy cannot be used for a new materialization call.
- Revoking the federation prevents new materialization under it.
- Existing inbox evidence and materialized facts remain inspectable by an authorized local Domain member.
- No source event is replayed as a local side effect.

## 7. Validation checkpoint

Phase 25 validation performed in the connector environment:

- focused Phase 25 schema/source/reference checks: **33 passed**;
- `aw-store` resource integration suite: **15/15 passed**;
- full workspace `cargo check --locked --workspace --all-targets`: passed;
- default-feature `aw-cli --all-targets`: **87 + 48 + 6 tests passed**;
- minimal/web/net feature configurations: passed;
- actual compiled-binary Phase 25 smoke: passed;
- full source-bound reference validation profile including the Phase 25 check: passed.

The binary smoke verifies that semantic import leaves the materialized-fact view empty until an explicit policy and materialize command are issued, then verifies exact provenance, idempotent retry, and revocation fencing.

The separate full-host release gate remains environment-pending: this connector has no `cargo-clippy` subcommand and its `/nix/store` is read-only. Do not infer clippy or Nix success from the green Phase 25 native/reference evidence.

## 8. Next boundary

Phase 25 does not introduce distributed write authority. A natural next distributed-control phase is Domain AuthoritySet/quorum/failover and managed AwNet coordination using the existing typed MachineLink/federation model.

A future context-projection phase may let a user explicitly attach selected materialized facts to a Session/context plan, but it must remain separately target-authorized and provenance-preserving. It must not make materialization itself mutate canonical conversation history.
