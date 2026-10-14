# aw-store

Optional SQLite host crate for `aw-agent-core`'s journal interface. See [SDK](../../.agents/docs/SDK.md) and [State](../../.agents/docs/STATE.md). One private local process owner; immutable nodes and start-before-effect receipts; no provider credentials or grants restored from history. Source is pre-1.0 and not compiled in the authoring environment. The headless core does not depend on this crate.


## Current Phase 10 / source 0.11.0

The store now upgrades to schema2, with pinned plans, summary operations/artifacts, additive/compact/one-shot application, source-read releases, dependency propagation and export controls. Back up before migration; no downgrade is supplied. [SDK contract](../../.agents/docs/SDK.md) · [Context implementation](../../.agents/docs/CONTEXT-FLOW-IMPLEMENTATION.md). These APIs are source candidates, not compiled or security-audited here.
