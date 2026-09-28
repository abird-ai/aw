# Personal-device inference — Phase 8 / source 0.9.0

**Authored Rust source, not compile-validated or audited.** Phase8 adds a concrete narrow bridge rather than adding a server key-upload field. Local private-socket operation and an explicitly configured SSH stream are the supported source paths. Internal/OIDC users, general remote public hosting and browser-only keys are separate work.

## 1. Ownership and grant

The owner has one existing `Agent`, journal, account registry and tool root. Each prompt obtains its usual explicit account/model lease. A personal account can now reference an `aw-inference::Bridge` with matching actor/account/executor metadata; the registered adapter does not contain the personal provider's endpoint or secret. An ordinary account cannot be registered with a mismatched execution site, and a delegated descriptor cannot replace a preexisting account.

The device's trusted local CLI configures `DeviceWorker` with a concrete provider before connecting. Its local grant binds exactly one model/account/executor and an owner principal, with a bounded set of durable session/branch IDs. The CLI creates a one-scope grant; the underlying SDK supports at most16 exact scopes. A branch name is not an ID; a fork needs a separately authorized branch scope. UID labels describe the authenticated owner role within this paired deployment, not global cross-machine identities or OIDC assertions.

The device validates binding, scope, message structure/size, tool-schema allow-list, grant lifetime and finite counters before polling a provider. It reads credentials only through the locally configured adapter. No request can override local endpoint, authorization headers, environment references, local config or file root. Labels/grant hashes are not hardware attestation; trusted native code can violate its own labels.

A personal invocation's account owner and payer remain the initiating user. The server's ordinary/shared accounts stay distinct and must be explicitly selected. An offline/denied worker causes that chosen personal call to fail—never substitution, automatic reconnection, or another billed call.

## 2. Setup and transport

Create a private0700 directory outside the source checkout on each participating Linux machine. Use `aw device key --out ABSOLUTE_PATH` to create a new private32-byte pairing capability. Copy only that capability to the other endpoint using a trusted private channel. It is not a model API key, cannot be used as one, and is never included in a browser snapshot or JSON credential field. Possession authenticates this link, so treat it as sensitive.

The owner `--device-link` file is private JSON containing version1, socket, key_file, account_id, executor_id, model and grant_id. `serve` accepts at most four links, only with durable state. It binds a dedicated private socket per link, separate from its native-client socket. Normal `aw`, `aw web` and `aw attach` do not create an inference worker or load a personal profile implicitly.

The device `run` connects to an owned private same-UID Unix socket and authenticates the paired binding with a mutual nonce/HMAC handshake. For two machines, independently create an SSH StreamLocal forward:

```sh
# On the personal device, first ensure /LOCAL/PRIVATE is owned and 0700.
# Inspect host keys and authenticate SSH through your normal trusted workflow.
ssh -a -N -o ExitOnForwardFailure=yes \
  -o StreamLocalBindMask=0177 -o StreamLocalBindUnlink=no \
  -L /LOCAL/PRIVATE/device-forward.sock:/REMOTE/PRIVATE/device.sock \
  USER@OWNER_HOST

# A different local terminal runs the compiled aw worker; it does NOT open SQLite.
aw device run --socket /LOCAL/PRIVATE/device-forward.sock \
  --key-file /LOCAL/PRIVATE/pair.key --grant /LOCAL/PRIVATE/grant.json \
  --receipts /LOCAL/PRIVATE/receipts.json --profile PERSONAL_PROFILE
```

SSH is a user-configured external prerequisite; aw neither launches it nor manages its keys. Do not disable host-key checking, agent-forwarding restrictions or existing socket inspection. A stale local/remote socket is not automatically deleted by aw. If a host's SSH configuration forbids stream-local forwarding, the operation is unavailable, not silently converted into an unauthenticated TCP endpoint.

HMAC binds frames but does not encrypt them. The shipped connection is private local IPC or a deliberately encrypted/authenticated SSH tunnel. `DeviceWorker::run_stream` is a low-level trusted-host/test seam; exposing it on arbitrary TCP without confidentiality/access control is not a supported deployment.

Browser access to an owner on another host remains subject to the existing private numeric-loopback Host/Origin pairing contract. This bridge does not turn its web server into multi-user TLS hosting or broaden CORS. Set up appropriate deliberate local access separately; no seamless browser→localhost credential extraction is assumed.

## 3. Finite local consent

`aw device grant` creates a new private file; it does not overwrite an existing grant. Review it before starting the worker. Defaults and hard ceilings:

| Bound | CLI default | Hard ceiling / semantics |
|---|---:|---|
| Lifetime | 1,800 seconds | 24 hours, actual current clock must be within issue/expiry. |
| Model invocations | 8 | 64 reservations per immutable grant/ledger. Each tool round is another model invocation. |
| Context bytes | 131,072 | 1 MiB; native context representation, not a tokenizer count. |
| Text + complete tool-proposal output | 32,768 bytes | 512 KiB per invocation. |
| Aggregate reserved output | 262,144 bytes | 16 MiB. Full per-call reservation, even if a failed call emitted less. |
| Requested output-token cap | 4,096 | 32,768. Passed to the local configured provider; vendor support remains a prerequisite. |
| Invocation deadline | 120 seconds | 600 seconds or remaining grant lifetime, whichever is smaller. |
| Advertised schema names | None | At most32 explicit names. No local tool execution. |

The fixed local profile's output-token cap may be lower; a higher configured cap is rejected. Profile fingerprinting includes endpoint/auth-reference/CA/options, not credential values. Secret rotation behind the same environment reference is not detected or claimed to be another identity.

These are local traffic/admission/timeout controls. They do not guarantee provider billing is bounded to an exact currency amount or that cancellation stops vendor computation. A malicious paired owner can spend within the finite grant and submit content within its approved scopes; this is not independent per-prompt human confirmation. A stronger deployment needs additional trusted local interaction/policy, not the pretense that HMAC proves a human authored a request.

Deleting/changing a grant or stopping the worker revokes future execution. The worker rechecks while idle at most about1s and while streaming about500ms plus bounded write/processing delays. Already-dispatched inference and already-sent bytes cannot be recalled. Blocking trusted native provider code may delay cooperative cancellation; this SDK is not a preemptive sandbox.

## 4. Replay and receipt ledger

The local ledger records a reservation before polling the provider. Identity is session ID + session-global turn ID + round ordinal; branch changes cannot evade that identity. The request digest includes exact authorized context/tools/identity but excludes connection epoch and registry revision so reconnect cannot make the same invocation look new.

A private owned regular JSON file and exclusive adjacent lock store at most64 receipts (256KiB file). Atomic temp-write/fsync/rename commits state; an unexpected external edit fails closed. A record contains request ID/digest, start/settlement time, reserved/observed output bytes, state and optional provider-reported usage. It does not contain prompts, keys, endpoints, output text, shell commands or raw private continuation.

On worker restart, prior `started` entries become `interrupted`. A repeated ID—same or different payload—is refused. Lookup does not execute anything. A successful result is receipted before its final frame, but transport loss can still leave the owner missing that answer. Phase8 does not cache/replay full results; inspect metadata, accept uncertainty and explicitly initiate any genuinely new task. No hidden retry can reclaim an already-used grant reservation.

The owner maintains its existing durable prompt/model journal. Both sides may record an attempted call whose vendor usage is unknown. Their files are not a distributed atomic transaction or verified invoice. Deleting the ledger, recreating history in another store or issuing a new grant changes the deduplication boundary; none is a safe automatic retry strategy.

## 5. Model tools and continuation

A completed tool proposal returns to the owner's existing grant/hooks/tool executor. The device only validates the declared schemas and completed call envelope. It neither runs a local tool nor certifies that the owner's remote effects are safe. Core `ToolContext` keeps the owner's execution-site label. Raw tool result content subsequently sent for inference is part of the approved context disclosure.

Some providers need private model continuation between tool rounds. The device keeps it in a bounded memory cache and sends a random opaque handle, not the raw continuation. The handle is valid only for the same session/turn and exact public assistant block/tool calls. Modified/reordered incompatible data, worker restart or missing handle causes failure rather than invented reconstruction. The core strips private handles from durable historical context as before. This is not a hidden reasoning export feature.

## 6. Connection lifecycle

One authenticated worker session is active per configured bridge, with a fresh one-job queue. Disconnect/cancellation destroys that queue; pending work does not migrate to a new connection. No automatic reconnect. Both directions verify ordered authenticated frames and invocation event order. Text is chunked/coalesced without silently dropping it; a slow or invalid transport closes and reports uncertainty.

Stopping owner work closes its particular delegated operation; the device cancels its local future cooperatively. Stopping only a viewing browser/native client does not cancel the owner. Idle device disconnection makes the registered account unavailable. No other account is chosen. A new worker must use the same local receipts unless the user deliberately creates a reviewed new grant.

## 7. Explicit non-goals and gate

No public remote API, OIDC/internal users, browser credential vault, raw HTTP proxy, local shell bridge, personal device discovery, autonomous device wakeup, spend settlement, hardware attestation or end-to-end encrypted shared history is shipped. Trusted same-UID programs and root can read local secrets; a pairing file does not isolate a compromised operating system. Ordinary inline local account use and common owner accounts remain unchanged.

Host acceptance must execute real Rust tests and the removed phase8_smoke helper against the compiled binary, inspect provider/header/state separation, and run a real local model. The independent Python policy/HMAC/ledger tests only test the assumptions and vectors; they are not bridge execution. A remote SSH deployment additionally needs actual two-machine verification; this package does not claim that was done.

[Protocol](DEVICE-PROTOCOL.md) · [SDK](SDK.md) · [Credentials](CREDENTIAL-PLACEMENT.md) · [Build status](BUILD-STATUS.md) · [Git workflow](GIT-WORKFLOW.md).

## Phase 10: explicitly permitted summary invocation

Use source0.11.0 on both owner and worker because the private wire is version2. Existing grant schema1 remains chat-only. A prepared context plan displays `input_sha256`; inspect its complete evidence/focus, then create a reviewed grant with repeated `--allow-summary DIGEST` (at most16) in addition to exact actor/account/model/session/branch and existing finite budgets. A summary consumes a normal paid/permitted invocation reservation and requires no local tools.

The worker rejects changed input, undeclared evidence, tools/private continuation or a missing digest grant. A permission for the target chat alone never authorizes arbitrary cross-source summarization. Provider keys remain resolved only in the local worker. Do not silently rotate an existing grant/ledger to hide uncertainty or reset its invocation history. [Wire amendment](DEVICE-PROTOCOL.md) · [Context implementation](CONTEXT-FLOW-IMPLEMENTATION.md).
