# aw-agent-core

Headless Rust agent SDK, version 0.2.0. In-memory multi-turn conversation, injected providers, typed lifecycle events, cancellation and trusted native async extension hooks. No terminal editor, HTTP client, web server, tool execution or database dependency.

See [SDK guide](../../.agents/docs/SDK.md), [embedding example](../../examples/sdk-host/src/main.rs), [extension library](../../examples/rust-extension/src/lib.rs), and [build status](../../.agents/docs/BUILD-STATUS.md). This crate is supplied as local source, not published on crates.io or a stable ABI. It was not compiled in the authoring environment.


Phase 3 adds the optional `TurnJournal` and `HistoryState` contracts. The concrete SQLite implementation is a separate `aw-store` crate; it is not a core dependency. See [state](../../.agents/docs/STATE.md) and [SDK](../../.agents/docs/SDK.md). Current crate version0.4.0 remains source-candidate/uncompiled in the authoring host.


## Current Phase 10 / source 0.11.0

Ordered context blocks and a separate tool-free SummaryExecutor/SummaryJournal are added, with before/after-summary hooks and current source-aware journal guards. HTTP/filesystem/SQLite remain outside the core. [SDK contract](../../.agents/docs/SDK.md) · [Context implementation](../../.agents/docs/CONTEXT-FLOW-IMPLEMENTATION.md). These APIs are source candidates, not compiled or security-audited here.


## Phase 11 / source 0.12.0 — hardening

The stream sink keeps a sticky failure, source/account access is checked again after awaited intent writes and summary output hooks, and standalone summaries use the normal extension registry checks. See [accepted history](../../.agents/plans/accepted-history/TODO.md) and the real compiler gate. Source tests are provided; compilation is not claimed by the reference Python test result.
