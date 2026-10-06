# Phase 30 — stable cross-host user identity and authentication continuity

Phase 30 closes the human-identity gap deliberately left by Phase 29. A replicated/promoted AwNode can now authenticate the **same stable aw user** without pretending that Unix IDs, local usernames, emails, display names, or source-host principal strings are globally meaningful.

This phase reaches the intended core distributed-app MVP architecture boundary: Domain authority, Workspace state, writer authority, and stable human user identity can all survive a host transition under explicit trust and revocation rules.

It does **not** add password login, WebAuthn/passkey ceremonies, durable rotating browser/login sessions, live OIDC authorization-code/PKCE, OAuth connector tokens, or shared provider-secret custody. Those remain explicit later security boundaries.

## 1. Schema v15

`015_identity_continuity.sql` adds only the durable projections needed for explicit identity continuity:

- `identity_continuity_grants`
  - target-side explicit trust decision for one source AwNode in one Domain;
  - accepted by one stable local aw user;
  - active/revoked state + monotonic revision;
  - revocation is immediate;
- `identity_continuity_snapshots`
  - immutable evidence of a signed imported stable-user/binding snapshot;
  - source store, source AwNode, Domain security epoch, digest and source signature;
- `portable_identity_bindings`
  - imported `device_key` or `oidc` public identity metadata;
  - stable `user_id`, source AwNode/store, user + Domain security epochs;
  - explicit active/revoked state;
  - unique `(domain_id, kind, binding_key)` prevents one portable identity from silently rebinding to a second user;
- a partial unique index prevents one active device public key from linking to multiple local users;
- stable-user projection columns are added to context plans, operations, artifacts, applications, pending attachments, source grants and derivations.

No private device key, password hash, auth-session bearer/refresh token, provider API/OAuth secret or SecretStore payload is introduced by this migration.

## 2. Stable user is the authorization identity

The stable opaque `users.user_id` is now the cross-host authorization identity.

A stable principal is represented at the runtime boundary as:

```text
user:usr_<opaque-id>
```

This is intentionally different from historical/local bindings such as:

```text
unix:1000
internal:team:alice
```

`user:usr_...` is accepted only when that exact user already exists and is active on the local authority/store. Supplying an unknown stable principal can **never create a new user**.

Existing Unix/internal principal strings remain:

- local authentication/binding labels;
- immutable node/session/audit provenance;
- backward-compatible lookup fallback for old rows.

They are no longer required to be identical across hosts for authorization.

Core Session authorization now checks stable projections:

- `sessions.owner_user_id`;
- `members.principal_user_id`;
- Domain memberships already keyed by `user_id`;
- context ownership/grants/idempotency/derivation projections added in v15.

This means a user authenticated as `user:usr_X` on a replica can read/append exactly where `usr_X` was already owner/member, even if the original source Session still says `owner = unix:1000` as historical provenance.

## 3. Context authorization continuity

Phase 30 extends the Phase 29 Workspace semantic snapshot protocol to **version 2** so stable context authorization projections survive Workspace handoff too.

The v2 snapshot additionally carries, where present:

- context artifact `owner_user_id`;
- context source-grant `principal_user_id`;
- context derivation `requester_user_id`.

The original principal strings remain preserved beside them.

Context planning, artifact ownership, source releases, pending attachments, application receipts and summary-operation idempotency now prefer stable-user authorization when available. A host-binding change therefore does not silently:

- lose access to an owned context artifact;
- invalidate a legitimate source grant;
- create a duplicate summary/application receipt;
- regenerate already-recorded work.

The Workspace replica semantic protocol is deliberately version-bumped instead of silently dropping these new authorization projections when mixed with an older implementation.

## 4. Local device-key binding

A user may explicitly register an Ed25519 device public key.

`aw identity device-create FILE`:

1. creates a fresh random 32-byte Ed25519 seed;
2. writes it only to an owned private file (`0600`-style on Unix, `O_NOFOLLOW`, no overwrite);
3. registers only the corresponding **public key** in `user_devices`;
4. advances the stable user's security epoch;
5. emits an immutable security-audit event.

The private seed is never stored in ordinary SQLite and is never included in Workspace or identity-continuity snapshots.

Existing private keys can be registered explicitly with:

```text
aw identity device-register FILE
```

and revoked with:

```text
aw identity device-revoke DEVICE_ID
```

Revocation advances the user security epoch and cannot be silently undone by resync.

## 5. OIDC identity metadata

A user may explicitly link OIDC metadata:

```text
issuer + subject
```

with an optional local provider label.

The identity key is derived from **issuer + subject**. Email, username and display name are never identity merge keys.

Two stable users cannot claim the same issuer+subject. A revoked local OIDC identity cannot be silently reactivated.

Important Phase 30 boundary:

> portable OIDC metadata is **not a live OIDC login**.

On another host, imported OIDC issuer+subject remains in `portable_identity_bindings`; Phase 30 does not manufacture a locally verified `user_identities` login row from remote metadata.

A future live OIDC authorization-code/PKCE flow must verify the issuer and subject locally before it becomes an authentication binding.

OIDC links can be explicitly revoked with:

```text
aw identity oidc-revoke IDENTITY_ID
```

which advances the stable-user security epoch. A subsequent signed continuity snapshot removes the portable OIDC binding while leaving unrelated active device keys usable at the new epoch.

## 6. Explicit target-side continuity trust

Pairing alone never imports user identity authority.

The target must explicitly accept a source:

```text
aw identity continuity-accept SOURCE_NODE --domain DOMAIN_ID
```

The source must already be a trusted paired AwNode and must have an explicit relationship to that Domain, such as:

- a current Domain owner/admin trust decision;
- the recorded control AwNode for an imported Workspace replica;
- an explicit Domain authority peer grant.

The grant is durable, inspectable and revocable:

```text
aw identity continuity-grants --domain DOMAIN_ID
aw identity continuity-revoke GRANT_ID
```

Revoking this target-side grant immediately makes previously imported portable bindings unusable for authentication. The imported snapshot remains immutable audit/evidence, but it is no longer authority.

Running `continuity-accept` again after revocation is the explicit fresh trust decision; it reactivates the same grant while incrementing its revision. Pairing, reconnecting, snapshot sync or device proof never reactivates it automatically.

## 7. Signed identity-continuity snapshot

The source exports only safe public identity-authority metadata for active Domain members:

- stable `user_id`;
- safe user display/realm/status metadata;
- current user security epoch;
- Domain role/policy revision;
- active device **public** keys;
- active OIDC issuer+subject metadata;
- current Domain security epoch.

It excludes:

- `local_auth_credentials` secret/verifier material;
- `auth_sessions`, refresh/bearer tokens or session families;
- source-host Unix/internal bindings;
- passwords or password verifiers;
- provider account secrets;
- OAuth connector tokens;
- SecretStore payloads.

The snapshot has a canonical digest and is signed by the source AwNode Ed25519 identity. The signature transcript binds:

- source store;
- source AwNode;
- Domain;
- Domain security epoch;
- snapshot digest.

Import requires:

- authenticated trusted source AwNode;
- explicit active target-side continuity grant;
- valid source signature;
- exact stable-user origin metadata on ID collisions;
- non-stale user and Domain security epochs;
- no portable binding reassignment to another user.

## 8. Typed MachineLink capabilities

Phase 30 adds two separate capabilities:

```text
identity-continuity
identity-authenticate
```

They are intentionally separate.

`identity-continuity` transports signed safe public identity metadata.

`identity-authenticate` performs a live user-device proof. Merely importing metadata never authenticates a user.

CLI carriage:

```text
aw net identity-continuity-serve PEER DOMAIN_ID ...
aw net identity-continuity-sync  PEER DOMAIN_ID ...

aw net identity-auth-serve PEER DOMAIN_ID ...
aw net identity-auth       PEER DOMAIN_ID --key PRIVATE_KEY [--session SESSION] ...
```

## 9. Live device-key authentication proof

The target creates a fresh nonce and challenge after the typed MachineLink lease has been authenticated.

The user device signs a canonical transcript binding:

- MachineLink lease ID;
- request ID;
- Domain ID;
- target AwNode;
- target store ID;
- connected peer AwNode;
- random challenge nonce;
- user device public key;
- optional requested Session ID.

The protocol uses two bounded QUIC streams because the common AwNet framing primitive intentionally finishes one stream after each framed message:

```text
stream 1: hello -> challenge
stream 2: proof -> authenticated result
```

The target verifies the Ed25519 signature against an **active imported device binding** whose:

- continuity source grant is still active;
- stable user is active;
- user security epoch still exactly matches;
- Domain security epoch still exactly matches.

If an optional Session is requested, the authenticated result is returned only after the resulting `user:usr_...` principal passes the ordinary Session authorization path.

There is no bearer token minted by this protocol and no implicit provider-account selection.

## 10. Revocation model

Phase 30 has independent revocation layers:

### Source trust revocation

`continuity-revoke` immediately stops every portable binding from that source from authenticating, even before another sync.

### Device credential revocation

Revoking the source device key advances the user's security epoch. The next signed snapshot removes the device binding; stale target copies fail epoch checks and then become explicitly revoked by refresh.

### OIDC metadata revocation

Revoking the source OIDC identity advances the user's security epoch. The next snapshot removes that OIDC binding while reissuing still-active device metadata at the new user epoch.

### Domain security changes

Portable bindings are bound to the Domain security epoch as well. Stale Domain-security generations cannot continue authenticating.

## 11. Security invariants

Phase 30 preserves these hard boundaries:

- no user equality by Unix UID/name;
- no user equality by username/login label;
- no user equality or auto-linking by email;
- no user equality by display name;
- no unknown `user:usr_...` auto-creation;
- pairing does not imply identity continuity permission;
- identity metadata import does not authenticate;
- imported OIDC metadata does not become a local verified login automatically;
- private device keys never enter ordinary SQLite or semantic sync;
- auth sessions/passwords/provider/OAuth secrets never enter identity-continuity snapshots;
- provider account ownership/payer/custody remains separate from collaboration identity;
- no silent fallback from a missing personal provider credential to a shared/server account.

## 12. Validation

Validated in the current connector environment:

- focused Phase 30 schema/source/security checks after fresh MVP review: **142 passed**;
- prior Phase 29 focused checks are **75/75** after hardening writer-ref actor identity to stable user IDs;
- `aw-store` library hardening regressions: **9/9**;
- `aw-store` context-flow: **29/29**;
- `aw-store` resource/integration: **21/21**;
- `aw-store` journal/store: **28/28**;
- default `aw-cli`: **87 + 50 + 6**;
- minimal configuration: **4 + 0 + 6**;
- web-only configuration: **57 + 0 + 6**;
- net-only configuration: **4 + 18 + 6**;
- actual compiled-binary two-node Phase 30 smoke: passed;
- the native smoke proves stable user continuity, explicit source trust, signed public metadata sync, live device-key challenge/response, replicated Session authorization, source-grant revoke/re-accept requiring fresh signed resync, OIDC metadata revocation, device revocation/security-epoch fencing, and no private-key replication;
- full `cargo test --locked --workspace --all-targets`: passed with `AW_GIT` and `AW_BASH` explicitly bound to trusted Nix-store executables because the connector sandbox does not expose the tests' conventional `/usr/bin/git` and `/bin/bash` paths.

The final source/static and reference-profile gates are rerun on the exact documentation-synced tree before checkpointing.

The separate full-host clippy/Nix release gate remains environment-pending because `cargo-clippy` is unavailable in this connector and `/nix/store` is read-only.

## 12.1 Fresh-review hardening addendum

A clean-room post-Phase-30 review found and fixed several issues that the original feature tests did not cover: stale portable credential revival after trust re-accept, inactive stable-user fallback through historical principal strings, a real v14→v15 immutable-context migration failure, cross-host artifact owner-label collisions, multi-Domain portable-binding row-ID collisions, unrelated-grant import authority, non-content-addressed identity snapshots, read paths that created identities, Workspace writer-ref free-form principal transport, private-key TOCTOU/orphan-file behavior, stale node schema status and CLI/help/documentation drift.

The hardened identity semantic snapshot is **version 2** and omits volatile export time; unchanged state has a stable digest. Workspace writer-ref transport is also **version 2** and carries `actor_user_id`, never a free-form Unix/internal principal. Continuity trust re-accept restores trust only; a fresh signed identity snapshot must reapply portable credentials.

See [Phase 30 fresh review](PHASE30-FRESH-REVIEW.md) for the exact findings, fixes and maintenance rules.

## 13. MVP boundary reached

With Phase 30, the core distributed-app MVP architecture is complete:

- stable AwNode identity and real-world route fallback;
- typed MachineLink authorization;
- remote execution and Fleet dispatch;
- Domain federation/quorum/failover;
- Workspace writer epochs and semantic replica handoff;
- explicit partition divergence preservation;
- stable human `user_id` continuity and revocable cross-host authentication.

Future numbered phases are now post-MVP security/product/runtime expansion rather than prerequisites for the core distributed architecture.

## 14. Next boundary — Phase 31

Follow the adopted DB-D ordering with **native auth-session and SecretStore/provider credential custody hardening**:

1. define a real `SecretStore` interface/backend boundary for shared/server credentials so raw secrets remain outside ordinary SQLite plaintext;
2. make `provider_accounts.secret_ref` / version metadata point only to opaque SecretStore handles;
3. preserve personal user-device/browser provider custody and strict no-fallback semantics;
4. add explicit shared/server credential creation, rotation, revocation and security audit without exposing raw secret values through logs/sync/export;
5. make the existing `local_auth_credentials` / `auth_sessions` schema into a real bounded native auth-session runtime with hashed rotating session material and security-epoch invalidation;
6. device/public-key authentication should feed that session layer without weakening the Phase 30 stable-user proof model;
7. do not yet claim full OIDC/SSO login; authorization-code + PKCE and OAuth connector tokens remain a subsequent Phase 32-style boundary;
8. keep materialized remote-fact -> model-context projection separately target-authorized;
9. keep direct multi-process SQLite behind execution/migration lease and WAL/CAS stress gates.
