# aw database, synchronization, identity and credential-storage architecture

**Status:** adopted design amendment on top of Phase 11 / source 0.12.0. This document is architecture, not a claim that the new database/sync/auth features compile or run. Phase 11's full Rust/Nix gate remains the first implementation acceptance step.

**Source input:** [accepted history](../plans/accepted-history/TODO.md). The uploaded v2 handoff extends the earlier storage handoff with semantic subscriptions/sync, durable identity/authentication/OIDC/OAuth2/SSO, security audit and provider-credential custody. This document adapts those requirements to the current aw implementation rather than replacing the existing Phase 10/11 store contracts.

## 1. Adopted architecture in one sentence

aw remains **SQLite-first, immutable-history-first, queue-optional and single-binary by default**:

> immutable durable facts + tiny mutable refs/policies + separate effect coordination + semantic commit log + stable identity/auth plane + explicit credential custody.

One local SQLite/WAL database initially backs most durable domains. Conversation history, context artifacts, receipts and security-relevant facts remain immutable after commit. Branch heads, current policies, execution/migration leases and selected materialized projections remain small mutable records with append-only history beneath them. Raw shared secrets live behind a separate `SecretStore`; personal provider credentials remain on the user's own device/browser/native worker.

No external broker, Postgres service, distributed object store or collaboration daemon is required for ordinary local prompting.

## 2. What remains unchanged from the current implementation

The following Phase 10/11 decisions are preserved:

- one-parent immutable conversation ancestry;
- O(1)-style branch creation by adding a branch ref instead of copying history;
- explicit branch head/revision checks and stale-target rejection;
- model/tool work outside long SQLite write transactions;
- durable model/tool start/result receipts and conservative unknown-effect recovery;
- context imports/compactions as immutable derived artifacts with transitive source restrictions;
- current provider authority is resolved from current configuration/delegation, never restored from transcript history;
- personal-device inference keeps provider credentials local and fails rather than falling back to a shared account;
- remote clients use an application service rather than a network-mounted SQLite file;
- shared unsent drafts/presence remain separate mutable collaboration state, not sent conversation history;
- Phase 11 source-bound validation remains the required first gate before any direct multi-process mode is enabled.

## 3. The logical store model

Application code should move toward the following interfaces while one `SqliteStore` can implement most of them initially:

```text
ContentStore       immutable conversations, context objects and artifacts
RefStore           branch refs, CAS and writer epochs
PolicyStore        memberships, branch/session policies and release rules
IdentityStore      stable users, external identity links, organizations/workspaces
AuthSessionStore   login-session rotation/revocation
AuditStore         append-only security/admin audit
CommitLog          semantic application change feed
SyncStore          peers, cursors, snapshots and applied-event identities
ReceiptStore       idempotency, paid/effectful-operation recovery
LeaseStore         execution/resource and migration leases
MigrationStore     schema epoch, migration stage and backfill cursor
SecretStore        shared/server secret material behind opaque handles
PresenceStore      ephemeral cursor/typing/shared-draft state
```

These are **logical responsibilities, not separate transactions or mandatory physical databases**. Operations such as admitting a run may need one SQLite transaction spanning policy, receipt, lease, content/ref and commit-log writes. The abstraction must therefore support a shared transaction/unit-of-work boundary.

`SecretStore` remains a distinct security boundary even in a simple local deployment. `PresenceStore` is normally memory/on-demand collaboration state rather than durable conversation storage.

## 4. Durable data classes and sync policy

Classify data explicitly rather than treating every SQLite row as equally replicable:

| Class | Examples | Default sync/retention behavior |
|---|---|---|
| Immutable content | prompts, final attempts, context artifacts, run/tool/model facts | Sync only when policy permits; cache by opaque ID + digest. |
| Mutable refs/policy projections | branch heads, memberships, branch policy | Sync semantic transitions; current row is a projection over event history. |
| Operational change feed | `commit_log` | Replayable, cursor-based, eventually compactable after snapshots/retention. |
| Security audit | login, identity link, credential use, policy/admin changes | Longer retention and stricter access; not interchangeable with `commit_log`. |
| Identity-authority local | password verifiers, passkey state, refresh-token hashes, login flow state | Do not replicate through generic content sync. |
| SecretStore only | provider API keys, OAuth refresh/access tokens, confidential OIDC client secrets, signing/master keys | Never ordinary SQLite plaintext or ordinary event/audit payload. |
| Ephemeral | cursor/presence/typing/live draft transport state | Memory/on-demand service; not immutable history. |

## 5. Semantic commit log and live subscriptions

Add a durable semantic application outbox in SQLite. **Do not expose SQLite WAL as the aw event protocol.**

Every durable application mutation that clients need to observe writes one or more bounded semantic events in the same transaction as the state change:

```text
BEGIN IMMEDIATE
  append immutable fact / update tiny projection
  append commit_log event(s)
COMMIT
```

Conceptual event:

```text
CommitEvent {
  seq,                  // monotonic within one physical store
  event_id,             // globally unique opaque ID
  store_id, tx_id,
  kind,
  entity_type, entity_id,
  actor_id?,
  schema_version,
  created_at,
  payload               // small bounded semantic metadata only
}
```

Large conversation/artifact bodies are referenced by opaque object IDs/digests rather than copied into the feed.

### Local subscriptions

A local in-process/UDS/eventfd-style wakeup is only a hint. Subscribers retain `last_seen_seq` and recover from SQLite:

```sql
SELECT ... FROM commit_log
WHERE seq > ?
ORDER BY seq
LIMIT ?;
```

**Current cursor rule:** the physical store cursor advances over the bounded source rows that were scanned, while disclosure is filtered separately. A subscriber may therefore receive an empty event page whose through-sequence still advances across local-only or currently unauthorized rows. This prevents an actor/peer from becoming permanently stuck behind the same invisible commit-log page. Cursor advancement never grants disclosure: only events that pass the current Session/Domain authorization and semantic-sync scope checks are returned. Per-user Session-view events explicitly use local-only sync scope.

Losing a wakeup is harmless. Occasional cursor checks recover missed notifications. This is the planned refresh mechanism for local TUI/web/agent readers once direct local multi-client mode exists.

### Cross-system synchronization

Cross-system sync replicates **aw semantics, not arbitrary rows and not raw WAL**:

1. authenticate the peer and negotiate store/schema/object/protocol capabilities;
2. request semantic events after a per-source-store cursor;
3. authorize each event/object class for that peer/tenant;
4. fetch missing immutable objects and verify canonical digests;
5. apply compatible ref/policy transitions transactionally;
6. record remote `event_id` and cursor in the same transaction;
7. retrying the same batch is idempotent.

Sequence numbers are local to a store. `store_id + seq` identifies ordering; `event_id` provides duplicate protection.

Same-branch distributed divergence preserves both immutable descendants. No last-write-wins. A live collaborative shared branch should have one logical writer-store epoch/lease; offline peers fork rather than advancing an authority they no longer hold.

Periodic semantic sync snapshots allow old peers to bootstrap without retaining the operational feed forever.

A future NATS/Redis/Kafka publisher may accelerate fanout, but only as a projection of `commit_log`. SQLite remains recoverable truth and there is no DB+broker dual-write requirement.

## 6. Stable user identity and authentication

The current `internal:<realm>:<id>` and local Unix principals are useful qualified names, but the long-term durable primary key should be a stable opaque aw `user_id`/principal ID. Human-readable login names, Unix bindings and external identities become authentication bindings to that stable user.

Conceptually:

```text
users(user_id, realm_id, status, display_metadata, security_epoch, ...)
user_identities(identity_id, user_id, kind, provider_id?, issuer?, subject?, normalized_login?, ...)
```

Rules:

- OIDC identity key is `(issuer, subject)`, never email alone;
- email/display name is mutable metadata/recovery data, not identity;
- local Unix identity remains a convenient binding qualified by the local machine/authority, not a universal user ID;
- unlinking/changing login credentials does not rewrite historical authorship;
- `security_epoch` invalidates sessions/caches after high-impact account changes.

### aw-native login

Preferred credential families:

1. passkey/WebAuthn public credentials where supported;
2. device-bound native client public keys for CLI/local clients;
3. optional password login using a memory-hard verifier such as Argon2id.

SQLite stores only verifier/public material. No plaintext password, recovery code or bearer token is stored. Recovery-code verifiers are consumed atomically.

### OIDC/SSO versus OAuth connections

Authentication and delegated third-party API authorization are separate domains.

OIDC login uses Authorization Code + PKCE and links by issuer+subject. Confidential OIDC client secrets are `SecretStore` references. Identity linking requires an already-authenticated/audited operation; equal emails do not auto-merge accounts.

OAuth2 connections for GitHub/Google Drive/etc. are separate connector records. Their access/refresh tokens live in `SecretStore`; they do not automatically become aw login credentials.

## 7. Organizations, memberships and policy

Add durable organization/workspace membership and role projections without rewriting immutable content:

```text
organizations
workspaces / realms
memberships(user_id, tenant_id, role_id, status, policy_revision, ...)
roles / role_bindings
resource_policies
branch_policies
policy_events
```

Authentication answers who the user is. Membership/branch/source policy answers what they may read/append/share/export/execute. Workspace membership does not imply access to a private branch. Summary/source release can remain narrower than workspace membership.

Policy/security epoch changes invalidate cached authorization and future sync/model-context disclosure.

## 8. Login sessions and revocation

Persist auth sessions as secret-free verifier state rather than reusable plaintext bearer tokens:

```text
auth_sessions(
  session_id, user_id, auth_method, credential_id?,
  refresh_token_hash, rotation_family_id,
  issued_at, expires_at, last_seen_at, revoked_at,
  security_epoch_at_issue, client_metadata
)
```

Use short-lived access cookies/tokens and rotating refresh capabilities. Reuse of an already rotated refresh token revokes the family. Suspension/security-epoch change invalidates outstanding sessions.

Auth sessions are neither provider credentials nor branch/execution leases and are not generically replicated to content peers.

## 9. Security audit is separate from operational events

Retain/expand existing store audit behavior into an explicit `AuditStore`, separate from `commit_log`.

Audit at least authentication outcomes, identity link/unlink, session issue/rotate/revoke, suspicious token reuse, membership/role/policy changes, branch ownership/transfer, source-release/declassification changes, export/import/backup administration, shared credential lifecycle/use, personal-device grant lifecycle/use, OAuth connection changes, migration events and sync-peer trust changes.

Audit metadata is bounded and scrubbed. Never write raw passwords, bearer tokens, provider keys, OAuth refresh tokens or OIDC client secrets to audit rows.

An optional hash chain can detect accidental/post-hoc modification but does not protect against a host attacker controlling both data and keys. External SIEM/append-only sinks can be layered later.

## 10. Provider credential custody

### Shared/common/server accounts

Store only secret-free account metadata in SQLite:

```text
provider_accounts {
  provider_account_id,
  tenant/workspace scope,
  provider kind,
  account_owner,
  payer,
  credential_custody,
  executor_id,
  allowed models/policies,
  status,
  secret_ref?, secret_version?
}
```

Raw shared credentials live in `SecretStore`: OS keyring, encrypted local secret file with separately protected key, KMS/HSM/envelope-encrypted service or hosted secret manager depending on deployment.

Database backup without the SecretStore produces **credential unavailable**, never fallback to another key. Rotation advances an opaque secret version; historical invocation provenance retains the nonsecret provider-account/version identity.

### Personal credentials

Preserve the existing Phase 8–11 rule: personal provider secrets remain on the user's own native/device/browser executor. The shared DB stores only user/account/executor/grant/status metadata.

Native worker/keychain is the preferred high-trust option. Browser-owned credentials may later use encrypted IndexedDB/non-exportable WebCrypto wrapping where feasible, but same-origin application code that can use/decrypt the credential remains part of that credential's trust boundary. **Do not describe browser storage as equivalent isolation to the native worker.**

Offline/revoked personal account → explicit failure. Never silently select a shared server account.

## 11. Device/executor identity

Add stable opaque device/executor records separately from user identity:

```text
user_devices(device_id, user_id, kind, public_key/fingerprint?, status,
             registered_at, last_seen_at, revoked_at, capabilities, security_epoch)
```

A device ID itself is not hardware attestation. Device keys/capabilities may authenticate delegation channels. Replacing/revoking a device does not rewrite historical authorship.

A device may be an inference executor without being a content-sync replica; a replica may sync content without holding personal provider secrets.

## 12. Branch/ref correctness and completed conflict results

Every branch mutation in the target direct-writer model uses expected head **and** expected revision CAS.

A paid model result must be durable independently of branch attachment:

```text
ExecutionOutcome = Completed | Failed | Interrupted
AttachmentOutcome = Attached | Conflict | Unattached
```

If a same-branch CAS loses, keep the completed result/receipt produced from its pinned head. Never silently rebase or discard paid work. The user can create/focus a branch from the original head, compare/summarize it, deliberately regenerate from the new head or leave the result unattached but auditable.

Add immutable `branch_ref_events` for create/advance/policy/transfer transitions. `branches.head` remains the fast current projection.

## 13. Execution leases remain separate from database locks

Before direct independent writers are enabled, replace process-only correctness assumptions with durable resource leases/hazards keyed by canonical effect domains:

```text
ExecutionLease {
  resource_key,
  holder_client_id,
  run_id,
  generation,
  expires_at,
  mode,
  state
}
```

Distinct worktrees may execute concurrently. Equal/ancestor/descendant write domains conflict according to explicit policy. Lease generation fences settlement. An expired lease is not proof that an external effect stopped; unresolved effects remain hazards until reconciled.

SQLite writer serialization is never the filesystem/external-effect lock.

## 14. Direct local multi-process mode

The current source still intentionally uses `owner.lock` and one in-process SQLite owner. **Do not delete that lock yet.**

Enable direct local database clients only after:

- every branch mutation uses explicit head+revision CAS;
- completed conflicting results survive unattached;
- branch-ref/commit-log events are durable;
- execution leases/hazards are multi-process safe;
- reader connections materialize immutable snapshots using short transactions;
- typed busy/CAS/lease/migration/auth errors exist;
- migration coordination is durable;
- global recovery only acts on abandoned client incarnations/leases;
- actual multi-process WAL stress tests pass.

Remote clients remain behind the application protocol. Do not use network-mounted SQLite as collaboration transport.

## 15. Migration protocol

The current schema 1→2 migration remains a valid historical migration, but future multi-client evolution requires a coordinated protocol:

- distinct database schema epoch, object schema versions, native/browser wire versions and migration recipe versions;
- client read/write capability negotiation;
- one temporary migration lease/generation;
- durable migration stage/cursor ledger;
- expand → bounded backfill → verify → activate → later contract;
- short writer quiescence only around incompatible DDL;
- old clients may remain read compatible while write incompatible;
- no auto-downgrade;
- an operation admitted in an older compatible epoch may settle through an explicit compatibility path after migration;
- an incompatible settlement preserves receipts/effects and requires reconciliation rather than repeating the provider/tool call.

Backfill data and cursor advance must commit atomically per batch or the recipe must otherwise prove replay/skip safety. The migration holder checks its generation inside every stage transaction.

Store APIs should return explicit states such as `Ready`, `NeedsMigration`, `ReadOnlyCompatible`, `TooNew`, or `CorruptOrUnknown`; only a `MigrationCoordinator` mutates schema.

## 16. Multi-tenant and backup semantics

Security-sensitive durable rows carry an unambiguous tenant/realm/workspace identity. Do not infer tenancy solely by mutable names.

One SQLite file can still back a local/team owner initially. Future scale may shard content or move ref/identity state to another transactional service, but only after measurement.

A full deployment backup can span different security domains:

1. content/ref/policy SQLite snapshot;
2. identity/auth store if split;
3. SecretStore backup/escrow according to policy;
4. encryption/KMS keys where intended;
5. external audit archive when configured.

Personal-device provider credentials are **not** part of server backup because the server never held them.

Restore verifies store ID/schema epoch, handles auth-session invalidation/rotation, validates SecretStore handles, re-handshakes sync cursors/peer epochs where needed and records an administrative audit event. A restore generation prevents old remote sequences from being mistaken as newly authoritative history.

## 17. Retention and deletion

Immutability does not mean every class is retained forever:

- credentials/tokens: revoke/delete promptly according to policy;
- user profile/contact data: tombstone/remove according to product/legal policy;
- authored immutable content: follow workspace retention; when deletion is required, use explicit tombstone/redaction semantics and rebuild affected indexes/sync snapshots rather than covertly changing hashes;
- security audit: retain minimum necessary identifiers according to policy;
- operational commit log: compact after permitted peers can bootstrap from snapshots;
- presence/draft state: short lived by default.

No local operation can recall content already delivered to an authorized device/vendor/export.

## 18. Implementation order adopted for aw

The new handoff contains broad future scope. Do **not** implement every table together. Current Phase 11 native validation remains first.

### DB-A — validate the existing implementation

Compile/correct Phase 11, retain the real dependency lock and run state/context/migration/backup tests. No direct multi-process claim until this passes.

### DB-B — immutable/ref/event foundation

- uniform head+revision CAS primitive;
- durable completed-but-unattached/conflicted result;
- append-only branch-ref events;
- stable `store_id`, bounded semantic `commit_log` and cursor API;
- local in-process/UDS wakeup as optional latency hint;
- explicit opaque object canonical digests/version metadata where needed.

Keep `owner.lock`.

### DB-C — logical store interfaces and identity foundation

- introduce transaction-aware Content/Ref/Policy/Receipt/Lease/Migration interfaces;
- stable opaque users + local/Unix/internal bindings;
- organization/workspace membership primitives;
- separate security audit domain;
- continue mapping current simple local mode into the same principal model.

### DB-D — authentication and credential custody

- native credential interface/passkey/device-key support first; optional password verifier;
- auth-session rotation/revocation/security epochs;
- provider-account metadata and SecretStore abstraction;
- personal-device/browser secret-free executor records;
- shared credential rotation/version/audit;
- still no silent fallback.

### DB-E — OIDC/SSO and OAuth connector records

- OIDC provider configuration and PKCE/state/nonce lifecycle;
- issuer+subject identity links and audited account linking;
- OAuth connector grants separately, with SecretStore token refs;
- no email-based implicit merge.

### DB-F — leases and migration coordination

- durable client/process incarnations;
- execution resource leases/fencing;
- migration lease/state/capability negotiation;
- expand/backfill/activate/contract helpers;
- recovery acts only on abandoned incarnations.

Keep `owner.lock` until this passes.

### DB-G — validated local direct mode

- reader connection/pool and short snapshots;
- direct short writers with bounded busy handling;
- semantic local subscriptions from `commit_log`;
- actual multi-process stress/WAL/checkpoint/backup tests;
- only then allow supported trusted local processes to open the DB concurrently.

Owner/service mode remains supported.

### DB-H — cross-system sync

- authenticated peer trust records and capability negotiation;
- per-store cursors/applied-event idempotency;
- immutable-object fetch/digest validation;
- semantic snapshots/log compaction;
- shared-branch writer epoch/explicit divergence behavior;
- generic auth secrets and personal provider keys remain excluded from the feed.

### DB-I — optional hosted scaling

Only after measurements justify it: external broker for fanout, hosted identity/ref backend, object store, KMS/HSM/secret manager or larger SQL backend. These remain adapters behind the same semantics.

## 19. Release gates before direct multi-process/sync/auth claims

Release tests must include:

- same-head race produces one attached winner plus durable conflict result;
- different branches settle concurrently without logical conflict;
- busy acquisition is bounded and never repeats model/tool work;
- pinned reader remains coherent while refs advance and releases read transaction promptly;
- execution lease overlap and crash fencing behave for same/ancestor/descendant roots;
- only one migrator owns each stage and every crash point resumes idempotently;
- older compatible clients become read-only or refuse unknown semantics rather than silently skip context;
- active operations settle across supported schema epochs without replay;
- online backup during readers/writers restores the exact snapshot;
- commit-log wakeup loss recovers from cursor;
- repeated remote sync batch is idempotent;
- same-branch replica divergence preserves both immutable results;
- login token rotation/reuse and `security_epoch` invalidation are durable;
- OIDC identity maps by issuer+subject and does not email-auto-link;
- generic sync/audit/export never contains raw password/provider/OAuth/OIDC secrets;
- shared credential loss produces unavailable, not another account;
- personal provider secret remains on its executor;
- restore forces auth/sync reconciliation where required;
- context/source restriction lineage survives migration, commit-log projection and synchronization.

## 20. Explicit non-goals for the next implementation slice

Do not simultaneously introduce Postgres, Kafka/NATS/Redis, a generic distributed database, network-mounted SQLite, automatic CRDT history, full SAML, KMS/HSM, global multi-master branch refs, or a browser password/key vault. Those are later adapters/features behind this architecture.

The first implementation objective is to strengthen the existing SQLite store without slowing or complicating the single-user path.

## 19. Revision 2.1 clarification

The supplied Revision 2 source was rechecked and is byte-identical to the retained handoff. The design therefore remains one coherent architecture; no alternative schema is introduced. The practical refinement is to treat four additions as first-class roadmap dependencies rather than later hosted-only concerns: **semantic `commit_log`/sync, stable opaque user/auth bindings, a separate long-lived security audit domain, and explicit SecretStore/provider credential custody**.

These additions do **not** change the staged safety boundary: `owner.lock` stays until uniform branch CAS, durable execution leases and migration coordination are validated; personal provider secrets remain off the shared server DB; and a broker/other SQL backend remains optional. See [DATABASE-DESIGN-REFINEMENT-V2.1](DATABASE-DESIGN-REFINEMENT-V2.1.md) for the implementation slices DB-A through DB-I.

## 20. Durable LLM operations and crash retry semantics

A durable record that a model call may have been dispatched is part of the immutable facts plane. The existing `runs`/`invocation_receipts` foundation is extended conceptually into **one logical model operation with one or more immutable provider attempts**. See [LLM-RETRY-RECOVERY](LLM-RETRY-RECOVERY.md).

```text
logical operation M42
  attempt 1: dispatch intent persisted → process crash → completion unknown
  attempt 2: explicit/policy retry       → completed
```

Attempt 1 is never overwritten as "failed with zero usage". Every retry has a new attempt ID and separate provider/account/payer/usage provenance. The operation projection can become `NeedsUserDecision`, `RetryScheduled`, `WaitingForExecutor`, `Completed`, `Blocked` or `RetryExhausted`, with commit-log history under it.

Recovery policy is origin-aware:

- **interactive:** expose the unknown call, possible duplicate cost and explicit Retry/Leave unresolved/provider-status actions;
- **autonomous pipeline:** evaluate the retry-policy snapshot that was persisted with the operation and, when allowed, durably schedule a new attempt with bounded backoff/attempt/budget limits;
- **tool effects:** preserve the stricter hazard model; unknown external effects are not automatically retried.

This does not relax the no-blind-replay invariant. Automatic autonomous retry is an explicitly authorized **new paid attempt** after reconciliation/policy evaluation. `SQLITE_BUSY`, migration, process restart, lost wakeup or branch CAS conflict never by themselves repeat a provider request.

The operation pins the exact source head/revision/context digest it was generated from. Same-context retries preserve that snapshot. A retry against the latest branch is a new logical operation. If a completed retry loses branch CAS, keep its paid result unattached and do not regenerate it automatically.

Durable retry scheduling belongs in SQLite/ReceiptStore + CommitLog; no mandatory external queue is required. Direct multi-process mode later adds generation-fenced retry claims so only one process dispatches the next attempt. While `owner.lock` remains, use the same durable representation to make owner crashes/restarts deterministic.

Focused refinement: [DATABASE-DESIGN-REFINEMENT-V2.2](DATABASE-DESIGN-REFINEMENT-V2.2.md).

## Phase 12 implementation checkpoint — schema 3 / source 0.13.0

Phase 12 implements the first durability slice of this target architecture while deliberately retaining `owner.lock`.

Implemented in authored source:

- schema3 `store_meta`, `commit_log`, `branch_ref_events`, stable user/identity metadata, security-audit plane, provider-account/device metadata, lease/migration/sync metadata tables;
- logical LLM operations plus immutable attempt lineage and original branch head/revision provenance;
- durable recovery hints and retry schedules;
- explicit interactive retry authorization and exact same-context retry lineage;
- autonomous retry due/backoff state and a generation-fenced due-claim primitive (claiming does not invoke a model);
- completed run/result persistence before branch CAS, retaining an attachment conflict instead of regenerating paid work;
- semantic commit events for session/branch/model/retry/context/membership lifecycle mutations covered by the current runtime;
- default CLI sessions remain durable unless `--no-state` is explicitly selected; web/native snapshots expose authorized recovery hints.

The schema also introduces additive metadata structures for later identity/auth, SecretStore references, execution leases, migration coordination and semantic sync. Their tables are **not** a claim that OIDC, a raw-secret backend, cross-store replication, or direct multi-process DB writers are running. Those application services remain later work.

The generalized autonomous pipeline runner is also not introduced here. A host can persist autonomous retry policy, recover/schedule it and exclusively claim a due next attempt; the future autonomous runner must consume that claim and bind the operation before dispatch. Unknown tool/external effects block this path.

Reference SQL/JavaScript/Python validation executes in the authoring environment; Rust/rusqlite integration remains pending the real Cargo/Nix gate.

## Machine/Fleet execution-plane extension — design amendment

The durable control plane should add stable Machines, MachineWorkspaces, Fleets, membership revisions, main-host FleetWorkspace manifests/bindings, session execution targets, FleetOperations and per-machine MachineRuns. Current tunnel connection instances are projections/ephemeral connectivity, while machine identity, workspace bindings, target snapshots and operation receipts are durable. Relevant mutations append semantic `commit_log` events; machine registration/trust changes also append security audit events.

A FleetWorkspace is main-host authoritative. Fleet members are execution endpoints, not automatically content/control-plane replicas. Every fleet operation pins exact member/workspace/capability revisions before dispatch. Phase 12 retry lineage applies per MachineRun; unknown machine effects remain tied to that machine and are not silently relocated. No schema/runtime change ships with this design amendment. See [FLEETS](FLEETS.md).

## Phase 34 implementation checkpoint — schema 19 / local coordination proof

Phase 34 implements the previously deferred DB-F coordination layer while deliberately retaining `owner.lock` as the ordinary/default product boundary.

Implemented and native-tested:

- durable local process incarnations bound to permanent store ID + restore generation;
- generation-fenced write and execution leases;
- real provider `InvocationStarted` execution lease before provider entry;
- dead/stale execution holder becomes a hazard, not proof the effect stopped;
- startup makes unknown provider state durable before reconciling the process-coordination hazard;
- one exclusive migration/backup/restore maintenance lease that also fences ordinary store APIs;
- bounded typed SQLite Busy/Locked behavior with no provider/tool replay;
- paid result persistence and branch attachment split into two committed transactions;
- startup reconciliation of settled `attach_status=pending` without provider inference;
- same-store restore stages the next restore generation and stale-process/lease fencing before restored bytes can become live;
- real independent-WAL and child-process kill tests for request/head/revision/Workspace-writer CAS, provider dispatch, result settlement, maintenance and restore boundaries.

The independent multi-process SQLite opener is `#[cfg(test)]` only. Phase 34 is evidence that an explicit trusted-local direct mode can be considered later; it is **not** a claim that direct mode is currently shipped. Raw WAL remains non-semantic storage-engine state and is never the application sync protocol.

## Phase 35 implementation checkpoint — schema 20 / explicit trusted-local direct mode

Phase 35 performs the required fresh review before exposing DB-G. That review found an exclusive-owner assumption that had to be fixed first: startup recovery still treated every `running` LLM run, started context operation, and accepted web request as abandoned. That is valid under one owner but unsafe with multiple live direct processes.

Schema v20 therefore adds exact local-process/store-generation ownership to those three in-progress state machines. Direct recovery only reconciles work whose owning process is missing, stopped, fenced, generation-stale, or heartbeat-expired; a live peer's work is left untouched. Unowned/pre-v20 live rows fail closed in direct mode and require an authoritative owner-mode recovery pass.

The supported ownership modes are deliberately mutually exclusive through the existing `owner.lock` file:

- ordinary `SessionStore::open`: exclusive lock, owner/service mode, still the default;
- `SessionStore::open_trusted_local_direct`: shared lock, explicit trusted-local mode;
- multiple direct clients may coexist;
- owner/service and direct clients may not coexist.

This file-lock split gives deterministic crash authority without PID guesses. A direct shared-lock acquisition proves no owner currently holds the store, so stale owner rows may be fenced. An owner exclusive-lock acquisition proves all earlier owner/direct lock holders are gone, so abandoned process rows may be fenced immediately even before a trusted-direct heartbeat TTL expires.

The direct opener is current-schema-only and never creates/migrates a store. It requires an existing private state directory, exact schema v20/application ID, `coordination_schema=2`, and a conservative supported-local-filesystem check; network/FUSE/clustered/unknown filesystems fail closed. Restore remains owner/service-only.

Phase 35 intentionally uses one conservative `store:ordinary-access` coordination scope for every persistent ordinary store API call. This serializes ordinary API access across owner/direct processes, closes the maintenance check-to-transaction race, and keeps Busy/Locked handling bounded. It can later be refined into narrower reader/resource scopes without changing the authority protocol.

Real provider execution leases, unknown-effect no-replay ordering, two-transaction paid-result settlement, exact request/head/revision/Workspace-writer-epoch CAS, staged restore-generation fencing, SecretStore/OIDC/user/context authority and semantic replication remain unchanged.

See [accepted history](../plans/accepted-history/TODO.md) for the full implementation and adversarial proof.

## Phase 38 implementation checkpoint — schema 21 / shared drafts

Phase 38 advances current `user_version` to 21 only to hold collaboration state. It does not redesign the Phase-35 ownership/coordination protocol. Owner/service still takes `owner.lock` exclusively; trusted-local direct takes it shared; the two modes do not coexist; direct remains current-schema/local-filesystem/no-migrate/no-restore.

The v21 tables are deliberately not conversation-tree or global-CRDT tables. They hold one branch-bound shared prompt generation, exact policy/audience revisions, bounded accepted Yjs-compatible updates, stable-user contributor attribution and a unique sealed submission receipt. Immutable Session nodes are still produced only by normal prompt admission. Presence remains memory-only.

Audience or mode revision abandons the active generation so old CRDT history is not silently reused under a changed disclosure boundary. Exact seal verifies document epoch, accepted text revision, metadata revision, branch head/revision and candidate digest. A competing/duplicate Send for one generation resolves to the same submission identity. No provider credential, payer, tool grant or private provider continuation is stored in the shared-draft schema.

## Phase 39 implementation checkpoint — schema 22 / bounded pipelines

Phase 39 advances current `user_version` to 22 with separate immutable pipeline definitions/revisions, explicit bindings, durable runs, step-attempt receipts and approval records. Pipeline state is not Session history and stores no provider credential, provider URL, shell command or tool grant. Manual runs pin one immutable revision and exact request fingerprint. External `agent` steps persist a `started` intent before invoking the existing Agent; an unresolved started step is reported as unknown and is never silently replayed. No SQLite transaction spans provider I/O or human approval.

The Phase-35 ownership protocol is unchanged: owner/service still takes `owner.lock` exclusively; trusted-local direct takes it shared and remains exact-current-schema/no-migrate/no-restore. Under Phase 39 exact-current-schema means v22. The v21→v22 migration is actual-binary tested through a first durable prompt.

SQLite error reporting is hardened without weakening no-replay: Busy/Locked retains the existing bounded conflict, while other SQLite failures surface a bounded sanitized error class, extended code and message so durable-state constraint failures are diagnosable.


## Unnumbered post-Phase-39 compatibility repair — schema 23

Current owner-mode SQLite is schema v23. This is not a new product phase: it repairs a legacy schema-v22 shape observed in user testing where `context_operations.owner_process_id` (and potentially the paired Phase-35 ownership columns/indexes on `runs` / `context_operations` / `web_requests`) were absent even though metadata had already advanced past the historical Phase-35 migration. Owner-mode v22→v23 introspects `pragma_table_info`, conditionally adds only missing `owner_process_id` / `owner_store_generation` columns, recreates the three local-process indexes with `IF NOT EXISTS`, runs foreign-key/integrity checks, and only then starts owner-process recovery. Existing correctly-shaped v22 stores therefore receive metadata/index advancement without duplicate-column failure. Trusted-local direct remains exact-current-schema/no-migrate, so a v22 store must be opened once in owner mode before direct access.

## Phase 40 implementation checkpoint — schema 24 / bounded main-target routing

Phase 40 advances live owner-mode SQLite to v24. It adds immutable `routing_strategies` / `routing_strategy_revisions`, explicit user-or-Session `routing_bindings`, per-stable-user + Session + branch + role + lineage `routing_affinities`, durable `routing_operations`, and one paid-classifier `routing_attempt` per classified operation. Route definitions are secret-free and contain only candidate IDs, configured account/model references, bounded descriptions and a destination fingerprint derived from the same endpoint/custody-reference tuple used by durable Session binding. Raw provider credentials, endpoint response bodies, tool grants and arbitrary classifier-generated configuration are absent.

Classifier work is a separate external effect from Chat. Its attempt intent is committed with local process/store-generation ownership before provider entry. A dead/fenced owner changes a surviving started classifier attempt and operation to `unknown_external_effect`; recovery never creates a replacement classifier call. Route-each therefore means one newly admitted classifier operation per new prompt, not replay of an uncertain one.

A selected route is not sufficient by itself to execute Chat. `routing_operations` gains one-shot `working_identity_json` + `consumed_at`; the normal `SessionHandle::begin` transaction consumes that exact route receipt only when actor, Session, branch, pinned head/revision and selected account/model all match. The newly created logical `llm_operations` row stores `routing_operation_id`. This keeps the existing Agent/Session journal as the only working-turn authority and lets explicit same-context retry reconstruct/revalidate the original routed target instead of rerouting.

Route-once affinity stores the immutable strategy revision and target ID plus CAS revision/establishing operation. It deliberately does **not** persist `ProviderAccounts::grant_revision`: that lease counter is process-local and changes on in-memory account registration. Every affinity reuse instead rechecks the immutable destination fingerprint, current saved-profile model allowlist and a fresh current `ProviderAccounts::select()` capability. Endpoint/custody-reference changes fail closed before provider entry and require an explicit new strategy revision.

The Phase-35 ownership protocol is unchanged. Owner/service takes `owner.lock` exclusively; trusted-local direct takes it shared, remains exact-current-schema/no-migrate/no-restore, and therefore now requires v24. Historical Phase39 v22 pipeline state and the unnumbered v23 coordination-shape repair remain intact beneath this additive routing layer.


## Phase 41 — schema v25 bounded subagent Sessions

`025_subagents.sql` adds only `subagent_sessions`; child conversation data stays in ordinary `sessions` / `branches` / immutable nodes / run and invocation receipts. The relation pins parent Session, branch, head, revision and turn, child Session, creator stable user, foreground/background mode, lifecycle, task SHA-256, exact secret-free `InvocationIdentity`, and effective child tool mode. No credential, endpoint, raw task body, arbitrary system prompt, routing definition, cron or webhook is stored there.

Native parent-owner admission creates the child Session and relation transactionally in the parent Workspace before child provider entry. The relation moves `admitted -> running -> completed|failed|cancelled|interrupted`; restart resolves surviving admitted/running relations to `interrupted` but never manufactures a replacement provider call. The child Session's normal journal keeps provider intent/result/unknown-effect evidence.

Because parent `subagent` ToolStarted is a non-read effect, the parent run creates the ordinary physical-root hazard. A Phase-41 child is explicitly model-only. `model_only_child_run_key` may return `subagent:<physical-root-key>:<child-session>` only while the durable relation is running, stored invocation identity matches, the exact parent branch run remains active on the physical root, and no foreign hazard/run exists. Projected child history ignores only that exact parent hazard during this eligibility window. Any unrelated hazard fails closed. Writable/read/shell child tools therefore remain deferred rather than weakening the physical-root coordination invariant.

Trusted-local direct remains exact-current-schema/no-migrate/no-restore and therefore now requires v25. Phase-41 spawning itself is native owner-only.


## Phase 42 — schema v26 isolated subagent execution bindings

`026_subagent_execution_bindings.sql` adds a durable intent/receipt table for host-managed Git worktrees. Parent repository and execution-root identity are exact platform path bytes stored as BLOBs. The record pins the exact parent Session/branch/turn, stable creating user, read-only/workspace tool mode and exact Git base object. Worktree creation/removal are external effects: intent is committed before Git process entry, post-effect verification is required before success, and owner restart/ambiguous mutation becomes `unknown` without replay.

A tool-enabled child consumes one `ready` binding in the same SQLite transaction that creates its ordinary child Session. The child Session binding maps the parent's cwd/tool-root relative positions into the managed worktree, yielding a distinct physical effect root. It therefore uses ordinary run/hazard fencing and never the Phase-41 model-only same-root bypass. Cleanup is separately intented and cannot delete child Session/history. Trusted-local direct still requires exact current schema v26 and never provisions/cleans managed worktrees.


## Phase 43 — schema v27 delegation envelopes

`027_delegation_budgets.sql` adds three authority-free accounting tables: `delegation_roots`, `delegation_child_allocations`, and `delegation_model_attempts`. The root is unique to an exact parent Session/branch/turn and stores bounded immutable limits (created children, active children, model attempts, one-hour-style deadline configuration, max depth=1) plus monotonic/active counters and provider-reported token evidence. It stores no credential, endpoint, route, shell command, price or currency.

Every new child relation and managed execution binding must link to the root/allocation. Allocation reservation precedes Phase-42 worktree mutation; child running atomically consumes an active slot; terminal settlement/recovery releases it exactly once. Model-attempt reservation precedes the ordinary provider invocation intent and provider entry. Ordinary invocation result persistence remains authoritative and occurs before delegation usage settlement, so accounting cannot sacrifice paid-result durability. Started attempts that survive owner restart become `unknown` and stay consumed. Trusted-local direct remains exact-current-schema/no-migrate/no-provision and therefore now requires v27.


## Phase 44 — schema v28 recursive delegation lineage

`028_recursive_subagents.sql` is additive. Because Phase-43's legacy depth columns are constrained to one and the child/worktree tables participate in cyclic foreign-key relationships, v28 avoids destructive table reconstruction. It adds `recursive_max_depth` (1..4) to roots, `recursive_depth` plus self-referential `parent_allocation_id` to child allocations, and `recursive_depth` to subagent relations. Existing rows default to one. Triggers enforce immutable root max depth, top-level/no-parent versus descendant/required-parent shape, same-root running parent allocation with exact depth+1, and child relation depth matching its allocation.

A descendant reservation increments the same root `children_created`; running increments the same `active_children`; child provider rounds continue to consume the same root model-attempt/deadline envelope. Tool-enabled descendants allocate ordinary Phase-42 execution bindings linked to the new descendant allocation. No descendant-specific resettable counters, routes, accounts or schedules exist. Trusted-local direct remains exact-current-schema and therefore requires v28.


## Phase 45 — schema v29 governed child routing

`029_child_routing.sql` additively links the existing Phase-40 routing tables to the Phase-43/44 delegation graph. `delegation_roots.child_routing_revision_id` pins one optional immutable worker strategy revision. `delegation_child_allocations` stores the one consumed routing operation, target ID, selection source, selected secret-free InvocationIdentity JSON and binding timestamp. A unique route-operation index prevents one route receipt from authorizing two child allocations.

`delegation_routing_attempts` accounts classified child routing against the root's existing model-attempt/deadline counters. It stores no provider URL, credential, price or arbitrary target. Child worktree admission is blocked until a routed allocation has a selected identity, and child Session admission requires the exact selected identity. The selected target's destination fingerprint is derived transactionally from the immutable route revision/target and written into the child Session binding. Existing null-route roots keep inherit behavior. Trusted-local direct remains exact-current-schema and therefore requires v29.


## Phase 46 — schema v30 bounded team topology

`030_team_topology.sql` adds `team_templates`, immutable `team_template_revisions`, `team_runs`, `team_run_members` and immutable `team_join_receipts`. Definitions are bounded typed data from `aw-pipeline`; no provider URL, credential/account/model ID, shell command, route candidate, worktree, budget, scheduler or executable code is stored.

Team execution does not duplicate child transcript bodies into SQLite. `team_run_members` stores only the child relation/session links, answer digest and truncation evidence. Before a member is accepted, the store reconstructs the child Session's authoritative completed one-turn history and requires the imported answer to be exactly that answer or an explicitly truncated strict prefix. `team_join_receipts` stores only the ordered provenance manifest and joined digest. Join is accepted only when all required durable member receipts are completed and match the deterministic template-order join. Trusted-local direct remains exact-current-schema and therefore requires v30.


## Phase 49 — schema v31 planner DAG receipts

`031_planner_dags.sql` is additive. It adds `delegation_roots.planner_routing_revision_id` and three planner tables. `planner_operations` owns paid-planner intent/outcome and the validated proposal. `planner_task_runs` owns deterministic task order/dependency/lifecycle and child provenance. `planner_join_receipts` is immutable and owns only the final provenance manifest + joined SHA-256. No table stores provider credentials, endpoint URLs, tool grants, shell commands, schedules or full imported child answer bodies.

Planner admission may pin a planner revision into an existing active delegation root only while the pin is null; once non-null it is immutable. The planner strategy must be active, actor-owned, role `planner`, mode `fixed`, and its selected target identity must match the actual pre-authorized provider selection. The operation pins the exact parent branch head/revision and planner evidence digest before provider entry.

Planner `begin` atomically consumes one root `model_attempts_started` slot before returning `InvocationPurpose::Planner`. Settlement increments root finished/known-usage counters and only then materializes validated task rows. Child-created counts are untouched until real subagent allocations occur. v30→v31 migration preserves store identity/restore generation; old roots migrate with null planner routing pins. Trusted-local direct remains exact-current-schema and therefore requires v31.

## Phase 50 — schema v32 governed guardian review receipts

`032_guardian_reviews.sql` is additive. `delegation_roots.guardian_routing_revision_id` pins one optional immutable actor-owned fixed guardian strategy. `review_operations` stores exact source/root/parent identity provenance, guardian identity, policy/objective/input digests, bounded host policy text, lifecycle and strict verdict metadata. `review_human_approvals` stores the explicit stable-owner decision when a guardian returns `needs_human`. `review_release_receipts` is immutable and stores only the reviewed source digest, release basis, releasing stable user and timestamp.

No review table stores full child answers or joined team/planner evidence. Release reconstructs the same bounded join deterministically from authoritative child Sessions and re-verifies the Phase-46/49 join digest. This preserves the existing rule that candidate answers have one authoritative durable home while policy receipts carry only provenance/digests.

Guardian provider intent is a separate external effect and uses `InvocationPurpose::Guardian`. One root `model_attempts_started` slot is reserved before provider entry and normal settlement updates the same root finished/usage counters. An owner restart changes admitted review work to `interrupted` and started provider intent to `unknown`; it never manufactures a replacement guardian call. Settled waiting-human decisions are ordinary durable local state and survive restart.

A positive guardian verdict or human approval is only a gate result. It cannot grant missing Session/provider/tool/root/routing/budget authority. `review_release` additionally requires the original parent `InvocationIdentity`, so approval cannot become a context-transfer mechanism. v31→v32 migration preserves permanent store identity and restore generation. Trusted-local direct requires exact v32 and still never migrates.
