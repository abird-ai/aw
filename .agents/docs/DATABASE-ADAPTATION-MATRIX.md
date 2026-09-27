# Database handoff v2 → current aw adaptation matrix

**Baseline compared:** Phase 11 / source 0.12.0 (Phase 10 schema 2 plus Phase 11 SDK/validation hardening).  
**Input:** [accepted history](../plans/accepted-history/TODO.md).
**Status words:** **implemented-source** means present in authored source, not compiled/validated; **designed** means present in accepted design but not runtime; **new** means the v2 handoff adds a material target not already implemented.


> **Phase 12 implementation update (source 0.13.0):** schema3 now authors the first DB-B durability slice: semantic `commit_log`, immutable `branch_ref_events`, durable logical LLM operation/attempt lineage, recovery hints, retry schedules/claims, paid-result attachment-conflict preservation, stable-user/security-audit/provider-device metadata, and additive lease/migration/sync metadata tables. `owner.lock` remains; direct multi-process writers, OIDC/auth services, raw SecretStore backend, and cross-store sync are still not enabled. The table below remains useful as the full target gap analysis; rows labelled “new” at the Phase 11 baseline may therefore now be partially or substantially implemented in Phase 12.

| Area | Current aw state | Adopted design after v2 | What changes / implementation needed | Status |
|---|---|---|---|---|
| Physical store | One SQLite/WAL database, schema 2. | Keep SQLite as initial physical backend. | None conceptually; benchmark before any split. | implemented-source / keep |
| Store ownership | One cooperative process owns DB through `owner.lock`; attach/web go through owner. | Add supported trusted local direct clients only after durable coordination. | Do not remove lock until CAS/leases/migrations/recovery tests pass. | implemented-source → future change |
| Immutable session tree | One-parent immutable nodes; context import/compaction added in schema 2. | Preserve as ContentStore/object plane. | Add consistent object schema/digest metadata where useful. | implemented-source / strengthen |
| Forks | Branch ref points to existing head; no transcript copy. | Keep cheap O(1)-storage fork semantics. | Ensure branch-policy/ref events and uniform CAS. | implemented-source / extend |
| Branch CAS | Revision/stale-target checks exist in key paths. | Every ref mutation uses expected head + expected revision. | Centralize `RefStore::cas` and typed conflict. | partial |
| Branch ref history | Audit contains some branch events. | Dedicated immutable `branch_ref_events`. | Add event rows atomically with current-ref update. | new runtime work |
| Paid stale result | Current normal settlement couples final result and branch update. | Completed result can survive attachment conflict. | Split execution outcome from attachment outcome; preserve result and offer fork/compare/reuse. | important new work |
| Runs/receipts | Durable model/tool intents/outcomes and no-blind-retry receipts. | Preserve under `ReceiptStore`; event-log changes only notify. | Typed DB contention versus external uncertainty. | implemented-source / extend |
| Hazards | Durable exact-root possible-effect state exists. | Evolve into generation-fenced multi-process execution leases + hazards. | Canonical overlap domains, holder incarnation, expiry, fencing. | partial |
| Reader concurrency | One connection behind mutex/owner. | Short reader snapshots pinned by branch head/revision. | Reader connections/pool; release DB read tx after materializing immutable path. | new |
| Writer concurrency | App writes mediated by single owner. | Short independent local transactions in WAL. | Bounded `SQLITE_BUSY` handling and direct-writer tests. | new |
| Semantic subscriptions | Existing UI/server event mechanisms are owner-local, not durable store feed. | SQLite `commit_log` + lossy wakeup + cursor replay. | Add store ID, semantic event kinds, cursor API and bounded retention/snapshots. | new |
| Raw WAL | Not application protocol. | Explicitly never use raw WAL as domain event stream. | Preserve. | keep |
| Cross-system sync | Not implemented. | Replicate semantic events + missing immutable objects, per-store cursors/snapshots. | Peer trust, capability negotiation, applied-event dedupe, writer epochs. | new |
| Same-branch replica conflict | Not applicable today. | Preserve both descendants; explicit divergence/conflict, no LWW. | Add sync conflict branch/multi-head policy. | new |
| Broker | None. | Optional accelerator only after `commit_log`; never source of truth. | No work now. | deferred |
| Current migration | Transactional schema 1→2 migration. | Keep as historical migration. | Full rolling migration coordinator needed before multi-client writes. | implemented-source + new framework |
| Schema epochs/capabilities | Current `user_version` and protocol versions are separate in places. | Explicit DB/object/native/browser/migration versions and read/write capability ranges. | Add `Store::open` compatibility states. | partial |
| Migration lease/state | Process ownership prevents concurrent migrators. | Durable migration lease + stage/cursor ledger. | Expand/backfill/activate/contract; resume crashes. | new |
| Recovery | Exclusive opener marks unfinished work interrupted. | Recover only abandoned client/lease incarnations once multi-process. | Add client IDs/generations/heartbeats as required. | blocking change for direct mode |
| Logical storage boundaries | `aw-store` exists but mostly concrete SessionStore API. | Content/Ref/Policy/Identity/Auth/Audit/CommitLog/Sync/Receipt/Lease/Migration/Secret/Presence. | Add transaction-aware traits incrementally; avoid physical split. | design/new abstraction |
| Stable user identity | `aw-identity` names `internal:<realm>:<id>`; Unix actor and invite team identities exist. | Opaque stable `user_id` primary identity with auth bindings. | Keep qualified names as aliases/bindings; migrate DB FKs over time. | partial / design refinement |
| aw-native login | Invite pairing; no durable general login credential service. | Passkey/device key first, optional Argon2id verifier. | Identity credential tables and authentication service. | new |
| OIDC/SSO | Present in old design, not current runtime. | OIDC auth-code + PKCE, issuer+subject identity link. | Provider config, flow state, linking/audit; never email-auto-link. | designed → implement |
| OAuth connections | Not general runtime. | Separate delegated connector authorization from login identity. | OAuth connection metadata + SecretStore token refs. | new/previously conceptual |
| Organizations/roles | Realm/participants exist; no generalized org/RBAC plane. | Organizations/workspaces/membership/role/resource-policy plane. | Stable policy revision/epochs and branch-private overrides. | partial → implement |
| Auth sessions | Pairing/invite cookies are narrow and mostly runtime-scoped. | Durable hashed/rotating auth sessions and revocation families. | Short access + refresh rotation, reuse detection, security epoch. | new |
| Existing audit | Immutable store audit covers grants/forks/import/recovery subset. | Separate long-lived security `AuditStore`; operational `commit_log` remains compactable. | Add auth/admin/credential/sync/migration audit events and scrubbed metadata. | partial → extend |
| Shared provider creds | Owner profiles/config refs exist; no dedicated SecretStore. | DB stores opaque provider-account metadata; raw shared keys in SecretStore. | SecretStore trait/backend, versioned refs, rotation/audit. | new architecture |
| Personal provider creds | Phase 8+ device worker keeps key local; no fallback. | Preserve. Add durable executor/device metadata; optional browser-local mode is lower trust. | Device registration; browser-local crypto only if later explicitly implemented. | implemented-source / preserve |
| Browser local credential | Deliberately not shipped. | Optional encrypted IndexedDB/non-exportable WebCrypto wrapper where feasible, with same-origin caveat. | Do not use localStorage; native helper remains preferred for high-value secrets. | design-only optional |
| Device/executor records | Device link/grant is config/protocol metadata, not generalized durable user device registry. | Stable user-device/executor records and revocation/security epoch. | Add user_devices metadata/public key/capabilities. | new |
| Sync policy for secrets | No cross-system sync. | Explicit classes: content-sync, identity-local, SecretStore-only, ephemeral. | Encode peer event/object allowlists. | new |
| Multi-tenant scope | Realm/team stamps exist; not every row is normalized tenant-aware. | Unambiguous tenant/workspace scope on security-sensitive data. | Incremental backfill; fail closed if ownership ambiguous. | partial |
| Backup | WAL-aware `VACUUM INTO` style backup for state. | Deployment backup can span content DB, identity DB, SecretStore/KMS and audit archive. | Add metadata/restore generation/session/sync reconciliation. | partial / extend |
| Restore and sync | No cross-system peers. | Restore can move refs backward; increment restore generation and force peer/auth reconciliation. | Add store/restore epochs to sync and session invalidation policy. | new |
| Retention/deletion | Mostly immutable with bounded views; limited GC/deletion semantics. | Per-data-class retention; tombstone/redaction semantics where content deletion is required. | Design/admin API + sync snapshot rebuild rules. | new |
| Presence/shared draft | Design separates it; runtime collaboration not completed. | `PresenceStore` remains ephemeral/on-demand. | Keep outside immutable DB; optional CRDT recovery log later. | designed/deferred |
| Queue/broker | None. | No mandatory broker. | Optional publisher from durable commit log later. | keep/deferred |


## Revision 2.1 refinement

The newly supplied Revision 2 handoff is byte-identical to the source already retained in this repository. This refinement therefore does not create a competing storage design. It makes the four Revision 2 additions explicit in the implementation map: **semantic commit-log/sync, stable identity/auth, separate security audit, and SecretStore/credential custody**. See [DATABASE-DESIGN-REFINEMENT-V2.1](DATABASE-DESIGN-REFINEMENT-V2.1.md).

Implementation grouping:

| Slice | Primary deliverable | owner.lock status |
|---|---|---|
| DB-A | Compile/correct Phase 11 and validate schema2/backup/context | required |
| DB-B | Uniform CAS, durable conflict results, ref events, `store_id`, `commit_log` and cursor/wakeup API | required |
| DB-C | Transaction-aware logical stores, opaque stable users and expanded AuditStore | required |
| DB-D | Native auth sessions, SecretStore, provider/device metadata | required |
| DB-E | OIDC/SSO + separate OAuth connector model | required |
| DB-F | Execution leases + migration coordinator/capabilities | required |
| DB-G | Validated trusted local direct readers/writers | optional for service mode; removable for supported local-direct mode |
| DB-H | Authenticated semantic cross-system sync and snapshots | n/a to remote peers |
| DB-I | Optional hosted backend/broker/object/KMS split from measurements | deployment-specific |

## Highest-priority design changes

1. **Add `commit_log` as the durable semantic change feed before live direct readers/sync.**
2. **Canonicalize stable user identity separately from Unix/internal/OIDC login bindings.**
3. **Separate security audit from operational notifications.**
4. **Introduce provider-account metadata + SecretStore for shared keys while keeping personal keys local.**
5. **Finish branch CAS/result-conflict and durable leases before deleting `owner.lock`.**
6. **Make migration a resumable DB protocol transition rather than per-client auto-SQL.**
7. **Add sync only after the local event/object semantics are correct; never generic SQLite replication.**

## Immediate implementation order

The implementation order is defined in [DATABASE-ARCHITECTURE](DATABASE-ARCHITECTURE.md#18-implementation-order-adopted-for-aw). Phase 11's compiler/Crane correction gate remains DB-A and must run before this design is advertised as working runtime behavior.

## Retry/recovery refinement

| Area | Current source | Adapted target | Required work | Status |
|---|---|---|---|---|
| Provider-dispatch durability | `InvocationStarted`/intent is persisted before provider polling; incomplete calls recover as interrupted/unknown. | Preserve every possible dispatch as an immutable attempt fact. | Do not downgrade unknown usage to zero or erase the attempt. | implemented-source foundation |
| Logical operation vs attempt | Current invocation receipt primarily represents a model round. | Stable logical operation containing 1..N immutable provider attempts. | Add operation/attempt identity and retry lineage. | extend |
| Interactive crash recovery | Recovery is conservative and does not replay. | Show unresolved attempt and offer explicit same-context retry, latest-context new operation, provider-status lookup or abandon. | UI/API/store projection. | new behavior |
| Autonomous crash recovery | No generalized autonomous pipeline retry runtime yet. | Persisted pipeline-step retry snapshot automatically schedules a **new** model attempt when eligible. | Retry scheduler/projection, budgets/backoff, pipeline integration. | new |
| Durable retry schedule | Not generalized. | SQLite due-at/current projection + semantic events; restart-safe. | Add retry schedule and later fenced claims. | new |
| Provider reconciliation | No generic provider status API. | Optional read-only provider request-status/idempotency capability. | Capability interface; provider-specific implementations only when documented. | optional/new |
| Retry account selection | Existing account attribution and no-fallback rule. | Same current authorized account/model by default; no crash-induced reroute/fallback. | Recheck permission/budget before each attempt. | preserve/extend |
| Personal-device retry | Device receipts already prevent blind replay. | Wait/retry through same local executor and current grant; unavailable/revoked never falls back. | Add autonomous wait/backoff integration without resetting ledger. | preserve/extend |
| Completed-but-unattached result | Target DB design already requires durable conflict result. | Never retry solely because branch CAS lost. | Persist result + attachment status and offer fork/compare/summary. | target work |
| Tool-loop unknown effects | Hazards already block uncertain external effects. | Autonomous model retry pauses behind unknown tool effect. | Keep model attempt and effect attempt separate. | preserve/clarify |
| Retry accounting | Per invocation usage exists, unknown remains unknown. | Sum known attempt usage, count unknown attempts, reserve autonomous retry budget conservatively. | Operation-level projection. | extend |
| Commit log | Design target. | Publish attempt/retry/recovery state semantically. | Add events atomically with state. | DB-B |
| Security audit | Narrow current audit; v2 target broader. | Manual retry approvals, autonomous policy changes and shared credential retry use are audit-relevant metadata. | No secrets in audit. | extend |

**Key invariant:** automatic autonomous retry does not contradict the existing no-replay rule. It creates a separately admitted attempt with a new attempt ID after durable policy/reconciliation checks; the prior unknown attempt remains immutable evidence and may still have incurred provider usage.

Focused retry/recovery refinement: [DATABASE-DESIGN-REFINEMENT-V2.2](DATABASE-DESIGN-REFINEMENT-V2.2.md).
