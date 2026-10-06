# Phase 31 — native auth sessions and SecretStore/provider credential custody

Phase 31 is the first post-MVP security/product phase after the reviewed Phase 30 distributed architecture boundary. It activates two durable planes that had existed only as schema placeholders since Phase 3:

1. bounded rotating native authentication sessions; and
2. explicit shared/server provider credential custody behind a SecretStore interface.

The phase deliberately does **not** change the Domain/Workspace/Session hierarchy, AwNet authority model, stable-user identity rules, personal provider-custody model, or single-SQLite-owner design.

Raw shared provider credentials and native refresh tokens are never stored in ordinary SQLite plaintext.

## 1. Schema v16

`016_auth_secret_store.sql` advances SQLite to **user_version 16** and adds the minimum durable metadata required around the existing `auth_sessions`, `local_auth_credentials`, and `provider_accounts` tables.

### `auth_token_families`

One bounded native login/refresh family:

- stable `user_id`;
- optional local `credential_id`;
- optional Phase-30 `portable_binding_id` for device-proof-derived sessions;
- authentication method;
- active/revoked state;
- exact current auth-session ID;
- user `security_epoch_at_issue`;
- fixed family creation/expiry times;
- last rotation time;
- token-reuse detection time;
- revocation time;
- bounded non-secret client metadata.

`auth_sessions` gains:

- unique `refresh_token_hash`;
- `rotated_to_session_id`;
- `rotated_at`;
- family lookup index.

Pre-v16 `auth_sessions` were dormant metadata only. Migration marks every old session revoked rather than treating an unknown historical bearer token as a live Phase-31 session.

### `provider_account_grants`

Explicit stable-user grants for shared/server provider accounts:

```text
use
admin
```

A shared account always begins with the creator as an active admin. The last active admin cannot be revoked.

### `provider_secret_versions`

Versioned secret metadata only:

- provider account ID;
- positive version number;
- opaque SecretStore reference;
- active / superseded / revoked / unavailable status;
- optional creating stable user;
- creation/retirement timestamps.

The table never contains raw secret bytes.

### Migration behavior

Legacy `server_secret` account metadata is not trusted automatically:

- active-looking legacy server-secret rows become `unavailable`;
- already revoked rows remain revoked;
- a missing/non-positive legacy version becomes version 1 only as metadata provenance;
- legacy refs are recorded as unavailable/revoked evidence, not assumed to exist in the new SecretStore backend;
- duplicate dormant refresh-token hashes or duplicate legacy server-secret refs fail the migration before v16 is committed.

The result is fail-closed: upgrading metadata never creates a working credential or bearer session by assumption.

## 2. SecretStore abstraction

Phase 31 introduces the small runtime boundary:

```text
SecretStore
  put(ref, secret)
  get(ref)
  delete(ref)
  exists(ref)
```

Ordinary SQLite knows only opaque refs and versions.

Two implementations exist:

- `LocalSecretStore` — real persistent encrypted local backend;
- `MemorySecretStore` — write-once test backend with the same logical semantics.

This interface is deliberately narrow so a future OS keychain, KMS/HSM, managed secret service, or organization vault can implement the same contract without changing provider-account authorization or receipts.

## 3. Local encrypted SecretStore backend

The persistent backend lives under the private aw state directory:

```text
STATE/
  sessions.sqlite3
  secrets/
    master.key
    items/
      sec_<opaque>.bin
```

Security properties:

- `secrets/` and `items/` must be owned real directories with no group/other permissions (`0700` style on Unix);
- `master.key` is an owned, single-link, private regular file (`0600` style);
- secret files are opened/created with no-follow / close-on-exec behavior;
- values are bounded to 64 KiB;
- secret refs are opaque deterministic handles scoped by local store ID + provider-account ID + version;
- blobs use ChaCha20-Poly1305 authenticated encryption;
- the opaque secret ref is authenticated as AEAD associated data;
- each blob uses a fresh random nonce;
- credential-version blobs are write-once;
- writes are fsynced and published with create-new/link semantics so an existing version cannot be silently overwritten;
- master-key material and decrypted secret values use zeroizing memory where practical;
- loss of `master.key` while encrypted items still exist fails closed — a new key is **not** silently generated over recoverable encrypted state.

Disk regressions verify that plaintext provider credentials do not occur inside the encrypted blob.

The local backend currently requires Unix private-file semantics. That limitation is explicit; it does not silently downgrade security on other platforms.

## 4. Shared/server provider-account model

A durable Phase-31 shared account is explicit metadata plus an external secret version:

```text
provider account
  provider kind
  pinned base URL
  payer
  local executor
  allowed models
  explicit user grants
  secret version N
  opaque secret ref N
```

The raw bearer/API key is absent from account metadata, receipts, history, ordinary config, semantic sync, and audit metadata.

### Creation

```text
aw provider-account create ACCOUNT \
  --provider openai \
  --base-url https://api.openai.com/v1 \
  --payer TEAM \
  --model MODEL \
  --secret-file PRIVATE_FILE
```

Creation:

1. validates account/provider/model/executor/endpoint policy;
2. creates/resolves the local actor's stable user;
3. releases the SQLite write lock;
4. encrypts/fsyncs a new immutable SecretStore version;
5. reacquires a short SQLite transaction;
6. revalidates identity/account nonexistence;
7. writes account metadata, version metadata, creator admin grant and security audit;
8. removes the newly written SecretStore blob if metadata activation fails.

SQLite write transactions are therefore not held across encryption/fsync.

### Endpoint pinning

Shared credentials are pinned to the exact canonical provider base URL selected by the account administrator.

The endpoint:

- must be an absolute URL;
- cannot contain username/password userinfo;
- cannot contain query parameters or fragments;
- must use HTTPS, except loopback HTTP for local development/test services;
- is rechecked before account selection **and again immediately before secret release**.

This prevents a user with a legitimate `use` grant from redirecting a shared provider bearer to an attacker-controlled `--base-url`.

## 5. Explicit grants and no fallback

Shared credentials are never implicitly available to every authenticated user.

Commands:

```text
aw provider-account list
aw provider-account grant ACCOUNT USER_ID --role use|admin
aw provider-account grants ACCOUNT
aw provider-account revoke-grant ACCOUNT USER_ID
```

Authorization uses stable `user_id`, not Unix username/email/display name.

A `use` or `admin` grant authorizes only that exact account. The selected account still must match:

- active status;
- server-secret custody;
- local executor;
- pinned endpoint;
- allowed model;
- exact active credential version.

There is **no fallback** to another shared account, a personal account, environment API key, or profile default when any of those checks fails.

This applies even when a profile/environment account uses the same textual account ID: once an explicitly selected durable provider account exists, a missing grant/revoked/unavailable version fails instead of falling through to the profile credential.

## 6. Secret resolution at dispatch time

A durable provider account is registered into the transient agent registry only when `--account ACCOUNT` explicitly selects it for the current actor.

The transient `AccountDescriptor` contains:

- account ID;
- shared payer;
- server custody/executor;
- allowed models;
- actor-specific grant;
- exact `credential_version` provenance.

It contains no secret value.

The SecretStore provider wrapper revalidates the durable account on **every outbound provider dispatch**, not only at agent startup:

```text
actor grant
account active
provider kind
model
endpoint
executor
credential version
secret-version row active
SecretStore blob available/authenticated
```

Only then is the bearer decrypted into a short-lived zeroizing in-memory value and applied to the HTTP request.

The serializable provider `Profile` remains secret-free. The SecretStore dispatch path explicitly clears `api_key_env` before constructing the HTTP provider, so a selected durable account can never mix its credential with an environment credential.

If the selected SecretStore blob is missing/unreadable, that account/version becomes `unavailable`, the failure is audited, and dispatch fails before provider network activity. No fallback is attempted.

## 7. Credential rotation

```text
aw provider-account rotate ACCOUNT --secret-file PRIVATE_FILE
```

Rotation:

1. requires a current admin grant;
2. snapshots current account/version/ref;
3. releases SQLite;
4. writes the new immutable SecretStore version;
5. reacquires SQLite and revalidates account/admin/version/ref;
6. CAS-advances metadata to version `N+1`;
7. marks version `N` superseded;
8. records immutable audit metadata;
9. deletes the old SecretStore blob after successful metadata commit.

If another change wins the race, the new blob is removed and the caller gets a conflict instead of silently overwriting newer account state.

Long-lived agent processes pin version `N` in invocation identity. After rotation, their next attempt to use `N` is denied; callers explicitly reselect/rebuild against version `N+1` rather than transparently changing payer/credential provenance inside an already-selected operation.

## 8. Provider-account revocation

```text
aw provider-account revoke ACCOUNT
```

Revocation:

- requires admin authority;
- marks the durable account revoked;
- marks all secret versions revoked;
- records security audit metadata;
- deletes local SecretStore blobs on a best-effort cleanup basis;
- causes every future explicit use to fail before provider dispatch;
- never falls back to a profile/environment account.

## 9. Native refresh sessions

Phase 31 makes `auth_sessions` an operational runtime rather than dormant schema.

Refresh tokens:

- use 32 bytes / 256 bits of OS randomness;
- are emitted as opaque `awrt_...` bearer strings;
- are held in zeroizing memory where practical;
- are stored by the CLI only in new private token files;
- never appear in serializable session metadata;
- are persisted in SQLite only as a domain-separated SHA-256 hash.

Hashing is appropriate here because the tokens themselves are uniformly random high-entropy bearer values; this is not password hashing.

### Bounded lifetime

Native families have a fixed maximum lifetime:

```text
minimum TTL: 60 seconds
maximum TTL: 30 days
```

Rotation changes the bearer, not the family's absolute expiry. Refreshing therefore cannot extend a family forever.

### Local issue / verify

```text
aw auth issue --out PRIVATE_TOKEN_FILE [--ttl SECONDS]
aw auth verify PRIVATE_TOKEN_FILE
aw auth list
```

`aw auth issue` currently represents a trusted local Unix-authenticated CLI boundary. Passkey/password ceremonies are deliberately not invented in this phase.

The token-output path is checked before session creation; ordinary output failure revokes the newly created family rather than leaving an inaccessible live token.

## 10. Rotation and reuse detection

```text
aw auth rotate OLD_TOKEN --out NEW_PRIVATE_TOKEN
```

Rotation creates a new session row in the same family, marks the previous row rotated/revoked, and moves `current_session_id` exactly once.

Using a rotated token again is treated as bearer-token reuse:

```text
old token reused
  -> record reuse_detected_at
  -> revoke whole family
  -> revoke every session row in family
  -> new token also stops working
```

This is intentional fail-closed replay detection.

The output path is preflighted before rotation. If writing the newly rotated token still fails unexpectedly, the new family is revoked rather than leaving an inaccessible active credential.

Logout/revoke can be authorized by possession of the current refresh token itself:

```text
aw auth revoke PRIVATE_TOKEN_FILE
```

A stable user may also inspect/revoke their own family metadata through store APIs without exposing bearer material.

## 11. User security-epoch fencing

Every family/session captures the stable user's `security_epoch` at issue time.

Verify and rotate require exact equality with the current active user epoch. If user credential/revocation state advances the epoch, the family is revoked and cannot refresh.

This reuses the Phase 30 revocation model instead of introducing a parallel account-generation mechanism.

## 12. Phase-30 device proof → Phase-31 native session

The typed MachineLink `identity-authenticate` flow is upgraded to wire version **2**.

The user may request a bounded native session during a successful device-key proof:

```text
aw net identity-auth PEER DOMAIN_ID \
  --key PRIVATE_DEVICE_KEY \
  --auth-session-out PRIVATE_TOKEN_FILE \
  --auth-session-ttl SECONDS \
  ...
```

The server independently caps issuance with `--auth-session-max-ttl`.

The device-auth challenge transcript binds the requested auth-session TTL in addition to the existing lease/request/Domain/target-store/peer/nonce/public-key/optional-Session fields, so a network intermediary cannot add or alter session issuance after the device signature.

A native family minted from a portable device proof stores the exact imported `portable_binding_id`.

Every verify/rotation then rechecks:

- portable device binding still active;
- stable user still active and exact user security epoch;
- Domain still active and exact Domain security epoch;
- identity-continuity source grant still active.

Therefore continuity-source revocation or device-binding revocation invalidates the derived native auth family as well.

The new refresh token travels only inside the already authenticated/encrypted QUIC identity-auth reply. It is skipped from ordinary serialized result output; the client moves it into zeroizing memory and writes it directly to a new private file. The server zeroizes its temporary wire copy immediately after sending. If the reply cannot be delivered, the newly minted family is revoked.

## 13. CLI surfaces

Shared provider custody:

```text
aw provider-account create
aw provider-account list
aw provider-account rotate
aw provider-account grant
aw provider-account grants
aw provider-account revoke-grant
aw provider-account versions
aw provider-account revoke
```

Native sessions:

```text
aw auth issue
aw auth verify
aw auth rotate
aw auth revoke
aw auth list
```

Raw provider secrets are accepted only from owned private files. Refresh tokens are written/read only through private files. Neither is accepted as a normal command-line string argument.

## 14. Deliberate exclusions

Phase 31 does **not** implement:

- password enrollment/verification ceremonies;
- passkey/WebAuthn registration/assertion ceremonies;
- browser cookie/session middleware;
- live OIDC authorization-code flow;
- PKCE/state/nonce callback handling;
- OIDC discovery/JWKS validation;
- OAuth connector authorization or refresh-token custody;
- automatic account linking by email;
- hosted/KMS/HSM SecretStore backends;
- SecretStore replication through Workspace/identity semantic sync.

The schema still has room for local passkey/password credentials, but Phase 31 does not pretend those authenticators exist until their real ceremonies are implemented and tested.

## 15. Validation

Validated on the Phase 31 implementation tree:

- Phase 31 focused schema/security/architecture checks: **159/159**;
- Phase 30 focused checks remain **142/142**;
- Phase 29 focused checks remain **75/75**;
- `aw-store` library/unit tests: **22/22**;
- `aw-store` context-flow tests: **29/29**;
- `aw-store` resources/distributed tests: **21/21**;
- `aw-store` durable store/journal tests: **28/28**;
- default `aw-cli`: **87 + 50 + 6**;
- minimal: **4 + 0 + 6**;
- web-only: **57 + 0 + 6**;
- net-only: **4 + 18 + 6**;
- `cargo check --locked --workspace --all-targets`: passed;
- full `cargo test --locked --workspace --all-targets`: passed with the connector's explicit trusted `AW_GIT` / `AW_BASH` paths;
- actual Phase 29 Workspace handoff/writer-ref/failback binary smoke: passed;
- actual Phase 30 identity continuity/device-proof/revocation binary smoke: passed;
- actual Phase 31 SecretStore/provider dispatch/native-session/device-proof bridge smoke: passed;
- final documentation-synced static/source validation: **958 active files**, no errors/warnings;
- complete **33-step reference profile**: passed.

The separate full-host clippy/Nix release gate remains environment-pending in this connector because `cargo-clippy` is unavailable and `/nix/store` is read-only.

## 16. Next boundary — Phase 32

The next security/product boundary is **real OIDC/SSO login plus OAuth connector-token custody through SecretStore**.

Phase 32 should:

1. implement OIDC authorization-code + PKCE with exact `state` and `nonce` verification;
2. perform discovery/JWKS/issuer/audience/signature validation locally at the authority that accepts the login;
3. link only exact verified `(issuer, subject)` to stable `user_id`; never auto-link or merge by email/display name;
4. use the Phase 31 native-session family runtime after successful OIDC authentication rather than inventing a second browser-session model;
5. introduce OAuth connector accounts/scopes separately from provider inference accounts;
6. store OAuth access/refresh tokens only through SecretStore opaque/versioned refs, never generic semantic sync or ordinary SQLite plaintext;
7. implement token rotation/revocation/error handling without silent credential/provider fallback;
8. preserve personal browser/device custody versus server/shared custody explicitly;
9. keep materialized remote-fact → model-context projection as another separately target-authorized later boundary;
10. keep direct multi-process SQLite behind execution/migration leases and adversarial WAL/CAS stress gates.
