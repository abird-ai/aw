# Local owner protocol — Phase 7

## Phase 8 nonbreaking control-protocol note

The native client protocol remains version1. The existing permitted account list may now include an explicit personal-device Bridge; prompt selection already carries account/model. No client endpoint/key/actor mutation field is added. The device inference protocol is a distinct authenticated bounded connection with its own version/schema, not a raw escape hatch in `Prompt`. Native attach still loads no local provider configuration; `aw device run` is the separate explicit worker. [Device protocol](DEVICE-PROTOCOL.md).


**Protocol version1; source package0.8.0; pre-1.0 and not compiled/transport-tested as Rust in the authoring environment.** This is a Linux same-user filesystem Unix-socket protocol, not the full future remote/team API. [Owner/identity contract](LOCAL-OWNER.md).

## Transport and bootstrap

Frames are `u32` big-endian byte length followed by exactly that many UTF-8 JSON bytes. No newline framing, compression, HTTP, cookies or passed file descriptors. Maximum request1MiB/response32MiB; zero lengths and malformed/truncated data are refused. The caller must use one ordered request channel. Larger authored prompt limits remain128KiB. JSON control fields deny unknown fields; snapshot content is version-matched data.

Before data, the server checks peer UID. The client checks the private socket path, connected UID, inode identity and handshake identity. The first frame must be Hello, ID1, version1 and no expected instance:

```json
{"version":1,"id":1,"expected_instance":null,"command":{"op":"hello"}}
```

The result identifies `protocol`, random `instance`, verified `principal`, `owner_uid`, package version and actual limited capabilities. It contains no provider key, credential environment reference or endpoint. `shared_active_view=true` explicitly says this owner is not a per-client active-session engine. `inference_location` says configured owner, not a delegated caller device.

Every later frame has a strictly increasing positive transport ID and exact current owner instance. A native caller cannot authenticate as another UID by changing the frame. A reconnect starts a new transport-ID sequence and revalidates the peer; it does not replay prior commands.

## Commands

Exact Serde definitions: [protocol.rs](../../crates/aw-client/src/protocol.rs).

| `op` | Fields / meaning |
|---|---|
| `snapshot`, `accounts`, `sessions`, `branches`, `tree`, `usage` | Existing authorized owner projections; no model dispatch. |
| `inspect` | `session_id`, `branch`; independent read-only stored ancestry, not shared view selection. |
| `prompt` | `request_id`, `expected_revision`, `prompt`, explicit `account_id` and `model`, `session_id?`, `branch_id?`. Normal current account/tool/journal gates. |
| `prompt_status` | `original` exact PromptInput; lookup only, including exact linked final turn where retained. |
| `cancel` | `run_id`; target must be current active run or be reported already settled. No implicit new run. |
| `reset` | `expected_revision`; new saved session or memory reset under existing rules. |
| `open` | `expected_revision`, `session_id`, `branch`; idle, compatible context and current permissions. |
| `fork` | `expected_revision`, `name`, optional `at_node`; safe completed boundary, no file changes. |
| `checkpoint` | `expected_revision`, `summary`; explicit human context, no automatic model call. |
| `acknowledge` | `expected_revision`, `confirmed_inspected:true`; only after actual inspection. |
| `follow` | `run_id`; switches this connection into bounded observation until End. |

Saved target fields must be present together and match the active persisted branch when submitting. Missing/unauthorized accounts are errors, never silent fallbacks. There is no credential upload, arbitrary path/program, native surface mutation, network proxy, shutdown, auth impersonation or endpoint registration command.

```json
{"version":1,"id":2,"expected_instance":"OWNER_INSTANCE_FROM_HELLO","command":{"op":"snapshot"}}
```

```json
{"version":1,"id":3,"expected_instance":"OWNER_INSTANCE_FROM_HELLO","command":{"op":"prompt","request_id":"user-generated-unique-id","expected_revision":4,"prompt":"Explain this failure.","account_id":"permitted-owner-account","model":"permitted-model","session_id":"s_example","branch_id":"b_example"}}
```

The uppercase and `s_example` values are placeholders, not active IDs. Request IDs are at most64 ASCII letters/digits/underscore/hyphen. A transport ID is not a prompt deduplication ID.

## Responses and observation

Each response repeats `version` and request `id`, with a tagged `packet`:

```json
{"version":1,"id":2,"packet":{"kind":"result","value":{"example":"operation-specific data"}}}
```

Other packets: `error {code,message}`, `event {sequence,event,data}`, `end {reason}`. Error codes reuse the current owner categories (400 malformed,401 expired,403 denied,409 stale/conflict,413 bounded limit,429 capacity,503 unavailable). A readable error string is not permission to retry an uncertain mutation automatically.

Follow first returns a Result acknowledgement. It then sends an initial snapshot, ordered deltas/changed markers or refreshed snapshots, optional keepalive events and finally End. A request for a nonactive run still gets a current snapshot and End; obtain its exact result through PromptStatus, not by guessing the latest turn. The server subscribes before copying the snapshot, discards events at/below that high-water mark, and refreshes after broadcast lag. Once End arrives the connection can accept the next ordinary request.

While following, do not interleave commands on that stream. Use another verified connection for cancellation. The SDK's incremental Reader preserves partial header/body progress across cancelled observation polls. An ordinary request future may have partially written or read data: dropping it poisons that client connection instead of allowing the next call to consume a stale reply. The admitted owner task is unaffected by transport disposal.

## Submission, result recovery and shared views

The existing prompt fingerprint includes exact expected view revision, text, resolved account/model, authenticated actor, saved session and branch. Durable lookup occurs before checking the new active view so a previously admitted exact request can be recovered after reset/switch/restart. Current session read access is still checked before its result is disclosed.

`prompt_status` returns `{found,receipt,completion,turn,durable}`. A durable result comes from the actual native-run/final-node link. A preflight failure may have a settled receipt but no native turn; report unknown/failed preparation rather than success. Memory-only receipts may retain completion metadata after reset without old answer bytes. `found:false` never runs anything and is not proof that another owner/database never executed the request.

Open/reset/fork/checkpoint are shared execution-view operations with expected revision but no separate durable idempotency key in this slice. If their reply is lost, inspect the owner/tree before another explicit mutation. They never invoke inference as a side effect of inspection.

Snapshots retain current owner instance/sequence/revision, turns/pending, accounts/tools, reconciliation and saved selection. The owning principal remains Unix-scoped, not a federated user string. Raw generated text may be sensitive or contain terminal controls: the shipped native CLI escapes rendering; an SDK consumer is responsible for safe presentation. No semantic secret detector is claimed.

## Compatibility and security limits

The client and binary are version-pinned source SDKs, not published/stable wire releases. Unsupported protocol/version/instance/schema fails explicitly. No automatic remote negotiation, CORS, secret export, browser pairing-code reuse for native peers, peer-to-peer authority or cross-machine Unix-ID equivalence is introduced.

The exact Linux `SO_PEERCRED`/filesystem semantics are described in `https://man7.org/linux/man-pages/man7/unix.7.html`. They apply to local connected peers, not an arbitrary remote user behind a forwarder. Tokio's pinned source/API must be compiled and tested on the host; reading documentation is not that verification.

## Bounded reply handling

The client rejects oversized/control-bearing handshake labels and more than32 advertised capabilities. Ordinary application error replies remain usable; unexpected reply kinds/schema poison the connection rather than leaving unread streaming data available for the next command. These checks are authored and covered by Rust test source, not compiled evidence in this environment.

## Phase 10 context commands

The version1 matched-build local-client envelope gains tagged `context` with a bounded JSON `command`, and `context_cancel` with its request ID. The owner deserializes into the same typed context action used by the web/CLI; arbitrary actor/key/endpoint fields are not accepted. This is a source-compatible protocol-family increment, not a claim old matched-build binaries understand new variants.

Attach `--context-action FILE` or `/context-flow JSON` performs the command; generated operations return accepted/status handling through the same shared owner. `/summary-cancel ID` is explicit and distinct from ordinary agent cancellation. Permission, account, source release, target revision and exact receipts are rechecked server-side. Read-only status recovery never repeats paid work. [Implementation guide](CONTEXT-FLOW-IMPLEMENTATION.md).
