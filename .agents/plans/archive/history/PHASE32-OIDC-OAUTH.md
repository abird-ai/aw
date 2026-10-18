# Phase 32 — verified OIDC/SSO login and OAuth connector-token custody

Phase 32 completes the next post-MVP identity/credential boundary on top of Phase 31. It adds real OIDC authorization-code + PKCE login/link and separate OAuth connector token custody, while preserving the stable-user model and keeping all bearer/client-secret material outside ordinary SQLite plaintext.

## 1. Schema v17

`017_oidc_oauth.sql` advances SQLite to **user_version 17**.

It adds:

- `user_identities.login_verified_at`
  - separates Phase-30 manually asserted OIDC metadata from locally verified login authority;
- `auth_token_families.oidc_identity_id`
  - binds a native Phase-31 refresh family to the exact verified OIDC identity that created it;
- `oidc_providers`
  - provider ID, canonical issuer, client ID/auth method, requested scopes, status and opaque/versioned client-secret metadata;
- `oidc_provider_secret_versions`
  - client-secret version history only, never raw secret bytes;
- `oauth_connector_accounts`
  - user-owned connector authorization/token metadata, independent from inference provider accounts;
- `oauth_connector_client_secret_versions`;
- `oauth_connector_token_versions`.

No OAuth access token, refresh token, OIDC client secret or OAuth client secret is stored in ordinary SQLite plaintext.

Pre-v17 OIDC rows remain **metadata-only**: migration explicitly leaves `login_verified_at` NULL. A Phase-30 manual `issuer + subject` row does not become login authority merely because the schema was upgraded.

## 2. OIDC provider configuration

New CLI namespace:

```text
aw oidc-provider create
aw oidc-provider list
aw oidc-provider rotate-secret
aw oidc-provider revoke
```

OIDC provider configuration includes:

- exact canonical issuer;
- client ID;
- `none`, `client_secret_post` or `client_secret_basic` client authentication;
- explicit scopes including `openid`;
- optional versioned client secret stored only through SecretStore.

Issuer policy:

- absolute URL;
- no username/password userinfo;
- no query parameters or fragments;
- HTTPS required except loopback HTTP for local development/test issuers;
- trailing slash canonicalization before exact issuer comparison.

Client-secret create/rotation follows the Phase-31 failure model: SecretStore write outside the SQLite write transaction, short durable transaction with revalidation/CAS, cleanup of the newly written secret on pre-commit/commit failure, old-version deletion only after successful commit.

## 3. Real authorization-code + PKCE flow

New CLI namespace:

```text
aw oidc login PROVIDER --out PRIVATE_TOKEN_FILE
aw oidc link PROVIDER
```

The CLI starts a short-lived loopback callback listener and prints the authorization URL for the user/browser.

The implementation performs:

- 256-bit random `state`;
- 256-bit random PKCE verifier;
- PKCE `S256` challenge;
- independent random OIDC `nonce`;
- loopback-only callback binding;
- exact callback Host/path validation;
- duplicate callback parameter rejection;
- exact `state` validation;
- authorization-error rejection;
- bounded callback/header/code sizes;
- one code exchange with no automatic replay.

The HTTP client follows **no redirects** for discovery/JWKS/token endpoints.

## 4. Discovery and ID-token verification

The configured issuer is resolved through:

```text
ISSUER/.well-known/openid-configuration
```

Discovery must return the exact configured issuer.

The implementation validates:

- authorization endpoint;
- token endpoint;
- JWKS URI;
- advertised code-flow support when supplied;
- advertised PKCE S256 support when supplied;
- advertised ID-token algorithm support when supplied.

ID-token validation is local and fail-closed:

- bounded compact JWT;
- only `RS256` and `ES256` accepted;
- JWKS key type/curve/algorithm/use checks;
- `kid` selects exactly one matching key when present;
- a JWT without `kid` is accepted only when there is exactly one matching signing key;
- RSA verification uses the 2048–8192-bit SHA-256 verifier;
- ES256 uses P-256 fixed-format ECDSA verification;
- exact issuer;
- exact client audience;
- `azp` when required/mismatched;
- required `exp`, required `iat`, optional `nbf`, bounded clock skew;
- exact nonce;
- bounded, non-empty subject.

Email/display-name claims are not identity inputs.

## 5. Exact stable-user mapping

The only login identity key remains:

```text
verified issuer + subject
```

`aw oidc login` behavior:

- if an already login-verified active issuer+subject exists, reuse its exact stable `user_id`;
- if no binding exists, create one new stable user + verified OIDC binding;
- if only a Phase-30 manual metadata row exists, login fails and tells the user to explicitly link while already authenticated;
- revoked OIDC identities cannot silently reactivate.

`aw oidc link` behavior:

- current local authenticated stable user is the explicit target;
- provider callback must independently verify the exact issuer+subject;
- if a manual metadata row already belongs to that user, it may be promoted to login-verified authority;
- if issuer+subject belongs to another user, fail with conflict;
- no email/display-name/username matching or merging.

The verified transition is security-audited with a digest of subject rather than raw subject in the audit metadata.

## 6. Reuse Phase-31 native sessions

OIDC does not invent another session system.

After a successful `aw oidc login`, the verified OIDC identity mints the existing Phase-31 bounded native refresh family.

The family stores `oidc_identity_id` and verify/rotate require:

- exact stable user active;
- exact stable-user security epoch;
- OIDC identity active;
- `login_verified_at IS NOT NULL`;
- configured OIDC provider still active.

Therefore explicit OIDC identity revocation or provider revocation invalidates the derived native session family.

The returned native refresh bearer is written only to a new owned private file and remains hash-only in SQLite, exactly like Phase 31.

## 7. OAuth connectors are separate from inference providers

New namespace:

```text
aw oauth-connector create
aw oauth-connector list
aw oauth-connector authorize
aw oauth-connector refresh
aw oauth-connector rotate-secret
aw oauth-connector revoke
```

OAuth connector accounts are **not** `provider_accounts` and do not affect inference payer/provider selection.

Each connector is owned by one stable user and carries:

- authorization endpoint;
- token endpoint;
- client ID/auth method;
- requested scopes;
- granted scopes;
- client-secret version/ref when applicable;
- token-set version/ref;
- access-token expiry metadata;
- configured/active/revoked/unavailable state.

Endpoints must be absolute, credential-free, query/fragment-free and HTTPS except loopback development HTTP.

## 8. OAuth authorization and token custody

OAuth authorization reuses the same loopback/state/PKCE machinery but does not require an OIDC nonce/ID token.

The local fixture and runtime independently verify PKCE before issuing tokens.

Token responses must be Bearer-token based.

The token set is serialized only into SecretStore and contains:

- access token;
- optional refresh token;
- Bearer token type;
- granted scopes;
- access expiry.

Ordinary SQLite stores only opaque version refs plus non-secret scope/expiry metadata.

Raw token structs zero access/refresh strings on drop where practical.

## 9. Scope and refresh rules

At authorization:

- granted scopes must be a non-empty subset of explicitly requested connector scopes;
- a server response claiming scopes outside the requested set is denied.

At refresh:

- the previous granted scope set is the maximum and the default when the token endpoint omits `scope`;
- refresh cannot silently widen from a narrower grant back to the original requested set;
- a returned replacement refresh token advances with the token set;
- if the provider omits a replacement refresh token, the existing refresh token remains part of the new encrypted token-set version.

Token-set rotation is CAS/revalidation fenced and SecretStore-cleaned on pre-commit/commit failure.

## 10. Secret lifecycle

OIDC client secrets, OAuth connector client secrets and OAuth access/refresh token sets all use the Phase-31 `SecretStore` abstraction.

Each secret/token class receives a distinct domain-separated store/account/kind/version opaque ref.

Creation/rotation does not hold SQLite write locks across encryption/fsync.

Version state is explicit:

```text
active -> superseded
active/unavailable -> revoked
```

Old SecretStore blobs are removed only after successful metadata commit.

No OIDC/OAuth secret metadata participates in generic Workspace semantic replication or identity-continuity sync.

## 11. Security audit

Secret-free audit events cover:

- OIDC provider creation;
- OIDC client-secret rotation;
- OIDC provider revocation;
- locally verified OIDC identity binding/login;
- OAuth connector creation;
- OAuth client-secret rotation;
- OAuth token-set version rotation;
- OAuth connector revocation.

Audit metadata includes versions/scopes/issuer/provider IDs where safe, never bearer/client-secret material.

## 12. Feature composition

OIDC/OAuth network protocol support is an explicit `oidc` feature.

- default build includes it;
- minimal/web/net configurations still compile/test independently;
- `--no-default-features --features oidc` is separately validated;
- builds without the feature keep management metadata commands but authorization/login/refresh fail clearly rather than silently doing a weaker flow.

## 13. Validation

Validated before the final .agents/docs/reference checkpoint:

- Phase 32 focused checks: **120/120**;
- Phase 31: **159/159**;
- Phase 30: **142/142**;
- Phase 29: **75/75**;
- final documentation-synced source/static validation: **967 active files**, no warnings/errors;
- `aw-store`: **29 + 29 + 21 + 28**;
- default `aw-cli`: **90 + 50 + 6**;
- minimal: **4 + 0 + 6**;
- web-only: **57 + 0 + 6**;
- net-only: **4 + 18 + 6**;
- OIDC-only: **7 + 0 + 6**;
- full `cargo test --locked --workspace --all-targets`: passed with trusted `AW_GIT` / `AW_BASH` bindings in this connector;
- actual Phase 29/30/31 binary smokes remain green;
- actual Phase 32 smoke passed with a real local 2048-bit RSA key/JWKS, discovery, authorization redirect, independent PKCE verifier, RS256 ID-token signature, nonce/state checks, exact subject link/login, native-session revocation fence, OAuth code+PKCE, encrypted token custody, refresh rotation and revoke;
- raw OAuth access/refresh tokens absent from SQLite and from SecretStore ciphertext bytes.

- complete **34-step reference profile**: passed.

The full-host clippy/Nix release gate remains separately environment-pending.

## 14. Next boundary — Phase 33

Phase 33 is the previously deferred context boundary:

> **explicit target-authorized projection of already materialized remote semantic facts into a Session/model-context operation.**

It should not create another replication system. It should consume Phase-25 materialized facts under existing Domain/user authorization and make attachment a separate explicit operation with provenance, target Session/branch/revision checks, revocation semantics, idempotency and no automatic inference/replay.

Phase 33 must preserve:

- imported evidence/materialized facts are not model context merely because they exist;
- target Session owner/member/write authority is separate from source/materialization authority;
- explicit fact selection or bounded query, no implicit “attach everything”;
- immutable provenance back to semantic event/source store/domain/materialization policy;
- target branch/head/revision/writer epoch CAS;
- revocation prevents new attachment/use but does not rewrite immutable prior history;
- no provider call during planning/attachment itself;
- no raw SQLite/WAL replication and no last-write-wins reconciliation.
