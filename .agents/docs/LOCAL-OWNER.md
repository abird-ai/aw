# Shared local owner — Phase 7 / source 0.8.0

## Phase 8 addition — explicit personal links

A durable `serve` can now accept up to four private `--device-link` files. They register explicitly bound personal accounts behind private listeners. The same native/browser clients can select those permitted accounts per prompt; they send no provider keys, endpoint or actor overrides. The concrete worker on the device reads its own configuration and independently validates a finite grant/receipt ledger. The existing same-user control socket and shared active owner semantics are unchanged.

This is not multi-user authentication or an automatic remote connection. Cross-machine access requires a separately reviewed SSH setup, and owner tools remain owner-local. [Device setup](DEVICE-INFERENCE.md) · [Device protocol](DEVICE-PROTOCOL.md).


**Authored source; Rust/Nix execution not validated here.** This increment provides a local same-UID owner/client boundary, not a complete remote workspace or user-device inference system.

## One execution owner, several presentations

```text
aw serve --socket PRIVATE_PATH
  one configured Unix-principal Agent
  one current provider-account registry + tool policy
  one private SQLite owner/journal
  one current session/branch execution projection
       │
       ├─ private Unix socket → aw attach / independent aw-client consumer
       │
       └─ optional --serve-web → paired loopback browser
                                  existing conversation / Files / Git / PTY UI
```

The shared owner reuses the existing application handlers and `Agent::prompt_as`; it is not another loop or another store implementation. Each accepted prompt still binds current account, model, principal, tool root and execution metadata. No client attaches by opening the SQLite files.

Ordinary `aw`, `--workspace` and `aw web` remain distinct existing launch paths. No socket/client machinery is initialized on their minimal native path. `serve` starts a Unix listener explicitly and is foreground; TCP is started only with `--serve-web`. No background service is installed and a dropped terminal that kills the foreground owner is not process persistence. Use an explicitly chosen service manager/tmux on the host when needed.

## Identity and pairing

The owner and native peer are both the verified effective Unix UID on the same Linux machine. Socket parent/private mode plus kernel `SO_PEERCRED` checks reject other UIDs, not malicious code already running under the same UID. Root/kernel compromise, user namespace remapping, transferred descriptors and SSH/proxy forwarders are not solved by attaching a username to JSON. Only deploy this slice for trusted same-user local processes.

An absolute socket path (at most100 bytes in this implementation) must have an existing private owned immediate parent. Symlink ancestors, dot/empty/control components and existing targets are rejected. The created socket is0600. The client verifies the filesystem identity and connected peer; the server verifies each accepted peer before reading its protocol. No actor, key, endpoint or filesystem authority is accepted from JSON.

`serve --serve-web` explicitly offers the same Unix-owner role through one-use pairing. The startup warning states this. The browser must pass the existing Host/Origin/JSON/cookie gates and does not have access merely by visiting localhost. The user distributing that pairing code grants the owner role, including access to that principal's compatible histories and permitted accounts. This is not an invite for an independently authenticated teammate.

The old `aw web` path retains `browser:paired`, separate from Unix histories. It gains no native socket and no implicit identity mapping. Pairing remains one-use/expiry based; re-pairing after logout requires owner restart in this first slice. Same-origin code and browser-profile security remain existing trust assumptions.

## View model and execution limits

The execution projection is shared. Open/new/fork/checkpoint requires an idle owner and expected current revision; existing root/instruction/destination checks still apply. The web interface's unsent-draft retarget guard remains necessary. The native client submits against its last displayed revision rather than silently refreshing/rebasing on Send.

`Inspect(session_id,branch)` is independent read-only access to an authorized stored projection. It does not install that history in the active Agent, change any default root, move another client, or call a provider. It is bounded by existing store projection limits and can report busy/unavailable. Branches at different roots may be inspected as authorized history; opening one for execution still requires the current compatible binding.

Only one active run is supported. This phase does not run many per-tab agents or switch tool roots in one owner. A conversation fork is not a Git worktree or filesystem snapshot. Selecting a browser review worktree still does not redirect the Agent.

## Receipt and result safety

A client captures an exact prompt, saved target, account/model, expected view revision and semantic request ID. The transport also carries the current owner instance. The owner reuses existing durable admission/replay checks. Explicit account/model fields avoid accidental interpretation under changed defaults during a later lookup.

`PromptStatus` is lookup-only. It computes the same fingerprint and returns only the matching authenticated receipt plus its exact linked final native turn when available. The new `SessionStore::web_result` reads the run's final-node link with current session authorization; it does not guess based on a prompt string or the latest visible turn. No schema migration or duplicate usage ledger is introduced. The historical `web_requests` table now also serves native-client requests.

Memory-only owners retain bounded small completion metadata, not duplicate full answers. Reset can remove the old in-memory answer while its receipt still reports an outcome. Owner restart loses memory-only receipts. A missing receipt in another/recreated store does not prove a provider or tool never ran.

Ordinary request cancellation/timeouts poison the client transport and preserve uncertainty. Neither reconnect nor status lookup resubmits. The CLI retains the exact original input in memory until resolved or explicitly discarded; this is not a durable client outbox. A library host that persists such input must protect its potentially sensitive prompt text and must not store it in source archives.

## Lifecycle and logout

Disconnecting a native socket or closing the browser leaves admitted owner work alive. The same owner may be queried through another authorized client. Native cancellation is a separate targeted command; Ctrl-C during follow opens a second verified connection and checks the same owner instance before sending it.

In shared-owner mode, browser logout clears its cookie and shuts down the browser surface host/terminals, but does not cancel the common agent or close native sockets. Old standalone `aw web` keeps its prior cancellation behavior. Paused browser cookie expiry similarly does not become native owner shutdown. Explicit owner shutdown cancels work, closes listeners, requests terminal cleanup and gives journal settlement a bounded chance; after abrupt death the store recovers unknown effects without replay.

The socket path is removed only when its current inode/device still matches the bound object. A stale path is not automatically replaced. Inspect the stopped owner before manually removing one. No arbitrary native protocol shutdown or shell-launch command is added; model tools remain those configured and gated at owner startup.

## Resource limits

At most16 native connections. Follow streams share the existing eight observational slots with browser SSE. Requests max1MiB; response envelopes max32MiB; user prompts remain128KiB. First handshake timeout5s, idle next-request timeout30min, slow response writes timeout5s. SDK ordinary call timeout30s. A bounded broadcast lag results in a fresh snapshot, not replay of tools or missing deltas treated as fact.

A consumer can wait for a run without holding the Agent mutex. Streaming updates are observation only. Malformed/oversized frames or stale instance IDs fail before dispatch. Post-dispatch socket failure cannot undo work. Trusted native extensions and blocking OS operations remain cooperative and are not forcibly preempted by these transport deadlines.

## Validation gate

Run the real host script, `cargo test -p aw-client`, native owner Rust tests, the independent SDK example, and the removed phase7_smoke helper --binary target/release/aw`. The latter must demonstrate two native peers + paired browser using one owner, exact deduplication, poisoned local config not being loaded, read-only branch inspection, logout/cancel separation and lookup after restart. Independent Python AF_UNIX probes are narrower OS/wire evidence, not proof those Rust paths execute.

[Protocol](LOCAL-PROTOCOL.md) · [Credentials and future bridge](CREDENTIAL-PLACEMENT.md) · [State](STATE.md) · [SDK](SDK.md).

## Phase 10 context flow

The shared owner now accepts typed context planning, generation/status, manual artifacts and application via `aw attach --context-action FILE` and `/context-flow JSON`. It retains one active mutation controller; a summary does not create a second Agent or occupy the branch writer across provider I/O. Other viewers may inspect permitted status, but unrelated mutations wait/fail busy. Same-UID/native and browser pairing roles remain those established by the owner, not actors supplied in JSON. [Context guide](CONTEXT-FLOW-IMPLEMENTATION.md).
