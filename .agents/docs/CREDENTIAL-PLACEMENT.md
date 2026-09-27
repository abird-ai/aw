# Credential ownership and execution placement — Phase 3

## Current Phase 8 refinement — explicit source bridge

The earlier phase sections below are retained history. Phase8 now supplies the narrow `aw-inference` Bridge/DeviceWorker, local CLI grant/profile/ledger and private Unix transport, optionally carried by independently configured SSH. The owner keeps the conversation/tool loop; a device uses its own provider credentials. No browser key upload/vault is added. This is source implementation, not compile/run validation or generic remote multi-user hosting.

[Device contract](DEVICE-INFERENCE.md) and [wire protocol](DEVICE-PROTOCOL.md) supersede earlier statements that no device adapter exists. All malicious-origin, same-UID, local-spending-oracle and separate tool-host caveats still apply. The shared pairing capability is not a provider credential. Users explicitly choose the personal account; the old `attach` path itself still does not load a local model configuration.


**Architectural contract plus explicit source status.** Phase 3 retains typed account selection/local HTTP and adds durable secret-free invocation receipts. A remote collaboration service with user-device inference, browser-held credential vault, native companion bridge, multi-user login is **not yet implemented**. No browser API key entry or upload route exists in the shipped web owner.

## 1. The core decision

A shared conversation owns history, not a credential account. Each admitted prompt binds an authenticated actor to one allowed provider account and model. That binding is fixed through its model/tool loop and recorded on each actual model invocation. Another user continuing the same history supplies their own binding; neither user inherits the previous caller's key, payer, model preference or execution authority.

Keep these dimensions independent:

| Dimension | Example | Authority |
|---|---|---|
| UI / collaboration host | Remote team aw server | Serves authorized session state; need not possess a personal model key. |
| Human identity | Alice authenticated to the team | Authentication and session ACL, not a user string supplied in a prompt. |
| Credential custody | Alice's laptop | Local configuration/vault owned by Alice. |
| Inference executor | Alice's local aw process | Makes the provider HTTP call; endpoint/auth known here. |
| Model destination | Alice's local Ollama, or a permitted hosted API | Context disclosure must be approved for that destination. |
| Tool executor | A designated local or remote runner | Separate filesystem/process capabilities; browser location does not choose it. |
| Usage initiator and payer | Alice / Alice, or Alice / team budget | Explicit account grant and per-invocation receipt. |

`localhost` is relative to the process making that connection. The remote team server cannot use its own `localhost` to reach Alice's Ollama. A browser fetch reaches the browser device, while a Rust provider reaches its executing device. Render this distinction rather than silently proxying through another machine.

## 2. Supported account modes

### Personal, local Rust/CLI — implemented source

The local host establishes `PrincipalId("unix:<effective uid>")`; `AccountDescriptor::personal` records that actor as owner/payer and keeps the credential custody/inference executor at the local device label. `LazyHttpProvider` reads the selected configuration's credential environment reference only when making an actual request. Credential values do not enter the core descriptor, transcript, event, invocation identity or tar.

UID is locally meaningful, not a global/federated identity. A future team identity must link verified issuer-qualified accounts, not equate the same UID or username on unrelated machines. The SDK accepts host-established principals but does not authenticate arbitrary principal strings itself.

### Explicit common/server account — implemented source

A `Server`-owned descriptor has a `Shared` or otherwise explicitly authorized payer and either an explicit principal allow-list or an explicit `shared_with_all_authenticated` setting. Empty grants deny all callers. There is no fallback from a missing/disallowed personal account to a common one. Personal accounts cannot accidentally enable all-user sharing or be tagged as server-custodied. A server-owned common account must explicitly declare server custody. Trusted host labels still are not cryptographic proof of physical execution.

The current `aw web` owner uses its startup profile as a **server-held** account granted to its one paired browser principal. `--shared-profile NAME` explicitly adds another configured account/model for that same principal. This is a small local owner, not a working team directory or multi-user account-management UI. The owner, not the browser, chooses endpoint and credential reference. `/api/accounts` returns only permitted non-secret choices.

### Remote workspace with personal inference on the user's device — required next integration

```text
Remote aw collaboration owner                  User's trusted local aw
  authorized session snapshot  ----------------> local admission/consent
  run/node IDs, context, tools schema             local account grant
                                                   |
                                                   v
                                                model endpoint
                                             (key stays on device)
                                                   |
  response/tool proposals/usage <------------------+
  tool result with provenance -------------------> next invocation
```

A provider call can be delegated without uploading its secret. The local adapter resolves an opaque local account ID; the remote request does NOT supply the API key, credential environment-variable name, arbitrary endpoint, executable path or unrestricted HTTP headers. The local executor independently checks caller/session, model/destination, context release, max usage, allowed tools, deadline, and idempotency.

The bridge must be an authenticated, explicitly paired connection with replay protection and local revocation. Prefer a local process making an outbound authenticated connection to an approved server, or a locally trusted UI/extension using a specifically paired local component. Do not expose an unauthenticated localhost inference proxy. Limit registered device/account handles and capabilities per authenticated principal.

Phase 2 `ProviderAccounts` deliberately refuses a descriptor for a different executor and requires custody to match its concrete execution registry. It therefore cannot silently treat a remote handle as a local secret. A future delegated-provider adapter needs its own acknowledged run/lease state machine. Metadata types alone are not an implementation of that bridge or cryptographic attestation.

## 3. Browser-only mode is a different trust claim

A browser may eventually make a direct model request where the provider supports browser CORS/auth and the user explicitly opts into that mode. Provider tokens should default to memory-only, with no inclusion in app API requests, logs, analytics, URLs, service-worker caches or synchronized configuration. Optional encrypted-at-rest storage requires a reviewed vault design and explicit unlock/lifetime behavior.

**localStorage, sessionStorage and IndexedDB are not secret enclaves against same-origin JavaScript.** Code served by the workspace host executes under that origin. A malicious/compromised host can ship JavaScript that reads a key when entered/unlocked or invokes a credential-holding component. Encrypting at rest does not remove that runtime trust. A nonextractable CryptoKey may restrict raw export but still permit authorized-origin code to request cryptographic use. Browser extension/WASM/service-worker packaging does not automatically fix a compromised trust root.

Accordingly distinguish two promises:

1. **No intentional credential upload in the application's protocol.** Possible with direct-browser requests, but the user still trusts the delivered UI code and its dependencies.
2. **The collaboration server cannot obtain the personal provider secret even if it serves malicious page code.** This needs a separately trusted local boundary: native aw/OS vault and trusted confirmation, an independently hosted/signed installed client or a reviewed isolated extension integration. The remote origin must not receive a secret-export API.

Even a non-exporting local companion can become a spending oracle if it honors every malicious page request. It must enforce local destination/model/budget/approval grants rather than trusting a same-origin string or broad bearer token as unlimited consent. Keys staying local does not mean prompts/results are hidden from the collaboration server: shared transcript privacy and end-to-end encryption are a separate design not shipped here.

Browser-to-local network requests also face browser-specific permission/mixed-content/CORS rules. No universal seamless remote HTTPS → localhost access is promised. Current web CSP is deliberately same-origin; this phase does not broaden it or collect client keys.

## 4. Future secret-free delegation envelope (design, not a shipped endpoint)

A task carries `request_id`, `attempt_id`, authenticated principal/session/branch binding, expected session revision, registered executor/account handle, granted model, authorized context manifest and typed tool schemas, budget/deadline, request digest and local grant version. The local side verifies the manifest and capabilities before reading a secret or invoking a provider. It returns a signed/authenticated receipt, incremental content, complete tool proposals, provider-reported usage and a clear completion/unknown state.

Tools remain mediated by their own executor. The model's proposed tool call cannot choose arbitrary hosts or become shell execution just because a local model generated it. A local inference call may request a remote tool only through a separately granted remote runner and validated result channel. No executable strings from the server are interpreted as local bridge protocol instructions.

Duplicate transport messages must not create a second paid call. Before an uncertain retry, query the owning executor receipt. Disconnect does not mean remote inference or billing stopped. If the user device is offline, wait/deny/ask for an explicit alternative account; never silently spend the team account. Scheduled work using personal device credentials cannot run while that device/executor is unavailable unless the user explicitly configured another permitted mode.

At shared-history admission, two users' prompts keep their authored parent/revision. Distributed model responses append through the authoritative session writer, not by inventing an independent fork store. Phase 3 persistent state must store account/executor/receipt provenance without credential material.

## 5. Accounting and authority in Phase 3

`InvocationIdentity` records actor, selected account, owner, payer, custody site, inference executor, model and grant revision. `ToolRecord` independently records its execution site. `InvocationRecord` records every attempt to dispatch a model round, including failed/cancelled attempts with unknown usage. An account is revalidated before each subsequent model or tool action; revocation does not undo an already-dispatched operation.

`usage_for(turns, actor)` filters in-memory invocation records by initiator and reports only available provider token counts; incomplete usage stays unknown. Phase 3 adds native durable invocation receipts and actor-filtered usage; there are no currency estimates, payment settlement, verified invoices or enforced monetary budgets. A future client-origin usage receipt is evidence from that client/provider, not a server-verified bill. Actor-filtered native store queries count each dispatched receipt once across forks. Imported histories do not create new native billing rows. Additional accounting/retention queries remain incremental; do not duplicate every historical call under its descendants.

`Agent::prompt_as` assumes the embedding host already checked authentication, session read/append ACL, shared-context disclosure, and account grants. It is a trusted native SDK API, not an auth service. Passing an `Arc<dyn Provider>` or labeling it a device does not sandbox native code or prove where it really runs. A compromised trusted host can violate its own labels.

## 6. Tool-related credential caveat

Scoped file tools still disclose allowed file contents to the selected model. Do not grant a workspace containing secrets that must not be sent. `trusted` bash runs with the owner's OS authority: clearing its environment is useful but does not isolate ~/.config, /proc, keychains, network or other same-user processes. Protecting personal credentials from untrusted code requires OS/container/runner boundaries, not only account structs or a guardian model.

## 7. Acceptance before shipping remote personal credentials

Test two authenticated users following the same branch with independent local accounts and explicit team accounts; inspect network and serialization paths for credential leakage. Exercise device offline, local approval refusal, replay/ambiguous provider completion, revocation mid-loop, hostile host-served JS, malicious bridge payloads, endpoint substitution, prompt-output injection, local-shell access, model/schema mismatch and inaccurate/missing usage. A key never sent by protocol is not proof that the served UI cannot steal it.

Sources consulted 27 September 2026: OWASP HTML5 Security Cheat Sheet (storage and messaging); MDN Local network access. See [SOURCES.md](SOURCES.md). These inform constraints; no external security audit or completed distributed executor is claimed.


## Phase 3 persistence boundary

Stored branch history includes prior actor/account/model/payer/custody/executor labels and optional usage, not a provider instance, key value, credential env/file reference or execution grant. Resume resolves the current host configuration and validates binding before a new prompt. Current stored membership is separate from provider grants. Import creates new review-gated history with unverified source provenance, not a re-registration of foreign accounts. The concrete remote personal-device bridge remains future work; no browser provider vault/key upload was added.


## Phase 4 integration note

The optional native workspace uses the same current account/tool grants and persistent owner described above. The underlying Phase3 SQLite schema and browser account/transport behavior are unchanged. Human file views and layout changes do not enter provider context, start another loop, restore credentials from history or grant permission. Source0.5.0 adds a separate pure layout SDK and native rendering; no new remote hosting, browser key store, inference delegation, Git or embedded PTY is implied. See [WORKSPACE.md](WORKSPACE.md) and [BUILD-STATUS.md](BUILD-STATUS.md).


## Phase 5 native Git/terminal addition

The new Git/PTY services do not register providers or change invocation accounts. Human Git reads and terminal output are not included in prompts by opening/moving tabs. Worktree review selection does not move the Agent tool root or credential executor. Same-root `send-review` is an explicit unsent text insertion, not permission expansion.

A human interactive shell still has the native OS user's file/process/network authority. Excluding token environment variables/rc files is not credential confinement; intentionally executed same-UID commands may access locally stored credentials. These terminals are not remotely exposed through the paired web API. Local-user inference delegation and the strong malicious-remote-server secrecy boundary remain separate, unimplemented work.

## Phase 6 addition — views do not move credentials or inference

Agnostic browser groups and human Files/Git/PTY endpoints do not change the account/credential contract above. No new request schema accepts provider keys, actor IDs, env names, inference URLs or account grants. Website URLs are host-configured frame targets, not inference destinations or fetch-proxy requests. Site login cookies are not provider accounts or aw authentication.

The served owner is still numeric-loopback/single-paired-principal. It may hold explicitly configured common accounts, while CLI uses its local profiles. A remote workspace with an authenticated personal-device model executor remains unimplemented. Browser localStorage is not introduced as a secret vault. Opening a file/site, changing groups or typing into a terminal causes no model call unless the user separately submits an authorized prompt.

A browser shell with owner OS authority can access other same-user resources even when environment is cleared. This is a distinct high-trust capability, not an enforcement mechanism for keeping secrets from untrusted commands. A future remote deployment must not reuse it as an unreviewed user-device credential bridge.


## Phase 7 local owner and attach — no key transfer

`aw serve` keeps the selected provider configuration at the owner, while `aw attach` connects before loading ANY local provider configuration or state store. An explicit account/model choice uses a permitted owner-registered account. Current actor, payer, custody and inference/tool locations remain bound through the existing SDK and journal. The native protocol contains no credential, endpoint, environment-reference or actor-setting field.

This is a same-user local control client, NOT the promised separately trusted personal-device inference executor. On a different machine, a team server still cannot use its own localhost as the user's model server. A future paired/outbound executor must perform its own model/context/spend admission and expose no secret-export API. Neither a local Unix socket nor browser pairing is a substitute for that remote authorization contract.

`serve --serve-web` deliberately grants its paired browser the launching Unix-owner role. The unchanged `aw web` mode retains browser:paired instead. Do not treat this opt-in same-user grant as automatic credential inheritance for arbitrary users. Browser logout in shared-owner mode revokes that UI but does not cancel a native-owned task; explicit account/tool grants and cancellation remain separate.

[Local owner](LOCAL-OWNER.md) · [Local protocol](LOCAL-PROTOCOL.md).

## Phase 9 refinement — named private participants

`aw team serve` maps each authenticated invitation/cookie to an independently scoped `internal:<realm>:<id>` controller. A participant may be granted common owner-held profiles and/or own personal-device links. The default may be entirely delegated: no dummy server credential is required. Neither shared profile availability nor a browser identity grants the personal device's local model consent.

The local worker's finite grant must name the new exact internal actor and owned session/branch IDs. A Phase8 grant for `unix:...` does not silently convert into team authority. Each personal account/link/socket/pairing path is uniquely assigned; common server accounts are a separate explicit grant with shared payer metadata. All users still select an authorized account per prompt, and denied/offline personal calls never fall back.

Team login introduces no personal key upload, token storage in browser, model endpoint override or remote/local tool bridge. In fact, all tool and human workspace surfaces are disabled for this first team slice. Root/UID/trusted server code can still access host-side configuration/history, and shared transcript encryption is not implemented. [Team identity](TEAM-IDENTITY.md) explains the private-only scope and process lifecycle.

## Planned branch-summary execution

Context-flow generation must use the requester's currently permitted account, not the account recorded on source answers. Source-read and inference-destination consent precede summarization. Native/local-device requests need a versioned summary-purpose and source-manifest contract; the existing ordinary target-session grant must not be treated as unrestricted permission to harvest unrelated histories. [Full proposal](CONTEXT-FLOW.md). This amendment adds no secret upload, browser vault, model invocation or device protocol feature.

## Phase 10 summary and source release

The requester explicitly selects the summarizer account/model. Source authors' provider clients, credential references and usage accounts are never restored into that selection. Import/reuse charges no additional generation. Every actual synthesis operation is separately receipted; missing usage remains unknown.

A peer source requires its owner's current read release and explicit recipient-account allow-list for provider use. Later ordinary prompts consuming derived facts revalidate the current main account against those source releases. A source grant is not a provider-account grant: the requester must already be entitled to the account.

Personal-device summaries use wire2 and exact locally approved input digests in a grant2. The target-session chat scope alone is insufficient. Human/model text, source identifiers, permission epochs and opaque account IDs do not supply keys. Browser storage still contains no model credentials and no secret-upload route is added. [Source/summary implementation](CONTEXT-FLOW-IMPLEMENTATION.md).

## Database architecture v2 — credential custody refinement

The durable database should store **secret-free provider-account metadata**, including owner/payer/custody/executor/model-policy and an opaque secret version/ref where a common/server credential exists. Raw common credentials move behind a `SecretStore` abstraction rather than ordinary SQLite rows. A DB restore without its SecretStore makes that account unavailable; it never substitutes another credential.

Personal credentials retain the existing local-device rule. A future browser-owned credential may use encrypted IndexedDB/non-exportable WebCrypto wrapping where feasible, but same-origin code able to use/decrypt that credential remains inside its trust boundary; this is weaker isolation than the native personal worker/keychain. Do not describe browser local storage as equivalent to native custody and do not add a browser secret-upload fallback.

OIDC client secrets and OAuth connector refresh/access tokens are also SecretStore material, not login/session/content/audit data. See [DATABASE-ARCHITECTURE](DATABASE-ARCHITECTURE.md#10-provider-credential-custody).
