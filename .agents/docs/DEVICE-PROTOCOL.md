# Personal-inference link protocol — Phase 10, protocol 2

**Pre-1.0 authored source protocol. No Rust/network/security audit is claimed.** This is a separate private link, not an expansion of browser JSON fields or the existing `aw-client` command schema. Exact definitions are in [protocol.rs](../../crates/aw-inference/src/protocol.rs), [grant.rs](../../crates/aw-inference/src/grant.rs), [owner.rs](../../crates/aw-inference/src/owner.rs) and [worker.rs](../../crates/aw-inference/src/worker.rs).

## Authentication and framing

One registered link binds actor, account_id, executor_id, model and grant_id. The owner principal is established by the existing owner, not supplied as a browser/client override. The local worker independently compares the exact binding with its private grant. Both machines hold an explicit32-byte random pairing capability; it is distinct from personal model credentials.

Messages use the existing big-endian32-bit byte-length framing in `aw-client`. Initial frames cap at4096 bytes. The owner sends `Hello {version:2, epoch, challenge, binding}` with fresh random32-byte lower-hex epoch/challenge. The worker replies with `Proof {version:2, nonce, value}` using its own fresh random nonce. The owner replies with the role-separated owner proof using the same hello/nonce. Both proofs must verify within5s before dispatch queues become available.

Proof MAC is HMAC-SHA256 over **length-prefixed** fields: domain `aw-inference/auth/v2`, role `worker` or `owner`, compact serialized Hello struct, worker nonce. Each field is prefixed by its big-endian64-bit byte length. Standard constant-time MAC verification is delegated to the cryptographic library; error content never reveals the secret. [Public test vector](../../crates/aw-inference/fixtures/handshake-v2.json) uses an intentionally public fixture key, never an operational credential.

Authenticated envelope:

```text
Envelope { sequence: u64, payload: UTF-8 JSON string, mac: lower_hex_32_bytes }
```

Each direction starts at sequence1 and accepts only the next exact value. The frame MAC covers length-prefixed domain `aw-inference/frame/v2`, session digest, sender role, big-endian64-bit sequence and exact payload bytes. Session digest is SHA256 of the serialized Hello/worker-nonce tuple. Payload cap2MiB, enclosing frame cap8MiB. Write timeout5s. Strict outer schemas reject unknown fields. Invalid/partial frames close the session; the reader preserves partial bytes across cancelled read polls.

This authenticates an explicitly shared capability and replay context. **It does not encrypt generic TCP, attest hardware, verify provider usage, or prove which human typed a prompt.** The supported CLI path requires private Unix IPC or a separately configured encrypted/authenticated SSH tunnel.

## Inference request

```text
Request {
  version: 2,
  id: session_id + ':' + turn_id + ':' + ordinal,
  epoch: current_connection_epoch,
  context: {
    purpose: Chat | Summary {operation_id, input_sha256, sources},
    scope: {session_id, branch_id}, turn_id, ordinal,
    identity: {actor, account_id, account_owner, payer,
               credential_custody, inference_executor, model, grant_revision}
  },
  messages: [{id, role, content, tool_calls, tool_call_id, name, continuation}],
  tools: [{name, description, parameters, effect}]
}
```

No endpoint/header/API-key/env-name/config-path/local-executable/local-tool-root field exists. Normal user/tool text may contain sensitive content; structural exclusion is not a semantic secret scrubber. The owner already admits that context to the chosen account; the local finite grant independently restricts recipient, scope and budget.

The owner only submits from durable session/branch context, with nonzero bounded session-global turn/round IDs. At most32 schemas, 128KiB schema JSON, 8192-byte descriptions and the core message/pairing limits apply. Personal owner/payer and device custody must match the exact grant. A raw schema is not permission to execute a command on the device.

`continuation` is absent or a random lower-hex lookup handle from that device for this same turn and public assistant-block digest. Raw model private state remains local. A generic host calling legacy `Provider::stream` on a Bridge is denied; it must supply the explicit invocation context and registry lease.

## Response events

```text
Delta    {type:'delta', id, sequence, text}
Finished {type:'finished', id, sequence, meta, tool_calls, continuation}
Failed   {type:'failed', id, sequence, code}
```

Per-invocation event sequence starts at1 in addition to the outer connection sequence. Delta chunks are at most16KiB; cumulative public output is capped by the owner's limit and the narrower local grant. Complete tool call IDs/names/arguments are validated before the owner can execute anything. Provider-specific error bodies/configuration are replaced with bounded failure categories.

A final device receipt is persisted before Finished is written. This is not proof the owner received or stored it, and not a distributed transaction with the provider. Loss or cancellation closes the invocation; no hidden resend. The owner can retain partial visible text but must not turn that into a completed success.

Allowed failure categories: denied, limit, already_attempted, unknown, provider, expired, cancelled, protocol. Some fatal conditions close the link instead of sending another frame (partial write, revocation, storage failure or cancellation). Failure categories do not grant permission to choose another account.

## Admission, attempts and cancellation

1. Authenticate fixed binding and current local grant.
2. Validate request structure/scope/schema and reconstruct only legitimate private continuation.
3. Atomically reserve the semantic invocation ID and full per-call output allowance in the private ledger.
4. Poll only the injected local provider, with fixed model/token settings and deadline.
5. Recheck local consent while waiting/streaming; emit bounded text; never run a tool.
6. Persist terminal outcome/known usage before sending complete result.

Semantic receipt digest excludes connection epoch and registry grant revision, but retains actor/account/model/scope/public request identity and content. Repeated known IDs are refused even when text differs. A reconnect gets a new queue/epoch but cannot reset the local ledger. A changed grant/local profile must not silently reuse the ledger.

The protocol intentionally has no remote reset-ledger, edit-grant, export-secret, launch-shell, reconnect-and-replay or permissive endpoint-override command. Receipt inspection is a local CLI/SDK operation. A low-level trusted stream consumer can monitor state; it cannot infer remote cancellation/billing rollback merely from EOF.

## Locations, evidence and compatibility

`aw serve` can register up to4 explicit links; each has one authenticated worker and one bounded job slot. CLI `device run` opens an outgoing connection and grants nothing automatically from an owner's request. Owner-side and personal-side Unix UIDs can differ across a separately authenticated SSH tunnel: the fixed owner-role label in the local grant remains deliberate, not a global UID mapping.

The reference Python tests cover standard HMAC vectors, role/nonce separation, framing, local policy shapes and a separate receipt implementation. The Rust tests and actual-binary smoke harness must run before declaring compatibility. There is no promise that a JSON schema excerpt substitutes for the matched0.11 source or that this unreviewed link is a general Internet protocol.

[Device setup/security](DEVICE-INFERENCE.md) · [Existing owner protocol](LOCAL-PROTOCOL.md) · [Actual test gate](BUILD-STATUS.md).

## Phase 10 protocol2 and explicit summary purpose

Protocol1 remains available only in Git history and its historical fixture. Current protocol `VERSION` is2, with authenticated domains `aw-inference/auth/v2` and `aw-inference/frame/v2`. The current deterministic fixture is [handshake-v2.json](../../crates/aw-inference/fixtures/handshake-v2.json); v1 remains only as historical evidence. Owner and worker must upgrade together; mixed versions fail closed.

`InvocationContext.purpose` is Chat by default or Summary containing operation ID, input_sha256 and exact EvidenceRefs. A summary request must have no tools/private continuation and exactly the fixed public system/user message envelope. The device recomputes the digest and compares declared source refs with the envelope dependencies before dispatch.

Grant schema2 adds bounded `allowed_summary_digests`. `device grant --allow-summary SHA256` records a separately reviewed exact input. Old grant schema1 with no such field remains eligible for ordinary chat only; it cannot authorize a summary by reusing the target-session scope. Existing scopes, models, finite calls/output/deadline, receipt reservation, disconnect and no-replay conditions also apply. No new key-transfer or arbitrary HTTP proxy exists.

The summary manifest/digest binds reviewed bytes, not truth or a malicious owner's semantic labels. Local approval is still a trusted finite consent boundary. New account/grant/link configuration must be coordinated deliberately; deleting receipt history is not a safe retry. [Detailed implementation](CONTEXT-FLOW-IMPLEMENTATION.md).
