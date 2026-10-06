# Phase 30 fresh review — MVP hardening pass

This is a clean-room review of the Phase 30 checkpoint (`23e177f` / `phase-30`) for bugs, regressions, security/correctness gaps, scalability, user-facing clarity and maintenance cost. It does **not** advance the product to Phase 31. The goal is to make the completed distributed MVP boundary safe and simple before adding auth sessions or SecretStore custody.

## Review conclusions

The high-level architecture remains sound and was **not** redesigned:

- one Rust binary;
- one SQLite owner per local store;
- Domain → Workspace → Session;
- pairing is peer identity only;
- typed capabilities above transport;
- semantic replication rather than raw SQLite/WAL replication;
- one Workspace mutable-ref writer + monotonic epoch + CAS;
- stable opaque `user_id` as cross-host human authorization identity;
- provider payer/credential custody separate from collaboration identity.

The fresh review did find several concrete implementation and product-surface defects. They were fixed by tightening existing boundaries rather than adding new subsystems.

## 1. Continuity re-accept could revive stale credentials

### Problem

Revoking an `identity_continuity_grant` stopped authentication because proof verification required an active grant, but re-accepting the grant could immediately make the old imported device bindings authoritative again without a fresh signed source snapshot.

If the source revoked a device while trust was disabled, the target could therefore revive stale credential state simply by re-accepting the source.

### Fix

- continuity-source revocation now also marks that source's currently imported portable bindings revoked;
- explicit re-accept restores source trust only;
- credentials remain unusable until a fresh signed identity-continuity sync is applied;
- an unchanged signed snapshot may reapply its safe projection, but immutable evidence is reused rather than duplicated.

This gives one intuitive lifecycle:

```text
revoke trust
  -> portable credentials inactive
re-accept trust
  -> source trusted, credentials still inactive
fresh signed sync
  -> exact current public credential projection active again
```

## 2. Historical principal text could bypass stable-user status

### Problem

Some Session/member/context authorization queries checked the historical principal string (`unix:1000`, `internal:...`) before or alongside the stable-user projection. Once a row had `owner_user_id` / `principal_user_id`, suspending or revoking the stable user could still be bypassed by presenting the old principal string.

`ensure_user_for_principal` also used an active-only lookup, so a known but inactive binding could be mistaken for an unknown principal and trigger an attempted second user bootstrap.

### Fix

- principal lookup now distinguishes:
  - unknown binding;
  - known active binding;
  - known inactive/revoked binding;
- known inactive bindings fail closed and never bootstrap another user;
- if a row has a stable-user projection, that projection is authoritative;
- raw principal equality is permitted only for genuinely legacy rows whose stable-user projection is `NULL`;
- Session list/owner/member and context plan/application/grant/pending lookups follow the same rule.

Regression tests suspend owners and members and prove that neither their historical principal nor their stable principal can regain access.

## 3. Cross-host artifact owner collision

### Problem

A replicated summary artifact may preserve historical provenance `owner = unix:1000` from machine A. Machine B may also have a different local `unix:1000`. One ownership check compared the historical text before consulting `owner_user_id`, allowing a same-text cross-host collision.

Downstream source-release checks also used the historical artifact owner instead of the stable projected owner.

### Fix

`artifact_owner_principal` now canonicalizes artifact ownership:

```text
owner_user_id present -> user:usr_<stable-id>
owner_user_id absent  -> historical principal (legacy fallback only)
```

That canonical principal is used consistently for:

- owner checks;
- private imported-artifact audience checks;
- source-use checks;
- release-version validation.

A dedicated regression proves local `unix:1000` cannot impersonate a replicated artifact owned by another stable user whose historical label also happens to be `unix:1000`.

## 4. Phase 30 migration could fail on real Phase 29 data

### Problem

Schema v15 adds stable-user columns to context tables and backfills them with `UPDATE`. Earlier context phases intentionally install immutable-update triggers on plans, artifacts, applications, derivations and operation identity.

An empty/fresh-store migration passed, but a real v14 database containing those rows could abort during the v15 backfill.

### Fix

The v14 → v15 migration now transactionally:

1. adds the stable-user columns;
2. drops only the affected update guards;
3. backfills the stable-user projections;
4. restores v15-aware immutable guards;
5. rebuilds derivation inheritance to propagate `requester_user_id`;
6. keeps the entire operation within the migration transaction and foreign-key check.

Plans/application receipts remain completely immutable afterward.

Artifacts and derivations permit exactly one narrow enrichment:

```text
NULL stable-user projection -> verified non-NULL stable-user projection
```

This is required when a later signed semantic Workspace replica knows the stable identity that an older local row could not resolve. Every historical/provenance field remains immutable, and an already non-NULL stable projection cannot be changed.

A real schema-v14-with-data regression now covers terminal context operations, immutable rows, v15 backfill, post-migration immutability, one-time projection enrichment and derivation inheritance.

The migration also gives an explicit fail-closed error if pre-v15 data contains the same active device public key bound to multiple users, rather than surfacing an opaque SQLite unique-index failure.

## 5. Identity snapshots were not truly content-addressed

### Problem

The signed identity snapshot included `created_at = now()` inside its digest. Therefore unchanged identity state generated a different digest/snapshot row on every sync, defeating idempotency and creating unbounded evidence churn.

### Fix

Identity-continuity semantic snapshot format is now **v2**:

- volatile export timestamp removed from the signed payload;
- local `imported_at` remains the audit timestamp;
- unchanged source state produces the same digest/signature payload;
- exact already-applied projection is a no-op;
- if local projection was deliberately revoked, the identical signed snapshot can reapply it after explicit trust re-accept without duplicating immutable snapshot evidence.

The identity snapshot also no longer carries unused Domain role/policy-revision fields. Identity sync transports identity/public-auth state only; Domain policy remains under its existing authority/federation path.

## 6. Multi-Domain portable binding row collision

### Problem

A portable binding row ID was derived only from the credential descriptor digest. The same device/OIDC binding imported into two Domains could therefore derive the same global primary-key ID, despite the table correctly allowing one binding per `(domain_id, kind, binding_key)`.

### Fix

Portable projection row IDs are now deterministically scoped by:

```text
Domain ID + binding kind + binding key
```

The portable descriptor digest remains Domain-independent and continues to prove the credential descriptor itself. This keeps credential identity separate from local projection-row identity.

## 7. Identity import authorization was too broad

### Problem

A non-admin user who had accepted *some* continuity source in a Domain could satisfy a generic metadata-access helper while importing a different source that another user had accepted.

### Fix

Identity snapshot import now requires exactly:

- the stable user that accepted that exact source grant; or
- a current Domain owner/admin.

An unrelated continuity grant never confers import authority.

Read-only grant/binding inspection and authority/materialization read paths were also cleaned up so a failed read does not bootstrap a new user row as a side effect.

## 8. Workspace writer-ref wire trusted free-form principal strings

### Problem

Phase 29's writer-ref wire carried `actor: String`, such as `unix:1000`, and the remote writer authorized that target-side string. This was incompatible with the Phase 30 cross-host identity model and could make host-local labels look like remote human identity.

The fresh stricter authorization correctly broke the old tests, exposing the protocol flaw.

### Fix

Workspace writer-ref stream is now **v2**:

- Branches/Checkpoint/Fork requests carry only `actor_user_id`;
- the client resolves its current stable user ID locally;
- the remote writer reconstructs only a validated `user:usr_...` principal;
- no free-form human principal string crosses the writer-ref wire;
- the replicated Workspace snapshot already contains the safe stable user rows and Session stable owner/member projections needed for authorization.

The Phase 29 two-node smoke is green on this protocol.

## 9. Private device-key file hardening and failure cleanup

### Problems

- key load used `symlink_metadata(path)` followed by a separate open, leaving an avoidable check/open race;
- `device-create` wrote the private key before opening/validating the target store and could leave an orphan key file on an ordinary registration failure.

### Fixes

- Unix key loading now opens first with `O_NOFOLLOW | O_CLOEXEC`, then validates ownership/mode/link count on the opened descriptor;
- device creation opens the store before creating secret material;
- if registration fails normally, the just-created key file is removed;
- crash recovery stays simple: an existing key file can always be registered explicitly with `device-register`.

Private seed material remains outside ordinary SQLite and semantic replication.

## 10. CLI/help drift

### Problems

- nested resource help displayed incorrect usages such as `aw device-create ...` instead of `aw identity device-create ...`;
- identity commands inherited a generic `--domain` option even when meaningless;
- network identity commands showed both positional `DOMAIN_ID` and unrelated generic `--domain`;
- top-level `aw --help` did not list the resource namespaces at all;
- `--on` still claimed remote execution was future work;
- README and the CLI contract described Phase 23/14-era capabilities as current.

### Fixes

- one shared `nested_argv()` helper preserves full resource command paths for all resource parsers;
- identity uses a small `IdentityStoreArgs` containing only `--state-dir` and `--json`;
- continuity operations expose one explicit `--domain DOMAIN_ID` only where needed;
- network identity operations retain only their actual positional Domain ID;
- top-level help now lists Workspace/Session/Domain/Machine/Fleet/Identity/Net namespaces and common role entrypoints;
- README is rewritten around the current Phase 30 MVP mental model and happy paths;
- `.agents/docs/CLI.md` now explicitly separates implemented verbs from planned ergonomic aliases.

## 11. Node status reported stale schema metadata

### Problem

`node_status` hard-coded `"schema_epoch": 5`, a literal introduced during Phase 19 that was never advanced. That mixed up SQLite schema version with the separate durable migration-coordination epoch.

### Fix

Node status now reports both independently:

- `schema_epoch` from `schema_meta.schema_epoch`;
- `schema_version` from SQLite `PRAGMA user_version`.

No guessed/hard-coded current schema number remains in status.

## 12. Design simplifications retained

The review deliberately avoided new framework layers. The resulting rules are simpler:

1. **Cross-host human authorization always uses stable user ID.**
2. **Historical principal strings are provenance/legacy fallback only.**
3. **Rows with stable projections never fall back to raw labels.**
4. **Read/inspection paths do not create identities.**
5. **Explicit bootstrap/grant actions are the only paths allowed to create local stable users.**
6. **Trust re-accept never resurrects cached credentials; fresh signed state is required.**
7. **Semantic snapshot hashes contain semantic state, not export timestamps.**
8. **Identity sync does not carry Domain policy authority.**
9. **Remote writer-ref requests carry stable user IDs, not arbitrary principal strings.**
10. **Credential descriptor identity and Domain-local projection-row identity remain separate.**

These rules are composable with Phase 31 auth sessions/SecretStore and reduce the number of special cases future code has to understand.

## Validation status

On the reviewed tree before the final documentation-synced checkpoint:

- Phase 29 focused checker: **75/75**;
- Phase 30 focused/security checker: **142/142**;
- `cargo check --locked --workspace --all-targets`: passed;
- `aw-store`: **9 lib + 29 context + 21 resources + 28 store/journal** passed;
- default `aw-cli`: **87 + 50 + 6** passed;
- minimal: **4 + 0 + 6** passed;
- web-only: **57 + 0 + 6** passed;
- net-only: **4 + 18 + 6** passed;
- actual Phase 29 two-node handoff/writer-ref/failback smoke: passed;
- actual Phase 30 two-node identity/trust/auth/revocation smoke: passed.

Final checkpoint validation is green as well:

- full `cargo test --locked --workspace --all-targets`: passed with `AW_GIT` and `AW_BASH` explicitly bound to trusted Nix-store executables because this connector does not expose the tests' conventional `/usr/bin/git` and `/bin/bash` paths;
- static/source validation: 949 active files, no errors and no warnings;
- complete **32-step reference profile**: passed;
- final `cargo fmt --all -- --check` and `git diff --check`: passed.

The separate full-host clippy/Nix release gate remains environment-pending in this connector.
