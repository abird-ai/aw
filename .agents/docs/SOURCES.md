# Implementation source references — Phase 1

This is new aw source, not a copy of an upstream agent. The earlier full design references are retained separately in `.agents/docs/design-v0.7/`. External docs inform boundaries; they do not certify compilation or compatibility of this package.

- Pi agent-core README, inspected for the request's headless SDK/state/event/context-hook separation: `https://github.com/badlogic/pi-mono/tree/main/packages/agent` (README identifies the current package under its upstream namespace). aw is Rust and does not promise TypeScript extension compatibility.
- Crane workspace quick start: `https://crane.dev/examples/quick-start-workspace.html`.
- Crane API for buildDepsOnly, buildPackage, cargoTest, cargoDoc and source filtering: `https://crane.dev/API.html`.
- Axum middleware/SSE documentation: `https://docs.rs/axum/latest/axum/middleware/` and `https://docs.rs/axum/latest/axum/response/sse/`. The source pins Axum 0.8.6; exact pinned-version compilation is a host gate, not inferred from latest documentation.
- Rustyline v17.0.2 source/API used by the inherited inline editor: `https://github.com/kkawakam/rustyline/tree/v17.0.2`.
- Ollama native chat protocol: `https://docs.ollama.com/api/chat`.
- llama.cpp server/OpenAI-compatibility reference: `https://github.com/ggml-org/llama.cpp/tree/master/tools/server`.
- OpenAI Chat Completions: `https://platform.openai.com/docs/api-reference/chat`; no ChatGPT-subscription authentication implemented.

Crane pin `eb35abda9f232cc6610b1d1e3200d15c49b7ac54` and nixpkgs pin `a32edd7654519351e48e80372a928df336394670` are inherited from Phase 0. Their NAR hashes were obtained from upstream Crane lock-update evidence at commit `73b980519cefc727a5f6cc8e5c0947a2f9be6edd`; local Nix fetching/evaluation remains unexecuted. Do not manufacture a Cargo.lock to mask the absent resolver.

The actual fixture outputs, test commands and remaining boundaries are recorded in [build status](BUILD-STATUS.md) and `validation/phase-1/`. Current source status, not an upstream feature description, governs what this package claims to implement.


## Phase 2 references — consulted 27 September 2026

- Ollama tool calling: https://docs.ollama.com/capabilities/tool-calling — native argument objects, tool_name results and multi-round continuation.
- OpenAI function calling: https://developers.openai.com/api/docs/guides/function-calling — completed streamed tool proposals and tool-result correlation.
- OWASP HTML5 Security: https://cheatsheetseries.owasp.org/cheatsheets/HTML5_Security_Cheat_Sheet.html — browser storage/same-origin script and message-boundary risks.
- MDN Local network access: https://developer.mozilla.org/en-US/docs/Web/Security/Defenses/Local_network_access — hosted-page access to user-device services is browser/permission dependent.

No source establishes a generic browser-safe API-key vault, universal local-network access, supported subscription API or completed aw security guarantee. Provider/storage facts are separate from authored/tested source status.


## Phase 3 references — consulted 27 September 2026

- SQLite atomic transactions: https://www.sqlite.org/atomiccommit.html .
- SQLite WAL behavior and limitations: https://www.sqlite.org/wal.html .
- SQLite PRAGMAs: https://www.sqlite.org/pragma.html .
- SQLite backup approaches including VACUUM INTO: https://www.sqlite.org/backup.html .
- rusqlite crate API: https://docs.rs/rusqlite/latest/rusqlite/ . Source pins 0.37.0 with bundled SQLite; the latest docs are not proof of pinned-version compilation.
- fs2 0.4.3 advisory file locking: https://docs.rs/fs2/0.4.3/fs2/trait.FileExt.html .

The exact source migration ran under Python SQLite3.46.1, not rusqlite's bundled library. Rust compiler/Cargo/Nix resolution remains a separate unexecuted gate. No source citation establishes secure remote credentials or Pi equivalence.


## Phase 4 native-workspace references (consulted 27 September 2026)

- Ratatui 0.29.0: https://docs.rs/ratatui/0.29.0/ratatui/ — Terminal, Frame and native widget API reference.
- Crossterm 0.28.1 events: https://docs.rs/crossterm/0.28.1/crossterm/event/index.html — EventStream/raw-mode and bracketed-paste handling; do not mix asynchronous stream with independent read/poll consumption.
- Crane quick-start: https://crane.dev/examples/quick-start-simple.html — retained buildDepsOnly/buildPackage/artifact reuse pattern.
- Cargo tests: https://doc.rust-lang.org/cargo/commands/cargo-test.html — test/build commands and actual execution boundary.

These primary API references informed source choices. Neither documentation access nor direct dependency pins establishes a successfully resolved/compiled dependency graph. No new Nix pin/NAR value was invented; previous pins remain unvalidated locally.


## Phase 5 native Git/PTY references — consulted for this source increment

- Git status porcelain v2: `https://git-scm.com/docs/git-status`.
- Git worktree machine-readable inventory: `https://git-scm.com/docs/git-worktree`.
- Git apply cached/binary/preimage semantics: `https://git-scm.com/docs/git-apply` (verify on the target Git; Python wire tests exercise the exact planned argv separately).
- portable-pty0.9.0 source: `https://docs.rs/portable-pty/0.9.0/src/portable_pty/lib.rs.html`.
- vt100-rust tag v0.16.2: `https://github.com/doy/vt100-rust/tree/v0.16.2`; inspected parser blob `c81495b80623f1d9cc0069717478671ea3a2bfa9`, screen `7dfec97da6370b7905f242b975258a1a38b663a7`, callbacks `cce6f6c0ee2a35dea2e8178b26283d200c01ab8c`, cell `c000cf30cf2b1770eddb0aae7619c5978c14db2f`.

Current pinned dependency APIs are a source-development basis, not evidence this Cargo graph resolved/compiled. Independent real-Git and kernel-PTY tests do not execute the new Rust crates. No general terminal compatibility, repository sandbox, browser feature or provider entitlement is certified here.

## Phase 6 browser references

Browser constraints were checked against primary standards/documentation for this design slice: WHATWG HTML iframe/navigable behavior (`https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-iframe-element`), W3C CSP3 frame ancestry/source restrictions (`https://www.w3.org/TR/CSP3/#directive-frame-ancestors`), and WAI-ARIA tab keyboard patterns (`https://www.w3.org/WAI/ARIA/apg/patterns/tabs/`). Crane's primary getting-started guide (`https://crane.dev/getting-started.html`) remains the build approach reference. No standard/documentation fetch proves this Rust source compiled or a particular site can be embedded.

The actual iframe-move fixture in this environment uses test-inserted srcdoc, because network page navigation is unavailable. Source screenshot/evidence is explicitly labelled fake transport rather than a deployed browser service. Prior provider/Git/terminal references retain their source-version assumptions.


## Phase 7 primary references and limits

- Linux AF_UNIX, filesystem path permissions and SO_PEERCRED: https://man7.org/linux/man-pages/man7/unix.7.html (consulted for this phase). Credentials are those associated with the peer at connection/listen time; these are not federated identities.
- Tokio pinned 1.48 API locations for UnixStream::peer_cred and incremental AsyncRead: https://docs.rs/tokio/1.48.0/tokio/net/struct.UnixStream.html and https://docs.rs/tokio/1.48.0/tokio/io/trait.AsyncReadExt.html. Fetching these exact documentation pages failed in this environment; the code and tests still require real compiler validation.

Independent Python probes executed actual Linux Unix sockets and peer credentials. They do NOT execute the Rust Tokio API or prove native owner/account authorization. No download of a working Rust/Nix toolchain succeeded. Original library/Nix pins are retained pending actual resolver/build evidence.


## Phase 8 and Git workflow references (consulted 28 September 2026)

- Official Git bundle documentation: https://git-scm.com/docs/git-bundle — full `--all` bundles, clone/fetch and prerequisites; not uncommitted work or config.
- OpenBSD/OpenSSH ssh(1): https://man.openbsd.org/ssh.1 — explicit Unix-socket local forwarding and SSH transport. aw does not implement or launch SSH.
- RustCrypto HMAC API pinned as 0.12.1, SHA2 pinned as 0.10.9. These are source choices, not a claimed compiled dependency graph or security audit.

The new source includes a public deterministic HMAC test vector, independent Python probes and unexecuted Rust conformance tests. Official protocol mechanisms are not evidence that this new application is safe for arbitrary remote multi-user deployment. No external source claim substitutes for the actual host gates.

## Phase9 source references (consulted 28 September 2026)

- OWASP Session Management Cheat Sheet: `https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html` — invitation/session token and explicit lifecycle considerations.
- OWASP Authorization Cheat Sheet: `https://cheatsheetseries.owasp.org/cheatsheets/Authorization_Cheat_Sheet.html` — current per-request resource authorization, not trusting client-provided actor labels.
- Tower0.5.2 ServiceExt primary API: `https://docs.rs/tower/0.5.2/tower/trait.ServiceExt.html` — typed service `oneshot` used to route to a chosen existing controller.

These references guide constraints, not proof that the authored gateway compiled or satisfies a security standard. The current HTTP listener remains private loopback, not a public production authentication deployment. No new identity provider or cryptographic algorithm is introduced.

## Phase 10 references (28 September 2026)

- SQLite official ALTER TABLE, generalized schema-change procedure: https://www.sqlite.org/lang_altertable.html — controlled new-table/copy/drop/rename, foreign-key validation; not disabling integrity as an everyday operation.
- SQLite transaction documentation: https://www.sqlite.org/lang_transaction.html — local commit boundaries do not make external model/tool execution atomic.
- Tokio `spawn_blocking`: https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html — blocking work belongs off asynchronous rendering; cooperative cancellation is not forced termination of native blocking code.
- The prior Pi compaction/branch-summarization audit is retained at [PI-CONTEXT-PARITY.md](PI-CONTEXT-PARITY.md). It establishes the referenced interaction, not copied code, current Pi compatibility or achieved summary quality.

Current Rust source is uncompiled here; these primary references do not replace actual pinned-version build/integration tests.


## Phase11 build workflow references — consulted 28 September 2026

This phase does not change the existing Nix/Crane source pins or assert a compiler build. Its optional manually triggered GitHub workflow pins publicly inspected release commits:

- `actions/checkout` v7.0.1, commit `3d3c42e5aac5ba805825da76410c181273ba90b1`: https://github.com/actions/checkout/commit/3d3c42e5aac5ba805825da76410c181273ba90b1
- `cachix/install-nix-action` v31.10.6, commit `8aa03977d8d733052d78f4e008a241fd1dbf36b3`: https://github.com/cachix/install-nix-action/commit/8aa03977d8d733052d78f4e008a241fd1dbf36b3
- `actions/upload-artifact` v7.0.1, commit `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a`: https://github.com/actions/upload-artifact/commit/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a
- Crane API/reference: https://crane.dev/API.html

These are reviewed source/release identities, not execution of those actions or a guarantee that a future GitHub runner/installation policy will support them. Workflow execution requires the user’s deliberate repository/runner action. Package dependencies still need actual resolver and Nix fetch verification. No private source was pushed to a remote service and no cloud machine was provisioned here.
