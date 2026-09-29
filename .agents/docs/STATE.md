# Phase 13 / schema 4 amendment — Domains, Workspaces, Machines and Fleets

> **Current Phase-50 storage note:** live SQLite schema is v32. The older phase sections below are preserved as historical implementation contracts, not a claim that schema2/4 or single-owner-only behavior is still current. Owner/service uses exclusive `owner.lock`; explicit trusted-local direct peers use shared `owner.lock`, require the exact current schema on a supported local filesystem, are heartbeat/store-generation fenced, and never migrate or restore. Current migration/coordination details are in [DATABASE-ARCHITECTURE](DATABASE-ARCHITECTURE.md).

Schema4 adds durable Domain/Workspace/Machine/Fleet/AwNet-peer metadata while retaining the Phase12 owner lock and local SQLite ownership model. Every Session now has one `workspace_id`; existing Sessions are backfilled into the reserved `local` Workspace of their legacy-realm Domain and local Machine target. New FleetWorkspaces are virtual control-plane objects and new Sessions created in one pin its Fleet membership revision. No remote transport executes from these records yet.

The migration does not rewrite historical principal strings, provider provenance, retry attempts or context dependencies. New code uses Domain terminology while `realm_id` remains legacy compatibility data. See [accepted history](../plans/accepted-history/TODO.md).

---

# Durable state contract — Phase 10 / source 0.11.0

**Implemented Rust/SQL source; Rust build not executed here.** Python tests execute the exact migration and representative SQLite transactions; browser tests use fake transport. [Build status](BUILD-STATUS.md) separates those checks from authored Rust and actual-binary fixtures.

**Current extension:** schema2 adds immutable context imports/compactions, source/release dependencies, plans, artifacts and summary-operation receipts. [Context-flow implementation](CONTEXT-FLOW-IMPLEMENTATION.md) specifies those operations. The original local/single-owner journal rules below still apply where not refined by the explicit Phase10 section. Back up existing state before migration; no downgrade is supplied.

## 1. Package and durable storage boundary

`aw-agent-core` defines `TurnJournal`, `TurnStart`, `JournalEntry` and `HistoryState`. It contains no SQLite, filesystem, PTY, HTTP or subprocess implementation. `aw-store` implements the journal with private local SQLite. `src/sessions.rs` composes current trusted config with an authorized stored projection for both CLI and web.

A single cooperative process holds an exclusive `owner.lock` in an owned real 0700 directory. State files are 0600, non-symlink/non-hardlink, checked before SQLite opens them. SQLite uses foreign keys, WAL, `synchronous=FULL`, `trusted_schema=OFF`, an application ID and `user_version=2`. Known schema1 is upgraded through the explicit checked context migration; unknown/incompatible databases are refused. Bootstrap quick_check is integrity screening, not a malicious-file sandbox.

The current store is local filesystem/single-owner. It is not secure against hostile same-UID code, root/kernel compromise, arbitrary network filesystems, or a malicious trusted SDK extension. There is no database encryption. A backup must include a consistent WAL-aware snapshot; copying only an active `sessions.sqlite3` file is not the backup procedure.

## 2. Data model

| Record | Meaning |
|---|---|
| `sessions` | Stable owner/title/root and context binding, global next turn ID and import-review state. |
| `nodes` | Immutable root, authored prompt, final attempt/trace, human checkpoint, additive context import or compaction; one parent. |
| `branches` | Named head reference/revision and active run, scoped to a session. |
| `members` | Current read/append grant, independent of model credentials and tool grants. |
| `runs` | Accepted prompt, identity, state, possible-effect marker and final-node reference. |
| `run_events` | Immutable model-intent/result, completed assistant block and tool-intent/result receipts. |
| `invocation_receipts` | Each native model round, actor/account/model/provenance and known/unknown provider usage. |
| `hazards` | Running/inspection-required/settled possible effects for a canonical tool-root key. |
| `audit` | Immutable grants, fork/checkpoint, import, recovery and explicit inspection acknowledgements. |
| `web_requests` | Persistent actor/request fingerprint, accepted receipt and optional exact native-run link. |

All timestamps are Unix **microseconds**, not seconds or millisecond browser dates. Object IDs are random opaque prefixed IDs. Node parents must already exist in the same session; immutable parent records prevent later cycles. A turn completes a prompt. A new prompt or checkpoint cannot append beneath an unresolved prompt.

A branch can fork only from a root, finished attempt, checkpoint or committed context node. The initial implementation does not fork in the middle of a streaming answer/tool batch. The tool/model subtrace remains inside the attempt plus journal, rather than an individually navigable tree node per token. No filesystem worktree is created by a conversation fork.

## 3. Admission and safe settlement

1. The core prepares bounded completed context with current account/tool grants and before-prompt/context hooks.
2. `journal.begin` authorizes the acting principal, compares the selected head/revision and session-wide next ID, checks root/import hazards, appends the immutable prompt and active run, advances the branch and next ID atomically.
3. Before a provider future is polled, commit `InvocationStarted`. Before any real tool effect, commit `ToolStarted`; mutating/process intents mark a possible-effect hazard.
4. Commit model metadata/completed assistant blocks and tool outcomes as they become available. No private provider-continuation material is serialized.
5. Before reporting a completed in-memory turn, atomically append the final attempt, update branch head/revision, settle hazards and the matching durable web receipt. Native pending usage becomes unknown if no result was recorded.

Start journal/DB failure stops the next dispatch. A later persistence fault blocks continuation and must be reloaded/recovered; an inspection click alone is not a database repair. An observer error cannot undo side effects already journaled/executed. The journal is awaited independently of a dropped UI connection. Trusted native code can still block/panic outside cooperative cancellation guarantees.

One active run per exact canonical tool root in a store is conservatively enforced, including read-only/chat runs. This does not cover nested roots, other databases, arbitrary shells or external applications, and it is not an OS sandbox. Current tools must recheck their own arguments/permissions. Do not infer lock exclusivity across machines from a context label.

## 4. Resume, branch selection and permissions

Load only the chosen root-to-head ancestry; completed traces enter future model context, failed/cancelled/interrupted attempts remain visible but not silently reused as successes. Restore current session-wide `next_turn`, not branch-local counters that could collide across forks. Stale handles fail before model/tool dispatch rather than auto-rebasing.

Current registered accounts and tool policies are retained in the Agent. History restores neither Provider instances nor API keys, OAuth sessions, shell grants or account membership. Original actor/payer/model records remain provenance of old invocations; a new caller must independently authorize shared-context disclosure and select a new valid account lease.

The shipped CLI verifies its effective Unix actor and private store ownership. The private web owner uses one paired `browser:paired` principal; it is not automatic Unix identity inheritance or a federated team user. CLI and web sessions are therefore not automatically interchangeable. Native hosts can explicitly grant session read/append membership, but that does not register or grant another user's provider key. The shipped store has session-level membership, not the future per-node/group policy engine.

Session binding contains canonical directory/tool root, saved instructions, creation-time model/profile hints, and a one-way digest of current endpoint/auth-reference/CA-reference configuration. Resume uses a saved directory when `-C` is omitted, requires matching execution/system bindings, and rejects changed destinations unless explicitly acknowledged with `--allow-context-transfer`. The acknowledgement applies only to that selected session. A rotated secret behind the same env name is not detected by this digest; it is not authentication or proof of physical locality. Switching accounts within the explicitly granted paired owner remains a deliberate disclosure choice.

## 5. Explicit context checkpoints

`/checkpoint SUMMARY` or the equivalent CLI/web action appends an immutable human-authored summary node. Future projection begins with that summary plus later completed turns/context blocks; older nodes remain in the tree and exports. It makes no provider call, does not automatically summarize or silently truncate history, and does not clear a root hazard.

Checkpoint text is labelled unverified user context in the provider messages, not promoted into privileged system authority. The writer must include relevant facts/constraints. The same manual operation is available from CLI management even when the old active history exceeds the in-memory projection bound.

There is no model-specific tokenizer or guaranteed fit to a particular model window. `/context` reports local byte/count estimates and configured limits. The active projection remains bounded (256 attempts, about 4 MiB history representation, 1 MiB prepared context, plus existing prompt/trace limits). Tree viewing/export is bounded to 10,000 nodes and 32 MiB; branches to 1,024. A very large historical tree can exceed the current view/export bound, so keep deliberate checkpoints/backup/rotation policy. SQLite total disk growth is not silently pruned or capped by these view bounds.

## 6. Crash and uncertain effects

When a new process acquires exclusive store ownership, it reconstructs unfinished native runs from accepted records. No provider/tool is called. Final state is failed/interrupted, not invented success. A recorded model-start with no reply is unknown usage: the provider may have been billed even though only an intent is known locally.

Completed assistant blocks may survive; in-flight uncommitted streaming text can be lost. A tool left in Running becomes Interrupted. Any possible mutating/process effect on an unfinished or unsuccessful run remains an inspection-required hazard, even if the process died before a complete result. Acknowledgement requires an explicit trusted user's note. `/new`, reset, checkpoint and forks cannot erase such same-store/same-root hazards. Starting a different store bypasses this cooperative history guard and is not a safe way to settle effects.

This is at-most-once admission/replay protection for known receipts, not exactly-once external world effects. SQLite commit and provider/server/tool completion are not one transaction. Do not automatically retry unknown side effects or charge another account after a disconnect.

## 7. Browser receipts across restart

A persistent `POST /api/prompts` includes exact saved session/branch plus client-view revision, request ID, text and optional permitted account/model. Its fingerprint additionally binds authenticated actor. A request receipt is committed before HTTP202; `journal.begin` atomically links it to the exact native run before provider execution. Final native settlement updates that same receipt in its transaction.

Lookup of an existing actor/request ID precedes current-view revision checks. Retrying the same body after reset/session switch/restart returns the old accepted/settled/interrupted receipt without another call. A changed body/account/target conflicts. A stale current target without a matching receipt is rejected. If preflight rejects before native begin, the ephemeral local binding is cleared and the retained receipt is settled without a provider call. If the owner dies first, recovery marks it interrupted.

Receipts are bounded to 100,000 per store; no invisible expiry that enables a duplicate request. Their history is in local SQLite, not a universal distributed ledger. Deleting/rotating that database removes its deduplication authority. Browser login cookies/codes are not persisted: new owner still needs pairing. The current server has one active conversation projection shared by its paired browser tabs, not multi-user or independent tab workspaces.

## 8. Export, import and backup

The JSONL envelope is `aw.session-tree` version1: header, topologically ordered nodes, branch references, then SHA-256 footer over the exact preceding bytes including newlines. Bounded UTF-8/schema/order/ancestry/prompt-result/ID checks run before import commit. A checksum is integrity checking, not a signature or trusted authorship proof.

An import creates a new ID, current owner and current explicit execution binding. Node/branch IDs are remapped; source authors and usage remain unverified; imported native invoice/dispatch receipts and memberships are not created. Every import starts behind an inspection gate. After acknowledgement its imported-provenance marker remains visible. There is no automatic model/tool execution on import and no original endpoint/account restoration.

Export includes saved prompts, instructions, output, tools, actor labels and paths; those **can contain sensitive information**. Provider secret configuration/handles and private continuation state are excluded structurally, but normal text is not semantically scrubbed. Unsent drafts are not exported. An idle/recovered session is required; incomplete active runs cannot be exported as completed history.

`backup` uses SQLite `VACUUM INTO` to a new private path, never a blind file copy or overwrite. Treat the entire backup as private state. Restore only offline into a new owned state directory, with no other aw owner, and keep the original backup. Opening validates application/schema and recovers unfinished work conservatively. No automatic command to overwrite a live database is supplied.

## 9. SDK surface and validation

```rust,no_run
use std::sync::Arc;
use aw_agent_core::{Agent, Result};
use aw_store::{SessionStore, SessionBinding};

async fn use_saved(mut agent: Agent, store: SessionStore, binding: SessionBinding) -> Result<()> {
    let actor = agent.actor().clone(); // authenticated/trusted host, not a web string
    let (session, branch) = store.create(&actor, "My session", binding)?;
    let (view, journal) = store.activate(&actor, &session.id, &branch.id)?;
    agent.load_history(view.history, Some(Arc::new(journal)))?;
    let turn = agent.prompt("Explain the test failure.", std::future::pending::<()>(), |_| Ok(())).await?;
    println!("{:?}", turn.outcome); // Result::Ok need not mean Completed
    Ok(())
}
```

`Store` management methods are blocking and intended for a host worker; the journal implementation uses `spawn_blocking` internally. Do not call blocking history/export/backup in a browser render or async event-loop path. The shipped web composition uses its blocking worker adapter for these operations.

`examples/sdk-state` is an independent consumer of core + store and a labelled fake Provider, not the CLI or web server. Rust tests cover restore/fork/receipt/recovery/membership, but were **not compiled here**. Python schema checks, Node projection checks and real Chromium using fake transport ran separately. the removed phase3_smoke helper --binary …` exercises actual Rust CLI/web owners and kill/restart only on a host with a compiled executable.


## Phase 4 integration note

The optional native workspace uses the same current account/tool grants and persistent owner described above. The underlying Phase3 SQLite schema and browser account/transport behavior are unchanged. Human file views and layout changes do not enter provider context, start another loop, restore credentials from history or grant permission. Source0.5.0 adds a separate pure layout SDK and native rendering; no new remote hosting, browser key store, inference delegation, Git or embedded PTY is implied. See [WORKSPACE.md](WORKSPACE.md) and [BUILD-STATUS.md](BUILD-STATUS.md).


## Phase 7 native attachment and exact result correlation

A local native client no longer needs to open the same SQLite files as an already running owner. `aw serve` holds the existing exclusive process lock; `aw attach` reads/mutates through its peer-validated local API and opens no database. One active execution view remains, with revision-safe explicit changes. The new read-only Inspect operation obtains an authorized stored branch projection without installing it in the current Agent.

The existing web_requests journal also records native prompt IDs. No schema migration is needed. `web_result` resolves a receipt's actual linked run/final node, rechecks current session access, and returns its optional final Turn. It does not correlate by text or whichever answer is currently visible. Native status lookup never admits work. Missing/unfinished results remain unknown instead of being replayed.

`serve --serve-web` shares the verified Unix principal by explicit pairing grant; standalone web retains its previous separate actor. Context, account selection, immutable history, start/finish receipts, crash hazards and current tool grants are otherwise unchanged. Client connect/reconnect and historical inspection do not acquire provider credentials, restore grants or clear reconciliation.

[Local owner](LOCAL-OWNER.md) · [Protocol](LOCAL-PROTOCOL.md).

## Phase9 named-owner integration

Historically, Phase 9 kept team sessions strictly owned-only. Private team sessions still have qualified internal owners; receipts remain keyed by actor/request and model records retain their actual caller/payer. Phase 38 adds only explicit owner-granted shared Session discovery/open plus separate draft permissions; private-by-default behavior remains, and provider/tool authority is not inherited from membership. A private realm stamp still prevents silent adoption of another realm or an unstamped CLI/web store.

Each participant has a separate current projection but all use one process-held store. Existing same-root hazard serialization remains; separate identities do not create unrestricted concurrent execution. New/open/fork/reset affects only that participant's controller. Current provider/grant checks still apply on explicit resume; neither credentials nor authority are restored from old nodes. See [team contract](TEAM-IDENTITY.md).

## Context-flow amendment — planned next schema, existing behavior unchanged

The existing `checkpoint` node and `HistoryState v1` remain manual, single-path context replacement. `fork` creates a reference without summary generation. No additive cross-branch summaries or automated compaction are currently implemented.

[CONTEXT-FLOW](CONTEXT-FLOW.md) defines separate compact/carryover/multi-source operations, immutable summary artifacts, source/dependency records and additive `context_import` nodes. Implement a real schema/projection/export migration; do not disable the current node-kind constraint or hide imports under checkpoint. Generated summaries must preserve source-access restrictions through later compaction and derived responses. The schema has not been changed by this documentation amendment.

## Phase 10 / schema 2 amendment — ordered context and source restrictions

The earlier schema1-only description is historical. A new controlled migration [002_context_flow.sql](../../crates/aw-store/migrations/002_context_flow.sql) upgrades the existing store to user_version2, rebuilding the nodes constraint for context_import/context_compaction while preserving IDs/parents/sequence and immutable triggers. It adds plans, operations/artifacts/source links, application receipts, pending/consumed one-prompt associations, transitive node dependencies, read-release epochs and imported-private fences. Foreign keys are checked before commit and reenabled; errors do not reset the store. Make a consistent backup first; no downgrade is supplied.

Human checkpoints still replace older active context. Additive imports preserve ordering; compaction retains its first-kept projection suffix. Derivation dependencies remain even when raw source text is later omitted by a checkpoint. Current access and account disclosure must pass at each new provider boundary; revoked derived sessions can be denied conservatively instead of exposing their later answers.

Summary operations reserve IDs without adding fake prompts or branch locks spanning model I/O. Exact Generate/Apply receipts differ. Recovery marks incomplete generation interrupted without repeating a provider. The schema does not promise atomicity with vendor billing. Existing tool/root hazards remain unchanged by import, fork or compact.

JSONL v1 remains importable; v2 includes artifact/dependency and one-prompt structure. Cross-owner derived export is refused without an independent release. Allowed imports are new owned-private/unverified/review-gated histories; original account grants, memberships and native billing receipts are never restored. The fuller [implementation guide](CONTEXT-FLOW-IMPLEMENTATION.md) is authoritative for Phase10 details. Python tests execute the exact SQL migration but do not validate rusqlite or the Rust projection.

## Database architecture v2 target — design amendment

The current Phase 11 runtime still uses the Phase 10 schema-2 single-owner SQLite contract above. The adopted long-term database/sync/auth direction is now [DATABASE-ARCHITECTURE](DATABASE-ARCHITECTURE.md): semantic `commit_log` rather than raw-WAL events; stable opaque users with Unix/internal/OIDC bindings; separate operational and security-audit logs; shared provider credentials behind `SecretStore`; personal provider credentials remaining local; resumable migration coordination; and eventual trusted local direct clients only after CAS/leases/recovery are durable.

**No schema, owner-lock or current recovery behavior changes merely because this design is adopted.** The status matrix is [DATABASE-ADAPTATION-MATRIX](DATABASE-ADAPTATION-MATRIX.md).

## Durable LLM operation/attempt recovery amendment

The existing crash rule remains conservative: `InvocationStarted`/dispatch intent is persisted before entering a provider and an unfinished invocation is never reclassified as "never happened" merely because the owner restarted. Add a distinct **logical LLM operation → immutable provider attempts** model as defined in [LLM-RETRY-RECOVERY](LLM-RETRY-RECOVERY.md).

A process restart classifies an admitted-but-unsettled provider attempt as unknown/interrupted unless a supported provider reconciliation proves a stronger state. For an interactive session, this state is visible and requires an explicit Retry to create another attempt. For an autonomous pipeline step, a persisted retry-policy snapshot may schedule a later attempt automatically. This is a **new attempt with its own accounting**, never replay of the ambiguous attempt or reuse of its request receipt as if the old dispatch did not occur.

A durable completed provider result wins over retry: recover/attach it if possible, or retain it as an unattached branch conflict. Branch movement never causes automatic regeneration. Unknown tool/external effects remain hazards and block automatic continuation even when the model step itself is retryable. Personal-account unavailability remains an explicit wait/failure; retry never silently falls back to a shared provider account.

Autonomous retry state must itself be durable (`due_at`, attempt count, backoff/budget, policy snapshot and eventually a generation-fenced claim) so a second crash neither loses nor duplicates the scheduled attempt. Schedule-occurrence identity and model-attempt identity remain separate.


## Phase 12 / schema 3 amendment — first-class durable operations and retries

Schema3 adds a semantic `commit_log`, branch-ref events, durable logical LLM operations, immutable attempt lineage, retry schedules/claims and UI recovery hints. A provider dispatch that may have happened survives process death as an unknown attempt; a retry is a new attempt, never replay or erasure of the old receipt. Interactive sessions require explicit retry. Autonomous policy may schedule a bounded retry only when no external-effect hazard blocks it. Completed results are persisted before branch attachment, so a stale branch CAS preserves the result with `attach_status=conflict` instead of discarding paid work.

`owner.lock` remains authoritative in this phase. The new records are prerequisites for later direct local multi-process mode; recovery still runs only after exclusive owner acquisition. Raw WAL is not used as an application event feed. See [accepted history](../plans/accepted-history/TODO.md).

## Phase 38 / schema 21 — shared prompt draft state

Schema v21 adds collaboration state separately from immutable Session history:

- `session_draft_policies` — Off/Auto/On and revision;
- `session_draft_grants` — stable-user read/edit/submit/manage/presence permissions;
- `shared_drafts` — branch-bound generations, document epoch, accepted revision, exact target and compact V1 document state;
- `shared_draft_updates` — bounded accepted update revisions with authenticated contributor user ID and idempotent update ID;
- `shared_draft_submissions` — exact sealed candidate bytes/digest, submitter, contributor set and one submission identity per generation.

Presence is deliberately absent from SQLite. Shared drafts are mutable pre-submission resources, not conversation nodes. Sealing freezes one candidate; ordinary Session prompt history changes only after the existing journal/admission path accepts that candidate. Duplicate Send reuses the same submission/prompt request; provider/tool unknown-effect recovery remains the existing no-replay protocol.

Phase 35's v20 process/store-generation ownership and owner/direct locking are unchanged. The trusted-direct opener remains exact-current-schema; after Phase 38 that means schema v21, but its lock/recovery semantics are still the Phase-35 contract.

## Phase 39 / schema 22 — bounded pipeline state

Schema v22 adds `pipelines`, immutable `pipeline_revisions`, explicit `pipeline_bindings`, `pipeline_runs`, `pipeline_step_attempts` and `pipeline_approvals`. Revisions are append-only by SQL triggers. Manual run request IDs are idempotent per stable actor and conflict if reused for a different pinned revision/input/target. `agent` step intent is committed before provider dispatch; a surviving `started` attempt is an unknown external effect and is not replayed on resume. Approval pauses are durable and no database transaction spans the human wait or provider call.

Pipeline binding authority remains separate from Session/provider/tool authority. A Session-scoped binding requires current Session read access but does not manufacture append, credentials, tool grants or payer/model authority. The actual Agent step uses the caller's normal execution configuration and records Session/branch/model/usage provenance when available.

Phase 35 lock/recovery semantics and Phase 38 shared-draft semantics remain unchanged beneath v22. Trusted-local direct remains exact-current-schema; current exact schema is now v22. Healthy schema-v21 state is actual-binary tested through migration and first prompt. Non-lock SQLite failures now retain bounded actionable error detail rather than collapsing to one generic storage string.


## Unnumbered post-Phase-39 / schema 23 — coordination shape repair

Schema v23 adds no new product authority. It is an idempotent compatibility migration for legacy v22 stores whose historical Phase-35 table shape lacks one or more per-process ownership columns on `runs`, `context_operations`, or `web_requests`. Owner mode conditionally restores those columns plus their local-process indexes before recovery. The captured failure `no such column: owner_process_id in UPDATE context_operations ...` is the motivating reproduced shape. No accepted Session history or provider/tool authority is rewritten. Trusted-local direct still requires the exact current schema and never migrates.

## Phase 40 / schema 24 — routing strategies, attempts and affinity

Schema v24 adds durable state for the native main-target routing slice without making routing state conversation history:

- `routing_strategies` + immutable `routing_strategy_revisions` — bounded, secret-free, destination-pinned candidate definitions;
- `routing_bindings` — explicit stable-user/self or owned-Session main-role strategy selection;
- `routing_operations` — exact actor/Session/branch/head/revision/input/candidate-set route admission plus selected target and one-shot working identity/consumption receipt;
- `routing_attempts` — one durable paid classifier attempt with local process/store-generation ownership and `unknown_external_effect` crash state;
- `routing_affinities` — per stable user + Session + branch + role + lineage sticky route-once preference with exact CAS;
- `llm_operations.routing_operation_id` — provenance from the normal working Chat logical operation back to the route receipt it consumed.

Affinity is not a credential/grant cache. Process-local provider lease revisions are not durable authority. Each use revalidates the immutable route target against current saved profile destination/model configuration and obtains a fresh account/model selection. Forks have different branch IDs and therefore fresh affinity by default. Classifier results can select only a predeclared candidate ID; no candidate means no working dispatch.

The working model still enters through the normal Agent and Session journal. The route receipt is consumed inside Chat admission; a mismatch or second consumption conflicts. Same-context retry retains the original account/model/payer/custody/executor identity and may only acquire a fresh current in-process lease for that same destination. There is no crash-time rerouting or account fallback.


## Phase 41 implementation checkpoint — schema 25 / bounded subagent Sessions

Schema v25 adds `subagent_sessions`, a secret-free relation from one exact parent Session/branch/head/revision/turn to one ordinary durable child Session in the same Workspace. It records foreground/background mode, task digest, exact invocation identity metadata, effective child tool mode, lifecycle timestamps and bounded error state. The child transcript/history remains in the normal Session/node/journal tables; the relation is not a second conversation store.

Phase-41 admission is owner-only and requires the exact active parent turn. Generic Session append membership and Phase-38 draft ACLs do not grant spawning authority. The child account/model is not chosen from tool arguments: it is copied from and revalidated against the current `ToolContext.identity`; provider credentials/endpoints are absent from the relation. Child tool mode is forced to `off`.

A running model-only child receives a synthetic internal run root only after the store proves its relation, exact parent run and absence of unrelated physical-root hazards/runs. This permits parent + model-only child concurrency without weakening the ordinary one-active-effectful-run-per-root contract. Recovery marks admitted/running relations interrupted after owner restart while the child Session's normal invocation/run receipts remain authoritative about unknown provider effects; no automatic replay is performed.


## Phase 42 implementation checkpoint — schema 26 / isolated subagent worktrees

Schema v26 adds `subagent_execution_bindings`, separate from `subagent_sessions`. It stores exact parent Session/branch/turn, stable creator, managed-worktree kind, host-selected read-only/workspace tool mode, canonical parent/execution path bytes, exact base commit, optional child Session, lifecycle timestamps and bounded error state. No task body, credential, endpoint, shell command, route, cron or webhook is stored.

`planned` is committed before Git worktree creation. Verified worktrees become `ready`; only a matching exact-parent-turn ready binding can be consumed when child Session creation atomically remaps its working/tool roots and marks the binding `bound`. Owner restart changes `planned`/`cleanup_planned` to `unknown` and never retries Git mutation. Human cleanup can target a child or an unbound ready binding; dirty work is retained unless `--discard` is explicit; successful removal never deletes Session/history.


## Phase 43 implementation checkpoint — schema 27 / delegation budgets

Schema v27 adds `delegation_roots`, `delegation_child_allocations` and `delegation_model_attempts`, plus nullable historical-compatible linkage columns on Phase-41/42 child/worktree rows. New application writes require root/allocation/depth linkage. One root is unique per exact parent Session/branch/turn and stores immutable host-selected limits plus monotonic/accounting counters. `max_depth` is constrained to 1.

Child allocation moves `reserved -> bound -> running -> terminal`; created-child count is consumed at reservation and is never refunded by later preparation failure. Active-child count is acquired at running and released once at settlement/recovery. Every child provider round has an exact `(root, child Session, turn, ordinal)` attempt reservation before provider entry; provider-reported token evidence settles only after the ordinary invocation-finished record. Restart converts still-started attempt reservations to unknown and retains the consumed started count.


## Phase 44 implementation checkpoint — schema 28 / recursive subagent lineage

Schema v28 additively introduces authoritative recursive lineage without rebuilding the cyclic Phase-41/42 tables: `delegation_roots.recursive_max_depth`, `delegation_child_allocations.recursive_depth`, `delegation_child_allocations.parent_allocation_id`, and `subagent_sessions.recursive_depth`. Historical v27 `max_depth/depth` columns remain fixed at one as compatibility provenance; new execution reads the v28 recursive fields. Existing rows migrate to recursive depth/max = 1.

Descendant allocation requires a running same-root direct-parent allocation/relation with exact stored InvocationIdentity; depth is exactly parent+1 and cannot exceed the immutable root recursive max. Descendants never create a new root. New child Session admission validates the same lineage again, so a forged runtime service clone cannot bypass store authority. Restart/recovery continues to operate root-wide and never resets counters or replays descendants.


## Phase 45 implementation checkpoint — schema 29 / child routing receipts

Schema v29 pins an optional child routing revision on each delegation root and records each reserved allocation's routing operation, target, selection source, exact selected InvocationIdentity and binding timestamp. Existing v28 roots migrate with a null route and retain exact-parent inheritance. New roots reject a route revision unless it is an active actor-owned immutable strategy with role `worker`; the route choice is immutable for that root.

`delegation_routing_attempts` records classified child-routing attempts against the same root model-attempt/deadline envelope. Budget reservation occurs before the ordinary Phase-40 routing journal crosses provider intent; ordinary routing settlement remains authoritative and happens before root usage settlement. Started routing-budget reservations become unknown on owner restart and remain consumed. A selected routing operation is one-shot consumed into one child allocation before any Phase-42 worktree or child Session/provider effect.


## Phase 46 implementation checkpoint — schema 30 / team topology and join receipts

Schema v30 adds immutable actor-owned team template revisions, exact parent-turn team runs, per-member child Session receipts and immutable join receipts. Team rows store no candidate answer bodies or joined text. Member receipts contain role/order, child Session/subagent IDs, imported-answer SHA-256 and truncation state; join receipts contain the ordered member manifest plus joined digest and `publisher=parent_agent`.

A team run is anchored to one exact active parent Session/branch/turn. Members transition `pending -> running -> completed|failed|interrupted`; one failed required member terminally interrupts still-pending siblings and produces no join. Owner restart marks admitted/running team runs and unfinished members interrupted without replay, while already completed candidate Sessions remain authoritative and untouched.


## Phase 48 saved binding reactivation

No schema change: current SQLite remains v30. Native workspace navigation now distinguishes two operations: caller-current binding validation (`open/open_session`) versus reactivation of an existing durable Session using its own stored `SessionBinding` (`open_saved/open_saved_session`). The latter is followed by host-side destination/profile/root reconstruction and does not mutate the stored binding. A successful switch therefore preserves durable Session provenance; a failed switch rolls back the active in-memory Host state.


## Phase 49 planner operation state — schema 31

Schema v31 additively pins an optional `planner_routing_revision_id` on each delegation root and adds `planner_operations`, `planner_task_runs` and immutable `planner_join_receipts`. A planner operation records the exact root/parent turn, branch head/revision, fixed planner strategy revision, secret-free InvocationIdentity, planner evidence digest/body, lifecycle, validated proposal/digest and bounded metadata/error. Planned task rows contain topology/lifecycle plus child/session links and answer digests, not full child answer bodies.

Planner lifecycle is admitted → started → planned → running → joined, with failed/cancelled/unknown/interrupted terminal alternatives. A started planner provider intent becomes unknown on owner restart and is never regenerated automatically. Planned/running operations and pending/running task rows become interrupted with completed child Sessions preserved. The planner provider attempt consumes the same root model-attempt/deadline/usage counters as ordinary child rounds.

## Phase 50 guardian review state — schema 32

Schema v32 additively pins optional `guardian_routing_revision_id` on each delegation root and adds `review_operations`, `review_human_approvals`, and immutable `review_release_receipts`.

A review operation is bound to one exact team/planner join and exact root parent Session/branch/turn. It records the source kind/ID/join SHA-256, fixed guardian routing revision, original parent `InvocationIdentity`, guardian `InvocationIdentity`, review-input/policy/objective digests, bounded host policy text, strict verdict/digest, lifecycle metadata and bounded errors. It deliberately does **not** duplicate joined child answer bodies. Full answers remain authoritative in ordinary child Sessions; release reconstructs the exact bounded join from those Sessions and verifies the existing join receipt.

Lifecycle is `admitted -> started -> pass|deny|waiting_human|needs_evidence`, with `failed|cancelled|unknown|interrupted` terminal alternatives. A started guardian provider intent becomes `unknown` on owner restart and is never replayed automatically. An admitted review becomes `interrupted`. Settled decisions and waiting human approvals survive restart.

`needs_human` creates one snapshot-bound approval row. Only the stable owning actor may decide it. Approval is not authority expansion: release also requires the exact original parent Session/branch/provider invocation identity and the original source digest. Release receipts are immutable and identify only the reviewed source digest, release basis (`guardian_pass` or `human_approved`), actor and time.

The same Phase-43 root model-attempt/deadline/usage counters cover guardian inference; guardian work never consumes a child allocation. Trusted-local direct remains exact-current-schema/no-migrate/no-restore and therefore now requires v32.
