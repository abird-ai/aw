# Database/storage design refinement v2.1

**Status:** adopted design refinement on Phase 11 / source 0.12.0. This file updates design/navigation only; it does not claim that sync/auth/SecretStore/direct-multi-process storage is implemented or compiled.

**Provenance:** Revision 2 is part of the [accepted history](../plans/accepted-history/TODO.md). Git retains the original source handoff; this document preserves the applicable design contract.

## 1. What Revision 2 adds beyond the earlier database handoff

Revision 2 does not overturn the existing SQLite/immutable-tree design. It adds four durable planes that need to become first-class in aw:

1. **Semantic change feed and synchronization**
   - `commit_log` written atomically with application mutations;
   - lossy local wakeups + durable cursor replay;
   - store IDs, peer cursors, applied-event dedupe, snapshots;
   - semantic object/ref replication rather than raw WAL/row replication;
   - explicit same-branch divergence and writer epochs.

2. **Identity/authentication plane**
   - stable opaque `user_id` separate from Unix/internal/OIDC bindings;
   - aw-native passkey/device-key/password-verifier credentials;
   - OIDC issuer+subject linking and PKCE flow state;
   - organizations/workspaces/memberships/roles;
   - durable hashed/rotating login sessions and security epochs.

3. **Security audit plane**
   - append-only security/admin audit separate from compactable operational `commit_log`;
   - login/session/identity/policy/credential/migration/sync trust events;
   - no raw passwords/tokens/provider secrets in audit metadata.

4. **Credential-custody plane**
   - shared/server provider credentials represented in SQLite only by opaque `SecretStore` refs/version metadata;
   - personal provider credentials remain on the user's browser/device/native worker;
   - stable user-device/executor metadata and revocation;
   - no fallback from a failed personal account to a common account.

## 2. Adapted long-term architecture

```text
                         aw storage/security plane

                    ┌────────────────────────────┐
                    │ one SQLite/WAL store       │
                    │ initially backs most of:  │
                    └─────────────┬──────────────┘
                                  │
      ┌──────────────┬────────────┼───────────────┬──────────────┐
      │              │            │               │              │
 ContentStore     RefStore    PolicyStore    ReceiptStore    LeaseStore
 immutable facts  refs/CAS    ACL/releases   idempotency     effects/migration
      │              │            │               │              │
      ├──────────────┼────────────┼───────────────┼──────────────┤
      │              │            │               │              │
 IdentityStore  AuthSession   AuditStore      CommitLog       SyncStore
 users/bindings sessions       security log    semantic feed   peers/cursors

        SecretStore                                PresenceStore
  raw shared/OAuth/OIDC secrets              cursors/typing/draft CRDT
  outside ordinary SQLite plaintext          ephemeral/on-demand
```

These are logical interfaces. A single SQLite transaction may span several of them. Do not turn them into independent physical stores or independently committing services until measured deployment needs justify the atomicity cost.

## 3. Current aw mapped to the new model

### Already materially present in source

- immutable one-parent conversation nodes;
- branch head + revision references;
- cheap branch forks without transcript copy;
- durable run/model/tool receipts;
- conservative effect hazards;
- context summary artifacts/dependencies/release epochs;
- WAL, `synchronous=FULL`, foreign keys, trusted schema off;
- schema 1→2 transactional migration;
- WAL-aware backup;
- personal-device provider-key custody with explicit no-fallback semantics;
- named internal participants and per-user account selection;
- basic immutable audit rows for selected storage operations.

### Present only partially or in narrower form

- branch CAS exists in important paths but is not one universal ref mutation primitive;
- identity is represented by Unix/internal principal labels, not yet canonical opaque `user_id` + bindings;
- team invites/cookies are narrower than durable auth-session rotation/revocation;
- audit is narrower than the proposed security event domain;
- effect hazards do not yet provide multi-process generation-fenced leases;
- provider accounts/config do not yet use a general SecretStore metadata model;
- `aw-store` is modular but does not expose all logical store traits/transaction interfaces;
- current migration code is one controlled migration, not a rolling migration protocol.

### Entirely new runtime work

- semantic `commit_log` and subscriber cursors;
- store/replica IDs and cross-system semantic sync;
- sync snapshots and commit-log compaction;
- stable opaque users and external identity bindings;
- passkey/device-key/password-verifier native auth;
- OIDC provider and login-flow state;
- OAuth connector records separate from login identity;
- durable rotating auth sessions/token families;
- organizations/workspaces/roles/resource policies;
- dedicated SecretStore trait/backends for shared/server credentials;
- durable user-device registry;
- execution leases and client/process incarnations;
- migration lease/stage/cursor/capability negotiation;
- direct local multi-process DB mode;
- restore-generation and sync reconciliation;
- retention/tombstone/redaction administration.

## 4. Revised implementation order

The database roadmap remains intentionally staged. Each slice should be usable and testable without enabling unsafe later modes early.

### DB-A — compile/correct the existing product

Run the Phase 11 full gate on a capable host, retain the actual `Cargo.lock`, fix all compiler/test failures, and validate schema 2/backup/context behavior. No new direct-write claim before this.

### DB-B — immutable/ref correctness + semantic local feed

Implement together:

- one `RefStore` CAS primitive requiring expected head **and** expected revision;
- append-only `branch_ref_events`;
- completed model result independent of branch attachment (`Attached | Conflict | Unattached`);
- permanent `store_id`;
- bounded `commit_log` events committed in the same transaction as the state they describe;
- cursor API + local in-process/UDS wakeup hint;
- lost-wakeup recovery from `commit_log`.

Keep `owner.lock`.

### DB-C — transaction-aware logical stores + stable identity/audit foundation

Introduce the logical traits/unit-of-work boundary and stable opaque `user_id` while preserving current principal labels as bindings. Add the expanded append-only security `AuditStore`. Do not migrate every table at once merely for aesthetic purity.

### DB-D — native auth + SecretStore + provider/device metadata

Add:

- passkey/device-key credential interface, optional Argon2id verifier;
- durable auth sessions with hashed refresh tokens, rotation families and security epoch;
- `SecretStore` abstraction for server/common provider credentials;
- secret-free `provider_accounts` and `user_devices` metadata;
- explicit secret version/rotation audit;
- preserve personal-device/browser custody and no fallback.

### DB-E — OIDC/SSO + OAuth connections

Add OIDC authorization-code + PKCE login, issuer+subject identity binding, explicit audited account linking, and OAuth connector records/tokens via `SecretStore`. Never merge users solely by email.

### DB-F — multi-process effect + migration coordination

Add durable execution leases/generations, overlapping resource-domain rules, client incarnations, migration lease, stage/cursor ledger, capability negotiation and expand→backfill→activate→contract helpers. Recovery becomes lease/incarnation-aware.

Keep `owner.lock` until this slice passes adversarial tests.

### DB-G — trusted local direct DB mode

Only now permit supported direct local readers/writers. Use separate short reader/write connections, bounded `SQLITE_BUSY` retry for transaction acquisition only, CAS conflicts, lease conflicts and migration barriers as distinct typed outcomes. Stress WAL/checkpoints/backups. Owner/service mode remains available and preferred remotely.

### DB-H — authenticated semantic sync

Add peer identity/trust scope, per-source cursors, immutable-object fetch, remote-event dedupe, snapshots, restore generation and explicit shared-branch writer epochs/divergence handling. Never sync auth secrets or raw SecretStore material through generic content replication.

### DB-I — optional hosted scaling

Only from measurements: broker fanout, Postgres/ref+identity backend, immutable blob/object service, KMS/HSM/cloud SecretStore. Preserve the same logical semantics.

## 5. Non-negotiable data-class boundaries

| Data | Durable home | Generic content sync? | Raw secret? |
|---|---|---:|---:|
| Conversation/context objects | ContentStore | when authorized | no |
| Branch/policy transitions | Ref/Policy + commit log | when authorized | no |
| Operational notifications | CommitLog | yes, bounded/filtered | no |
| Security audit | AuditStore | only under explicit audit policy | no |
| Password verifiers/passkey state | Identity/Auth authority | no | sensitive verifier/public state |
| Login refresh tokens | AuthSessionStore | no | hash only |
| Shared provider/OAuth/OIDC secret | SecretStore | no | **yes, SecretStore only** |
| Personal provider secret | user browser/device/native worker | no | **never server DB** |
| Presence/cursors/shared draft transport | PresenceStore | realtime only | no |

## 6. Important implementation clarifications

- `commit_log` is not the security audit ledger.
- raw SQLite WAL is not an application sync protocol.
- a broker is an optional accelerator downstream of `commit_log`, never source of truth.
- stable `user_id` is not a login name and a device ID is not a user ID.
- OIDC login identity and OAuth delegated API access are separate records and trust decisions.
- direct DB access is for trusted local processes; remote clients stay behind an authenticated service/sync protocol.
- restoring an old DB must advance a restore generation / invalidate affected auth+sync state; never let stale refs appear newly authoritative.
- branch CAS and execution leases solve different problems.
- lease expiry does not prove an external effect stopped; unknown effects remain hazards unless fenced/reconciled.
- reader snapshot means pin immutable head/revision and materialize under a short SQL snapshot, not hold a transaction for the duration of a model run/UI session.
- application code should use a shared unit-of-work when one mutation touches multiple logical stores; logical modularity must not create a distributed transaction inside one SQLite file.

## 7. What this refinement does not implement

This commit changes design documentation only. It does not add schema 3, a commit log table, direct DB writers, OIDC endpoints, password/passkey handling, SecretStore, sync peers, a broker, Postgres, multi-process leases, or new provider credential storage. The Phase 11 compile/Crane gate remains outstanding.
