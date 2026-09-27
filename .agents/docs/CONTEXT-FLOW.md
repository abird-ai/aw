# Cross-branch context flow — proposed contract

> **Phase 10 update (28 September 2026):** the source now implements the bounded explicit path. This document remains the dated Phase 9 design/audit; see [CONTEXT-FLOW-IMPLEMENTATION.md](CONTEXT-FLOW-IMPLEMENTATION.md) for actual source behavior, deliberate limits and build status.

**Status:** design amendment, not an implemented Phase 10 or a new runnable feature.  
**Date:** 28 September 2026.  
**Audited implementation:** Phase 9 / source 0.10.0, tag `phase-9`.  
**Scope:** Pi-like branch carryover plus explicit multi-source summaries across permitted branches and sessions.  
**Related:** [current state](STATE.md), [code/behavior comparison](PI-CONTEXT-PARITY.md), [credentials](CREDENTIAL-PLACEMENT.md), [team boundary](TEAM-IDENTITY.md), [current plans](../plans/TODO.md).

## 1. Current state, without implying more than exists

The current SQLite implementation stores root, prompt, completed attempt and human-authored checkpoint nodes. A branch is a named head reference. A view projects one root-to-head path. Forks preserve existing nodes and do not run a model, summarize another branch, copy files or merge a Git worktree.

`SessionStore::checkpoint` accepts a supplied string. `history_in` walks backward until the newest checkpoint, then returns that text and only the subsequent attempts. The core labels it user-authored, unverified context. This is a deliberate context cut, not an additive import or model-generated summary. The newest checkpoint replaces prior context *in the future prompt projection*, not in the saved tree.

Phase 9 team users are intentionally restricted to their own sessions. The base SDK supports session membership, but the team wrapper refuses cross-owner opens. Neither fine-grained branch/node policy nor a cross-user sharing endpoint is implemented. Summary source selection must not bypass that wrapper.

This proposal does not change those runtime semantics. It specifies the next layer and its release gates. The application remains compile-unvalidated; this documentation commit does not satisfy the build or Pi-replacement gate.

## 2. Four separate user operations

| Operation | Reads | Effect on destination context | Model work |
|---|---|---|---|
| Plain fork / switch | Chosen authorized ancestry | Normal selected path; no imported knowledge | None |
| Compact this branch | An authorized prefix of the current path | Summary replaces only a declared older span; retain a coherent recent tail | Explicit generation, or human text |
| Switch/fork with carryover | Work being left, relative to the destination | Append imported branch knowledge without discarding destination history | Explicit generation, or human text |
| Summarize selected sources | One or several chosen branches/ranges, optionally other sessions | Preview; then attach once, append to a branch, seed a fork, or seed a new session | Explicit bounded generation, or human text |

These share a summary service/pipeline but do not share projection semantics. **A checkpoint reduces context; a context import adds knowledge.** Never implement cross-branch import by calling the existing checkpoint method with only the other branch's summary: that would omit the destination's prior active context.

The design goes beyond a navigation-triggered summary: the user need not be leaving a source branch or have it open. Select accessible sources in the tree, state a purpose, choose a permitted summarization account/model, review the result, and decide where it goes. Source selection does not automatically include every branch in the session.

## 3. Plain tree ancestry plus explicit knowledge references

Keep one conversational parent per node. Do not turn the session history into a multi-parent conversation graph. An imported context block points backward to immutable evidence, but those references are not new conversational parents.

```text
shared starting point P
  +-- architecture -- A1 -- A2                  source snapshot A2
  +-- tests        -- T1 -- T2                  source snapshot T2
  +-- experiments  -- E1 -- E2                  optional source E2
  `-- implementation -- I1 -- I2
                              `-- ContextImport(A2, T2)
                                    `-- next user prompt
```

A2 and T2 stay on their original branches. I2 remains the target's parent. The imported block stores a summary artifact reference and explicit source coverage. The next prompt sees implementation context plus the accepted summary, not a concatenation of all histories.

The same primitive can create a new fork at P with imported lessons from A2/T2, or create a new session with an independent root and a context-import child. A new session requires a separately chosen execution binding, account defaults and audience. Importing text never copies tools, credentials, policy exemptions, filesystem mutations or the old model setting.

## 4. Pin sources before reading or calling a model

A source descriptor identifies an authority/store, realm, session, branch label for display, and **immutable node ID**. If selected by branch head, resolve and capture its revision/head during preparation. Branch names alone are not replayable evidence.

```text
SourceSnapshot {
  authority_id, realm_id, session_id,
  branch_id?, head_node_id,
  selection: DivergentSuffix | StandalonePath | ExplicitRange,
  range_start_exclusive?,
  read_policy_revision,
  source_manifest_digest
}

TargetSnapshot {
  target: ExistingBranch | NewFork | NewSession | NextPromptAttachment,
  session_id?, branch_id?, parent_node_id?, expected_branch_revision?,
  requested_audience_ref,
  execution_context_ref
}
```

For the first implementation all sources live under the same local authority and realm. The authority qualifier prevents future namespace collisions; it does not create a federation fetcher. Cross-realm transfer is denied until separately designed consent/export rules exist.

Only committed, permitted source content enters preparation. Prefer finalized turns/checkpoints; selecting a running branch pins its last eligible committed boundary. Never use partial tool arguments, mutable drafts or incomplete private provider continuation. A source may continue running after its snapshot is pinned; new output is not silently included.

### 4.1 Three scopes

**Divergent suffix** is the normal carryover case. Within one tree, find the deepest common ancestor of the source and target paths, and collect source entries after it. For source path P-A1-A2 and target P-I1-I2, select A1/A2, not P again.

**Standalone path** includes the explicitly selected source's own context required to interpret it. Use it for a new independent session, cross-session sources, or when a delta alone would be misleading. A bounded shared prefix can also be supplied as labeled background to a delta summary; it is recorded, not counted as new discoveries.

**Explicit range** includes a chosen contiguous path interval. Reject inconsistent boundaries. Non-contiguous selections are separate ranges. If a source/target's semantic setup differs, show this instead of silently assuming one set of instructions applies everywhere.

Structural common ancestry is not proof that the target model still remembers every shared fact. A manual checkpoint is lossy and has no exact semantic coverage manifest today. Do not infer complete coverage from its string. Let the user choose standalone/background inclusion when the target projection has omitted necessary context.

## 5. Prepare one bounded evidence set

1. Authorize source discovery/read and current target append/disclosure before fetching source bodies. Undiscoverable sources leak no titles, counts, IDs, authors or snippets.
2. Pin snapshots and normalize the requested ranges. Fetch only necessary paths, not every stored session or sibling branch.
3. Deduplicate exact evidence identity `(authority, session, node, selected fragment)`. The same source node selected through two branch paths contributes once. Identical prose at different nodes is not automatically the same fact or evidence.
4. Exclude source ancestry already deliberately supplied in the target background for a delta operation. Record why each range was omitted. If a selected node is an ancestor of another selected source, normalize the overlap while preserving per-branch attribution.
5. Build an evidence manifest with selected/omitted spans, content digests, tool-result excerpts, source status and derivation dependencies. Prior summary dependencies remain dependencies, not a reason to recursively unfold every raw source without permission or bounds.
6. Apply deterministic limits and source/destination policies, including the summarization provider and every guardian receiving text. Show the exact scope and proposed budget to the requester.

A provenance coverage list records which evidence was read, **not a guarantee that the resulting prose preserves every fact**. Avoid using such lists as permission to discard needed detail or as semantic equivalence proofs.

The first slice should refuse over-budget input with a useful narrower-range suggestion instead of silently claiming full coverage. An explicit later bounded map/reduce mode can summarize source partitions and then synthesize, charging and recording every invocation. Cap source count, input/output bytes, model calls and duration by the smallest applicable host, model and local-device grant limit. Byte bounds are not exact token counts.

Do not fetch actual files to make a summary seem current. Source file paths/hashes and test outcomes describe what was observed *then*. Any verification against today's working tree is a different authorized tool step, not hidden behavior of summarization.

## 6. Summaries have identity and provenance

Proposed records, not new fields already accepted by the current SQLite schema:

```text
ContextSummaryArtifact {
  artifact_id, schema_version,
  source_snapshots[], materialized_evidence_manifest,
  dependencies[], generated_from_artifact_ids[],
  purpose, method: Human | Model | Pipeline,
  content, content_digest, structured_source_map,
  created_by, generation_operation_id?,
  invocation_refs[], generator_profile_revision?,
  audience_and_disclosure_policy_ref,
  coverage: CompleteSelectedScope | ExplicitPartialScope,
  created_at
}

ContextImportNode {
  node_id, parent_id, target_session_id,
  summary_artifact_id, source_manifest_digest,
  attached_by, attachment_request_id,
  attachment_mode: Additive,
  created_at
}
```

Keep artifact content and lineage immutable. Updating or human-editing a generated summary creates a new artifact version, marked human-edited and linked to the original. A branch head/attachment configuration can advance by audited commands. An imported artifact uses the source restrictions applicable at access time; an immutable allow receipt never becomes a permanent exemption.

### 6.1 Useful summary structure

A default prompt asks for the goal, constraints, supported decisions, attempted alternatives, relevant tests/results, unresolved conflicts, files/artifacts with source context, and next actions. Preserve source labels and qualifiers:

```text
Design branch at A2
  Decision: preserve the public parser API.
  Evidence: public answer and test result at named nodes.

Tests branch at T2
  Finding: expiry-equality test failed in worktree test-42 at this revision.
  Status: observed in the source, not rerun on implementation.

Conflict
  Experiments proposed a breaking API. Design branch explicitly rejected it.
  No merged resolution is asserted.
```

Label observed facts, suggestions, hypotheses, failures, unknown/interrupted work and imported/unverified material distinctly. A model must not make two alternative designs appear jointly approved or mark proposed changes as completed. Exact code/diff/config material should be a separately authorized artifact or excerpt when correctness requires exact bytes; prose summarization is lossy.

Only public stored conversation content and permitted tool records are evidence. Private model reasoning, provider continuation, raw credentials and hidden branches are excluded. The same summary can reference prior summaries while retaining all transitive confidentiality dependencies. Do not claim independent corroboration when two sources derive from the same original.

## 7. Generation and attachment are different commands

Use the same provider selection, journal, cancellation and accounting semantics as other model work, with a dedicated **summary-purpose operation**. Do not call an HTTP adapter directly from a tree widget or invoke the unrestricted main agent just to obtain a summary.

```text
Prepare authorized source/target snapshot (no inference)
                 |
Admit explicit summary operation and reserve budget
                 |
Persist start intent before invoking selected generation provider
                 |
Generate tool-free bounded candidate (and optional required verifier)
                 |
Persist candidate + usage/status privately to requester/authorized reviewers
                 |
Preview / revise / cancel / select destination
                 |
Recheck access, destination audience, policy and exact target head
                 |
Atomically append import (or create fork/session + import)
```

The source is not changed by generation. The destination is not changed until application. Do not reserve a branch writer lock for an entire model request; use a pinned target and a compare-and-swap when applying. If the target moved, preserve the candidate and offer a deliberate fork/rebase choice. Do not automatically rerun paid generation or append at a new head the user did not select.

A source advancing after preparation does not invalidate its immutable snapshot; label it as of the captured node. A revoked permission or removed source, however, blocks subsequent disclosure/use unless a valid independent release applies. Generating a fresh summary of a newer snapshot is explicit, never background mutation of an old imported artifact.

Generation IDs and attachment request IDs are separate. Retrying an acknowledged attachment reuses its receipt, not another node or model call. A missing model completion after a crash is unknown usage; it does not authorize automatic regeneration. An orphan candidate can remain privately available under retention policy. Reusing an accepted summary elsewhere performs only current authorization and attachment work, not another charge for the original generation.

For a branch being left, the UI can collect options before navigation and select the destination only after successful apply. Cancellation or failure defaults to leaving the original view unchanged; the user can explicitly choose plain navigation. Never hide a navigation failure as a successful summary.

## 8. Access to read is not access to redistribute

The requester's permissions are necessary, not sufficient. Check all of:

- Read the selected source content and its derivation dependencies.
- Send that content to the chosen summarizer's actual inference destination/device, and to any guardian receiving evidence.
- Append to the selected destination or create the new session/fork there.
- Release the derived content to the destination audience, including exports, search and later model use.

Example: Alice can read a private incident branch and edit a project branch shared with Bob. Alice must **not** be allowed to expose the incident to Bob just by summarizing it into the project branch.

Initial least-complex slice: same-principal, owned source sessions into an owned private target, with current provider destination consent. Phase 9's team owned-only check remains. Same-session branch imports are still explicit because context contamination and model cost matter even when current permissions happen to match.

Cross-user sources require explicit sharing/release policy and a way to authorize each derived artifact and its descendants. Do not lift the team owned-only wrapper wholesale and equate a low-level SDK membership with safe context transfer. The user requirement covers any accessible branch in the final architecture; staged restrictions must be shown rather than implied to be implemented.

### 8.1 Restriction propagation and revocation

An artifact inherits the intersection of permitted source disclosures, unless an authorized declassification pipeline issues a specific independently scoped release. A summarizer, guardian or human click cannot invent that authority. A private source's restricted facts remain restricted when paraphrased.

Revalidate on preparation, dispatch, candidate preview/publication, attachment, future context assembly, viewing and export. Provider/prompt caches, compactions, retrieved snippets and imported summaries are subject to the same derivation closure. A wider destination membership change cannot make existing restricted artifacts visible automatically.

**Do not merely remove an imported summary after revocation while leaving its already-derived answers freely visible or reusable.** The answer and later summaries may reveal the same facts. Conservatively track consumed source dependencies on each affected invocation/output. A principal unable to use those dependencies must be denied the affected branch/context or receive an explicitly approved sanitized release/fresh branch. First implementation may deny the whole affected derived path rather than attempt unreliable word-level filtering.

No software revocation recalls content already delivered to a user's device or vendor. This contract governs future accesses and disclosures. Retention/deletion requirements are a separate administrative concern; immutability is not a promise of eternal access.

### 8.2 Source and dependency graphs

Conversation parentage remains a tree. Summary dependencies form a bounded backward-reference graph across immutable artifacts/nodes. A newly created summary may depend only on committed older source snapshots/artifacts; it cannot depend on its own future output. Enforce depth/fan-out limits, stable identity and cycle checks where imported external formats could forge references. A summary of a summary cannot discard inherited restrictions or reset lineage.

## 9. Caller accounts and local credentials

The person requesting generation chooses their own permitted summarization role/account/model. The source authors' model, payer or credential preferences are historical metadata, not defaults or grants. A shared summarizer account is used only through an explicit existing grant. The next ordinary destination prompt independently chooses that caller's normal main model/routing policy.

Use a **generation-capable** model for prose synthesis. Jev/Laya can be optional policy/routing/verifier decision steps, not automatically a conversational summarizer. Missing/denied/offline accounts do not cause fallback. Every actual map, reduce or verifier invocation is separately attributed; the summed original branch work is not rebilled or counted as another execution by importing it.

Personal-device inference retains keys locally. The existing device grant authorizes native session/branch invocations; it does not currently define cross-source summary disclosure. Before shipping this path, extend the versioned request/grant/receipt contract to identify `purpose: summarize` and the exact authorized source manifest, destination binding and budget. A target-session grant alone must not authorize arbitrary content harvested from other sessions. Do not smuggle summaries through a fake ordinary chat request to bypass that local consent.

The local device validates permitted source scope/manifest and destination/model limits using its trusted local policy or explicit approval. An HMAC from a paired owner authenticates the sender, not truthful content labels from a malicious owner; stronger local source verification requires verified snapshots/digests against a trusted authority. Document that trust boundary, rather than calling manifest IDs proof of privacy. No additional provider key ever enters the owner database, request envelope or browser storage.

A cancelled or unknown summary still consumes any observed/possible model usage. No website, tree expansion or plain branch change can trigger summarization just by rendering.

## 10. Context projection: preserve intent and ordering

Current `HistoryState v1` has only `turns` plus one optional summary. It cannot faithfully represent ordered additive imports between old and new turns. Introduce a versioned ordered projection containing completed turn spans and explicit context blocks, while maintaining the renderer-independent core.

Proposed behavior:

```text
active policy/system instructions (trusted host only)
applicable compacted prefix, if any (unverified derived context)
completed target messages before the import
imported summary, with labels/provenance and scope warning
completed target messages after the import
new caller prompt
```

Context imports are data, not live tool calls, permissions or privileged system instructions. Old tool calls/results are serialized as attributed evidence; never execute them on load or resurrect a prior call ID as a pending tool. Preserve actual completed call/result pairings for retained active trace segments.

Compaction has an explicit span and first-kept boundary. It may later absorb an imported block, but the new compacted artifact retains the import's full dependency restrictions and source links. An explicit context exclusion can stop an independent block's future use; if subsequent turns relied on it, exclusion alone cannot erase their dependencies. Fork from an earlier clean point or obtain an approved sanitized context.

Only the host's authorized projection enters the core. The core does not gain database/network ACL queries or raw SDK access to arbitrary sessions. The UI tree and model context use separate permission-filtered projections and independent budgets.

## 11. UI: small choices, optional richer picker

When navigating or creating a fork, offer:

```text
Continue here
  [ ] Carry a summary of the branch I am leaving
  [ ] Include summaries from selected branches…

If a summary is selected:
  Scope: changes since common ancestor / standalone / range
  Focus: decisions and test results
  Summarizer: my permitted model/account
  Destination: this branch / new fork / new session / next prompt only
  [Prepare] -> [Preview and apply]
```

These are proposed controls, not existing CLI commands. The first option is the zero-model-call path. A tree can select several sources without navigating to each. A protected source does not appear unless discoverable. TUI selection/preview uses keyboard-accessible views; browser uses the same commands and a searchable multi-select picker. Do not add a mandatory navigator column.

After attachment, show a collapsed contextual block with source branch/session labels where authorized, pinned snapshot IDs, generating user/model, a coverage indicator and links to inspect evidence. Show conflicts/omissions and old-worktree status. The main composer and per-user account controls remain unchanged.

Suggested per-user preferences are `carryover: never | ask | automatic_under_explicit_policy`. Never and Ask make no speculative calls while browsing. Automatic mode must specify scopes, model/account and limits in advance; it runs only on an explicit navigation action, not cursor selection, hover or tree refresh. It is separate from current-branch auto-compaction and from cross-user read grants.

Repeated imports of the exact same artifact can default to Focus existing with an explicit Duplicate reference option. Do not regenerate a paid summary or call multiple imports independent evidence. Reuse validates current policy and destination each time.

## 12. Storage, protocol and SDK implementation seams

Use the existing SQLite owner, not a second summary database. Plan additive tables for summary operations/artifacts, normalized source/dependency links, attachments and request receipts. The current `nodes.kind` constraint allows only root/prompt/turn/checkpoint; supporting context imports requires a deliberate schema migration with backup/restore/foreign-key/immutability tests, not stuffing undocumented payloads under checkpoint or disabling triggers.

Preserve existing human checkpoints as explicitly human-authored legacy artifacts, with no invented model usage or exact evidence coverage. Use the actual migration mechanism and update exporter/importer validation with the schema. A copied summary exported without its restrictions is a data disclosure. Export must carry authorized dependency/release metadata or require a reviewed self-contained release; import of unknown external dependencies stays unverified and execution-gated, never trusted because a checksum matches.

Proposed logical API stages (not shipped routes/signatures):

```text
context.prepare(request) -> pinned plan and authorized preview metadata
context.summarize(plan_id, account_selection, request_id) -> operation receipt
context.summary(operation_id) -> authorized candidate/status
context.apply(artifact_id, exact_target, request_id) -> import/new-branch receipt
context.cancel(operation_id) -> cooperative cancellation
```

Suggested SDK boundaries: pure source-range/coverage-plan types; store-owned authorized preparation and atomic application; a core/provider-backed tool-free summary executor with normal receipts; frontend renderers that observe and request actions. These can begin as modules in existing crates, not another agent implementation or framework explosion. Pipeline wrappers use those same operations when the generalized pipeline feature arrives.

Errors must distinguish stale target, denied disclosure, source unavailable, budget exceeded, model failure/cancel, incomplete operation and unknown completion without leaking undiscoverable resources. Polling/reconnect retrieves state; it never repeats a model call. Public protocols reject unknown metadata that attempts to set actor, payer, endpoint, policy outcome or source grants.

## 13. Staged implementation and acceptance

This amendment is a prioritized feature definition, not permission to skip current build validation.

1. Compile/correct current Rust/Cargo/Crane and execute existing state/provider/user isolation tests.
2. Add typed additive context blocks and store-owned preparation/application for same-principal sources, including explicit manual artifacts; migrate schema and serialization safely.
3. Add explicit tool-free generation, private candidate preview, invocation accounting and exact apply. Support one-branch carryover plus multiple owned-branch sources using the same plan primitive.
4. Wire CLI/TUI/web tree controls, source-range selection, new-fork/new-session destinations and user preferences. Extend personal-device grant/protocol before that generation route is exposed.
5. Enable explicit cross-user sources only with matching read/release/derived-output revocation controls and team sharing support. Do not loosen Phase 9 privacy globally.
6. Add optional incremental regeneration, bounded multi-stage summarization and optional auto-compaction after the explicit path is correct.

Release-blocking fixtures:

- Ordinary fork/switch/tree read makes zero provider calls.
- Carryover imports only the selected old-branch suffix (plus declared background), preserving original destination context.
- Multi-source selections deduplicate overlapping immutable evidence, not coincidentally identical text.
- Cross-session identity collisions do not alias; independent sessions have no invented common ancestor.
- Snapshot source can advance without mutating a saved summary. Uncommitted source text never sneaks in.
- Incomplete/failed tools remain labeled as such and are never replayed or reported successful.
- Human checkpoint semantics and old exports continue to work; additive imports cannot erase prior context.
- Source read allowed but provider/destination disclosure denied performs zero dispatch/attachment.
- Private-source to broader-audience target is blocked; future membership expansion and revocation gate derived outputs as well as source links.
- Compaction of imported content retains transitive restrictions and attribution.
- Summarization uses requester's permitted account; source authors' credentials are never restored or billed.
- Local personal-device consent includes summary-purpose/source scope, with no secret upload or automatic fallback.
- Concurrent source edits, target-head changes, racing apply, lost acknowledgements and owner crash create neither duplicate imports nor blind paid retries.
- One summary can be attached twice after authorization without duplicate generation billing; separate synthesis/guardian calls have their own receipts.
- Under-sized budget fails or reports an explicitly partial selected scope; it never claims an unread whole tree was summarized.
- Cross-worktree file/test results are attributed to their original context; no filesystem patching, hazard clearing or new tool grant occurs.
- Output is plain attributed context; malicious source instructions cannot turn summarization into a tool or privileged command.
- Export/import cannot launder restriction metadata or trust external authorship/usage.
- Unused feature starts no branch scan, summary service, background model call or new listener.

The accompanying graph fixtures exercise the intended selection rules in a small independent reference. They are not the Rust implementation, full authorization, model quality, protocol or migration tests.

## 14. Pi comparison and evidence

Pi's primary compaction reference distinguishes ordinary compaction from branch summarization triggered by tree navigation. Its `collectEntriesForBranchSummary` walks from the branch being left to the common ancestor; the summary can be inserted at the destination. Pi also provides extension hooks, so this is not a claim that extensions cannot implement richer policies or summaries.

aw's desired additional behavior is first-class arbitrary multi-source selection, cross-session context flow, exact immutable provenance, access/disclosure propagation, requester-account selection and personal-executor compatibility. Neither the current manual checkpoint nor Pi's described single-navigation path should be mislabeled as that entire feature.

See [PI-CONTEXT-PARITY.md](PI-CONTEXT-PARITY.md) for exact audited aw files and the primary Pi references. The current package remains Phase 9 / 0.10.0 with a documentation amendment; no phase tag or runtime feature flag is advanced by this change.
