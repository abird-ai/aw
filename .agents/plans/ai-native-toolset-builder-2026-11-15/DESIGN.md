# Toolset builder design — 2026-11-15

## Scope and contract

A host instantiates one **validated immutable** `Toolset`, composed from small typed profiles and using exactly the existing canonical operations. It owns metadata and limits, **not** resources, credentials, auth, effects, durable state or execution. Synchronous/default-feature builds stay independent of Tokio, MCP, AW Agent and host environment. The optional async/Job implementations consume the same validated values.

```rust
let tools = Toolset::builder()
    .filesystem(SchemaCaps { /* host-selected ceilings */ ..Default::default() })
    .grep(GrepProfile { default_context: 0, max_context: 128,
        ..Default::default() })
    .jobs(JobProfile { get_tail_default: 512, get_tail_max: 1024,
        ..Default::default() })
    .receipts(ReceiptProfile { unix_ts: true, checkpoint_minutes: Some(35) })
    .build()?;
```

- `Toolset::builder()` / `ToolsetBuilder::build()` validate everything at initialization; `Toolset` is cloneable, immutable and cheap to pass by reference/Arc.
- `filesystem_spec(tool)`: canonical schema, title/name/effect metadata and documented properties from exactly the configured limits. `request_limits()` and `grep_limits()` use the same caps; no description/enforcement drift.
- `operation_title(id)` / `operation_description(id, base)`: shared operation vocabulary, host-provided specific descriptions preserved; clear bounded extras only where relevant. Descriptor source is shared; adapters can add platform/MCP-specific instructions without forked core contracts. Format-specific title/annotation wiring stays with host adapters.
- `ReceiptProfile` is presentation intent: `unix_ts` opt-in and host-selected `checkpoint_minutes`. No clock in `aw-tools`; host supplies a **factual** Unix timestamp for the compact receipt. Only structured/model compact *receipts*, not raw file payloads or binary/media, are decorated. `model_instructions()` emits only guidance matching configured, actually provided metadata; configured minute horizon is advisory, not a Job timeout/auto-turn-ending behavior.
- `JobProfile` covers `jobs_get` default+max **aggregate** tail bytes, read/get wait ceilings, and list capacity hints. The shared engine ceiling is above host defaults, but actual `jobs_get` (AW/dotlink) must constrain requests to host-configured max. No new byte-wake semantics: `jobs_get.wait_ms` means settle-or-deadline; `jobs_read.wait_ms=0` is a nonblocking read/peek; positive-offset read waits for bytes/EOF. A negative offset is an immediate snapshot peek.
- `GrepProfile` covers symmetric default context, max per-side context, and bounded work. Validate *every explicitly supplied* context/before/after against the configured maximum even if overridden. Same actual shared parser/engine in direct and Graph.
- Safety ceiling > configured ceiling >= default; validated profiles never issue grants. Schema, direct request, Graph request, descriptors and response constraints must see the same immutable values. Factory creation must not silently widen enclosing host policy.

## Per-tool description hooks

**Decision: preserve canonical semantics, allow metadata-only customization.** Extend the builder with a small `ToolText` value and `tool_text(OperationId, ToolText)` hook. It covers the complete canonical 25-operation vocabulary: filesystem, Process, Graph, Agent and Jobs. Multiple calls for the same operation merge nonempty edits (last field value wins; appended guidance accumulates in order). No user-facing new tools or generic registration framework.

```rust
let tools = Toolset::builder()
    .tool_text(OperationId::Fs(FsTool::Grep),
        ToolText::new()
            .title("Search source files")
            .description_suffix("Use grep for narrow discovery, read for the full file.")
            .argument("pattern", "Regex or literal pattern"))
    .tool_text(OperationId::JobsGet,
        ToolText::new().description_suffix(
            "Prefer tail=0 for status-only; default is ${jobs.get_tail_default} bytes total."))
    .build()?;
```

- `title`: optional model-display/MCP title; it never becomes the canonical operation name, ID, effect or authority.
- `description`: replace standard prose when a product needs a different explanation; `description_suffix`: extend the shared default instead (recommended). Fields are **only** description strings, never structural schema fragments.
- `argument(name, text)` addresses root property `name`; `argument("/properties/edits/items/prefixItems/0", ...)` may address a nested property using a JSON Pointer to the **property object**, not the description leaf. Unknown or typoed argument paths fail. Nested descriptor paths are interpreted against the actual canonical/host schema, never inserted as new properties.
- Template variables resolve from the same validated profiles, once at build: `${grep.max_context}`, `${grep.default_context}`, `${jobs.get_tail_default}`, `${jobs.get_tail_max}`, `${jobs.get_wait_max_ms}`, `${jobs.read_wait_max_ms}`, `${jobs.list_page_max}`; unknown `${...}` fails before tool registration. Values are materialized, not reevaluated on every call. No general templating language or arbitrary closures required.
- Reject empty, multiline/control-character, or oversized metadata at construction. For filesystem tools, validate argument-path existence at build regardless of whether the operation is advertised; for host-supplied Process/Jobs schemas, validate when projecting the actual schema and fail closed before registration.
- `Toolset` owns a small immutable metadata overlay keyed by `OperationId`; the AW adapter applies it at original tool registration via `configure_with_toolset(builder, actor, options, &toolset)`, using a metadata-only `Tool` wrapper that delegates *unchanged* execution, validation, grant and scheduler logic. `LocalToolOptions::with_toolset` remains a limits-only convenience. Dotlink later maps the same descriptor to MCP title/description/argument annotations.
- Catalog introspection and runtime registration must use the same effective description. A disabled tool remains disabled; a configured description may not select tools, widen numeric caps, alter allowed arguments or introduce a second registration/dispatch mechanism.
- No need for inheritance, runtime mutable callbacks, a `dyn Trait` plugin system, reflected parameter setters or tool-specific desc forks.
- `Toolset::only([...])` filters the available **filesystem** vocabulary when AW uses `configure_with_toolset`; it does not itself select or grant Process/Job capabilities. The host's existing selected tools, mode, ToolPolicy, prepared executors and Agent Envelope still independently constrain registration. Empty selections remain explicit metadata catalogs and never grant missing tools.


## Focused edit decision

**Choose Option A:** canonical `edit` remains an exact, focused **single-file** operation, with one original-file snapshot and bounded replacement tuples. Multiple files are composed with existing `batch`/Graph `["edit", {path, edits}]` nodes. Graph owns order, conflict checks, independent per-node receipts, authority, failed/partial results and replay. Do **not** import dotlink 0.17.x's multi-file edit contract or add a second multi-file mutation engine during this builder phase. A future change would require measured model efficiency and clearer transactional guarantees, not simply another wire shape.

## Compact model receipts: ownership

A structured result is source truth: exact Job/output/outcome, byte ranges, permissions, replay and truncation. It stays separately accessible to SDK/host. Model receipt format is a **pure projection**, not an execution mode.

- Short meaningful first line (`j:7 bash fail rev=4 exit=1 out=18K err=732`), absolute continuation when useful, no raw argument echo and no bulk JSON wrappers.
- Optional `ts=<epoch_seconds>` is host-supplied additive metadata, appended only to a model status/receipt after final result size is known. It never replaces original Job/Tool timestamps or authorizes effects.
- A mandatory bounded result budget covers all status/tail/metadata text. An over-budget receipt must error or narrow data with truthful markers; no unbounded footer.
- `next` guidance is host-/operation-specific (e.g. a returned Job handle should tell the model not to start the shell again); do not inject generic repeated prose. Global `model_instructions` may carry a configured 35-minute checkpoint recommendation, *not* core policy.
- Keep compact result vocabulary and canonical titles/specs **as separate representations**; don't conflate a convenient one-liner for models with typed facts and SDK data.

## Compact receipts — first implementation slice (2026-11-15)

- **Canonical source:** immutable typed `job_manager::Snapshot` (and in a later phase, explicit typed completion/ACK/read-page facts). `aw-tools` never accepts a model-authored pseudo-status as proof. SDK/transport callers retain the original typed facts, not a parsed version of the model line.
- **Pure projection:** `aw_tools::receipt::Receipt` is a small closed enum, initially `JobStatus { ref, snapshot }` and `Ack { ref, kind, changed, released }`. It is **not** a new execution or persistence engine. Status extraction uses the original lifecycle, revision and terminal summary; no extra sources, guesses or string-parsing of manager status. Each receipt is one bounded, compact factual line with stable field names.
- **Presentation:** `Toolset::render_receipt(&Receipt, host_unix_timestamp, total_budget)` renders exactly once, optionally appending `ts=<epoch>` from the host. The ordinary SDK/Graph result and Job output pages are not decorated. No wall-clock read, dynamic callbacks, implicit cleanup, or output lease advancement in the shared renderer. A missing required timestamp or insufficient budget rejects the **whole** receipt rather than dropping status, changing fact fields, or minting delivery evidence.
- **Format:** default status `j:7 bash running rev=4`, or `j:7 bash settled rev=5 outcome=fail` with truthful `more`/`incomplete`. Graph progress is `finished/total` while unsettled. ACK is `j:7 ok ack=all released=false` or an explicit `ack=<offset>`; no claim that physical cleanup completed if another owner still has a lease. Handoff/reference/label length and control-character validation prevent tool-response line injection. Receipt text must be computed from typed data; a previous free-form `render_receipt(&str)` decorator cannot be used as a general raw-file wrapper.
- **Shared host integration:** use the same pure Job-status formatter in AW `jobs_list`, `jobs_get` (header before any bounded tail) and constructor handoff where semantics match, keeping current exact Actor/Envelope and JobManager authority. Do not call the receipt renderer for every text-returning tool or render a Job's retained output as a status receipt. Host-specific `ts`, checkpoint guidance and follow-up wording are deferred choices; the shared opt-in should make them easy after the cross-product design pass.
- **Typed Process fields (landed shared/AW):** terminal-only `ProcessTerminal` contributes exact `exit`, raw `out`/`err` byte totals, `out_truncated`/`err_truncated` and `output_incomplete` when appropriate. `NextAction::ObserveJob` emits `next=jobs_get|jobs_read no-rerun` only on constructor handoffs. It is a presentation hint, never implicit execution or ownership. Graph roots do not summarize child streams. Normal Job listings avoid hints and Process-byte joins.
- **Still separate:** `jobs_get` settlement-only wait, `jobs_read` byte waits/peeks, per-owner delivery/ACK, unknown-dominant Graph aggregation, release policy and DOTLINK MCP/Tasks. Those correctness decisions are gated in the existing Job/grep plan, not silently changed by receipt formatting.
- **Acceptance:** byte-exact 1-line status/ACK, terminal and running, cancelled/revoked, mixed/unknown, progress, `more/incomplete`, no newline injection, short/tight result budget, disabled/enabled timestamp and no clock dependency. Compare old public status/handoff cases before swapping a renderer. No duplicate status ontology.

## Preserve decisions for after builder qualification

Option A (single-file `edit` composed with Graph) was chosen and implemented in the builder phase. Keep other product-specific choices deferred: legacy recursive `ls` vs `find`, dotlink shell/tool descriptions, per-product timestamp defaults/turn clock, replay/Task ownership, optional Process receipt fields, rolling terminal tail vs retained prefix, and cross-product presentation. Shared `mixed/unknown` Graph aggregation was resolved in the AW Job/grep plan: `unknown` dominates any unresolved component; `mixed` is settled heterogeneous only. Remaining choices require separate semantic review after the shared factory. Never change ACK/delivery security merely to format a receipt.

## Implementation layers

1. `aw-tools`: simple immutable `Toolset`, validated profiles, shared canonical descriptions and title metadata; configured schema, parser and grep execution; exact bounded compact receipt decorator.
2. AW host adapter: consume shared filesystem schema/limits and runtime profiles without new authority or engine. Minimize changes to already dirty Job/grep implementation; do not regress existing behaviors.
3. Dotlink adapter: **after AW checkpoint**, consume the same factory, supply dotlink-specific host overrides/metadata (including `ts` and 35-minute guidance), preserve local machine policy and transport framing. No changes to dotlink in this phase.

## Acceptance

- Distinct host-selected profiles produce distinct advertised max/default, same per-side context validator and underlying execution limits; invalid combinations rejected before use.
- Catalog introspection lists each canonical operation unambiguously with documented arguments; schema shape, strict unknown-field behavior and visible numbers verified. Tiny schema budget; no duplicated registry of execution methods.
- Compact status receipt with optional factual timestamp and host guidance; tight budget/zero/missing timestamp, no raw read mutation; no wall clock or Tokio dependency in the factory.
- Existing AW/native/Graph and conformance tests, feature matrix, strict lint and integration checks at coherent checkpoints. No unrelated changes, version bump, or push.
