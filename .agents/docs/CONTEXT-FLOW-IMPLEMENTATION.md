# Explicit context flow — Phase 10 / 0.11.0

**Authored source implementation; Rust/Nix execution remains unvalidated.** This document describes the actual candidate API and limits. The [accepted design](CONTEXT-FLOW.md) and [Phase 9 audit](PI-CONTEXT-PARITY.md) remain dated baseline references, not claims that every future option shipped here.

## 1. Replacement versus addition

`context_import` is an immutable node with one parent and a summary-artifact reference. Projected history keeps earlier completed target turns, inserts the attributed block at its position, then later completed turns. Tool records stay data; replay does not execute them. `context_compaction` carries a declared first-kept node of the **active projection** and replaces its prefix while retaining the selected coherent attempt tail. The original nodes remain.

Legacy `checkpoint` still takes user-supplied text and cuts earlier active context. `HistoryState` version 2 adds ordered context blocks; version 1 is accepted only with no such blocks. Context text is an untrusted user-data message, not a system policy, permission, verified result or live tool call.

A one-prompt attachment is pending until a real prompt admission consumes it transactionally. It is not automatically pasted into every future provider context. The answer and later descendants still inherit source restrictions. Retry of a failed attempted prompt does not silently recreate a consumed one-shot attachment; inspect and deliberately attach again when appropriate.

## 2. Preparation and sources

`PrepareContext` selects an exact `TargetHead` and up to 16 sources. A source is a session/branch plus optional immutable head and selection:

- `standalone`: the source's active semantic path, respecting prior checkpoints/compaction.
- `divergent`: the suffix after the deepest common ancestor with the destination; only within the same session.
- `range`: the path after a named `start_exclusive` ancestor to the selected head; unsafe half-prompt boundaries are rejected or represented only by complete attempts.

Branches with a running prompt use the prior eligible committed boundary unless an explicit invalid head is selected. Preparation is transactional, pins snapshots and deduplicates exact `(authority, session, node)` evidence. It never follows all sibling branches or reads current worktree files. Prior summaries retain inherited source dependencies rather than falsely appearing independent corroboration.

Input is bounded and completely supplied for the selected scope or rejected. The selected raw path guard is 32 MiB; evidence envelope is at most 512 KiB, dependency count at most 512. Exact bytes include public source statements, outcomes and allowed tool records, not private provider reasoning/continuation or credential configuration. Model facts remain unverified historical observations; file/test status is not automatically current.

A prepared plan records the current release epochs and exact target snapshot. Source advancement does not change it. A target advancement can make application stale; retain the candidate and choose a new compatible target explicitly. Preparation another destination does not move the current UI. The application permits only destinations compatible with the current executor's saved directory/tool/instruction/destination binding; summaries never silently switch filesystem roots.

## 3. Separate operation and artifact lifetime

```text
prepare (no inference) → immutable plan + evidence digest
                                |
                  manual candidate OR explicit generate
                                |
          durable operation intent → tool-free provider call
                                |
               outcome + optional usage + immutable artifact
                                |
                         preview / human edit
                                |
        exact apply → append / fork / new session / one-shot / compact
```

`SummaryExecutor` applies current account leases, before/after-summary hooks, local limits and `SummaryJournal`. No tool registry or main-agent prompt is invoked. Two messages form a fixed tool-free envelope. The journal commits `started` before polling the provider; the summary uses a session-global invocation ID without fabricating a chat prompt or holding the branch writer through network work.

Only nonempty, fully completed tool-free output becomes a candidate. Failed/cancelled/incomplete operations retain status and known or unknown usage; partial text is not a successful artifact. Output stays private/buffered until current source/disclosure and result checks pass. A crash recovers started operations as interrupted; it does not regenerate or replay a provider call.

Human text produces an attributed `human` artifact with no model charge. Editing a candidate produces a `human_edited` artifact linked to the original under the same plan. It does not invent extra source coverage, a new verified result, or borrowed privileges. Summarization focus and all evidence remain untrusted; compiled hooks are trusted native code, not an untrusted pipeline sandbox.

## 4. Exact command contracts

All action values below are proposed-current **source API**, pre-1.0. Replace placeholder IDs. Native `--context-action FILE`, inline `/context-flow JSON`, native palette `context-action JSON`, attach, and the browser gateway use `aw::context_flow::Command`. No actor, endpoint, key or payer is accepted from this payload.

```json
{"op":"target"}
```

The response has `session_id`, `branch_id`, `head`, `revision`. `sources` lists readable source sessions; `targets` lists compatible owned destinations; `target_at` selects another owned/authorized compatible branch without moving the current view; `branches` lists permitted branches for a source.

```json
{"op":"prepare","request":{"target":{"session_id":"ACTUAL_SESSION","branch_id":"ACTUAL_BRANCH","head":"ACTUAL_HEAD","revision":2},"sources":[{"session_id":"SOURCE_SESSION","branch":"SOURCE_BRANCH","head":null,"selection":{"kind":"standalone"}}],"kind":{"kind":"import"},"focus":"Preserve decisions, test outcomes and conflicts with source labels.","max_input_bytes":131072,"max_output_bytes":16384,"timeout_seconds":120}}
```

The result includes `plan` and `input_sha256`. The exact digest is useful for a separate local-device consent decision; it is not evidence that the source statements are true. Save/retrieve by `{"op":"plan","plan_id":"..."}`.

```json
{"op":"manual","plan_id":"PLAN_ID","request_id":"manual-unique-1","text":"Human-written summary of the reviewed evidence."}
```

```json
{"op":"generate","plan_id":"PLAN_ID","request_id":"summary-unique-1","account_id":"MY_ALLOWED_ACCOUNT","model":"MY_ALLOWED_MODEL","confirmed_disclosure":true}
```

The web returns 202 and a request ID; query `{"op":"operation","id":"summary-unique-1"}`. The direct native executor waits cooperatively and returns the stored operation. An exact retry is looked up; changed input or reused IDs conflict. `artifact` requires the returned `artifact_id` and current access. A candidate is private to its requesting owner, even if the source is shared.

```json
{"op":"apply","request":{"artifact_id":"ARTIFACT_ID","target":{"session_id":"TARGET_SESSION","branch_id":"TARGET_BRANCH","head":"EXPECTED_HEAD","revision":2},"mode":{"kind":"append"},"request_id":"attach-unique-1","confirmed_disclosure":true}}
```

Other apply modes:

```json
{"kind":"fork","name":"with-findings"}
```

```json
{"kind":"new_session","title":"Follow-up investigation"}
```

```json
{"kind":"next_prompt"}
```

```json
{"kind":"compact"}
```

`compact` requires a compact plan prepared for that exact target. Prepare with `sources:[]` and `kind:{"kind":"compact","keep_recent_turns":2}`. The tail counts semantic attempt records, including unsuccessful records retained for visible provenance; only completed traces become normal successful model context. Retaining all current attempts yields an empty prefix and an explicit no-work error.

Apply creates no model request. Identical attachment request and fingerprint return the same receipt. A changed target needs a **new** application ID. `application` retrieves an existing receipt without moving views or reexecuting it. `pending` and `discard_pending` expose explicit one-shot handling. The app activates the selected result after successful apply; the store SDK itself merely returns the new reference.

## 5. Read-only sharing and inherited source restrictions

A source owner can use:

```json
{"op":"grant_read","session_id":"OWN_SOURCE_SESSION","principal":"internal:engineering:bob","allow":true,"summary_accounts":["bob-laptop"]}
```

The browser restricts recipients to enabled same-realm participants; trusted native callers provide authenticated host identities. This grants **read-only session access**, plus explicit account-ID disclosure permission for derived provider use. It grants no source append, model key, arbitrary account, tool, filesystem or billing authority. Empty `summary_accounts` permits reading but no model use of that source. Revocation uses `allow:false` and an empty list.

The destination's entire audience must be allowed to read every dependency. Original owner sessions are private by default. Team mode still denies opening another user's session as its active writable agent target: a shared peer source is input, not an editable target. The SDK supports an explicitly authorized destination member where its full audience satisfies the same rules; the first UI intentionally keeps target selection owned/compatible.

A release receives an immutable epoch identifier. Every import and derived prompt/result/checkpoint records its dependency/release closure. Read access, model account permission and release epoch are rechecked during plan/generation/preview/application, normal model context assembly, history viewing and export. Changing or renewing a release invalidates artifacts from the old epoch; do not assume regrant restores an old derived conversation.

Revoked source dependency means affected derived output is unavailable, not just that its source hyperlink disappears. The first read protection is deliberately conservative: querying a session can be denied if another branch within it contains revoked derived content. It does not attempt word-level sanitization. Opening a clean private session is the supported recovery, not erasing history or pretending the restricted answer is safe.

Account identifiers name permissions under the current trusted host configuration. They are not cryptographic proof of an underlying provider destination or a defense against a malicious owner changing code. No external declassification, per-node group policy engine, end-to-end encryption or remote authority federation is added. Already delivered text cannot be recalled. A human-authored summary is also lossy source-derived content; manually rewriting words is not permission laundering.

## 6. Normal execution and output fencing

`TurnJournal::validate_context_as` receives the current invocation identity. A later user changing their main account must still use a source-approved account; permission to generate once under one account does not permit future disclosure to another provider.

When active context consumes source-derived dependencies, the core buffers output events until final current-access checks after the turn journal commits. Stream text and tool output are not sent to observers speculatively and then withdrawn. Started/progress metadata still represents the admitted operation. Root tool effects may occur under already-granted execution authority; revocation does not roll them back, and settlement cannot be skipped simply to hide the event.

Observers cannot authorize tools, repair persistence or override a denied dependency. The default journal remains compatible for ordinary own-context callers. No listener, model call, branch scan, source-sharing timer or extra SDK service starts merely because a normal chat is rendered.

## 7. Storage, accounting and portability

Migration `002_context_flow.sql` upgrades `user_version` to 2 in the existing single-owner SQLite store. It deliberately rebuilds the node-kind constraint while retaining IDs, parents, row sequence and immutability triggers. Foreign-key checking brackets the controlled transaction; unknown/corrupt state fails rather than resetting a database. Keep a consistent backup before migration. No downgrade is supplied.

Artifacts/plans are immutable; operations and unique application receipts have exact transitions. Generation reserves a global native turn identifier but adds no prompt/node or hazard-clearing action. Usage is counted in the target session's native usage projection exactly once for the real model operation. Attachment and forks do not rebill the generating operation. Provider-reported missing usage is unknown, not zero.

Plans, artifacts and operations are capped at 4096 each per actor; application receipts at 65536. No implicit expiry converts an old accepted ID into a new paid request. Active context blocks cap at128, pending one-shots at8 per branch. There is no automatic garbage collector or unlimited summary archive promised by these bounds.

JSONL v2 carries context nodes, summary artifacts and needed one-prompt/owner-private evidence associations. v1 remains readable. Portable source export requires its owner; an ordinary peer read grant is not export permission. The exporter also refuses cross-owner derived dependencies because ordinary read release is not a portable declassification. An allowed export/import becomes a new owned private unverified session with remapped IDs and inspection required before inference; native account rights, release grants and billable invocation receipts are not imported. Raw context/export/backup text can contain sensitive information; no semantic secret scanner or encryption is implied.

## 8. Personal-device inference

Summary calls use `InvocationPurpose::Summary {operation_id,input_sha256,sources}`. Wire protocol2 authenticates these fields. A v2 local grant explicitly lists the exact prepared message digest through `--allow-summary`; all existing actor/model/target-session/branch/expiry/budget conditions still apply. Grant v1 remains chat-only and is not silently upgraded to authorize summaries.

The device independently hashes the exact two public messages, validates the declared evidence dependency list, refuses tools/private continuation on this request, and matches the reviewed digest before making its local provider call. A paired owner can lie about real-world source meaning; a digest binds approved bytes, not truth. Review the materialized plan locally before issuing consent. The browser never sends a provider key and the owner need not hold it.

The owner and worker must both use wire2; mixed versions fail. New grant creation/relinking and receipts follow the existing no-replay contract. Changing grants is an explicit operation, not a way to reset an uncertain paid request. A provider cancellation can still incur usage.

## 9. Host and SDK boundaries

The implementation adds modules to existing crates rather than another summarizing agent. `aw-agent-core` supplies pure data and a tool-free executor with an injected provider/account and journal. `aw-store` supplies canonical source authorization, preparation, dependencies, artifact lifetime and atomic application. `src/context_flow.rs` composes them for CLI/TUI/web/native-attach. SQLite work is performed in blocking workers in asynchronous server paths; no branch mutex remains held across model network I/O.

Native/attach controls accept exact typed JSON and print/render bounded result JSON; a future richer multi-select native widget is not claimed. The browser implements a lazy multi-source wizard, explicit generation/manual/edit/apply and source sharing, with requester-only account controls, stale-target recovery and context-revoked clearing. All use the same server operation, not independent JS authority.

The independent [SDK consumer](../../examples/sdk-context/src/main.rs) demonstrates the actual source API using a labelled fake provider and in-memory SQLite. Host harness exercises the compiled application and two authenticated users when run on a capable host. It does not substitute a Python implementation when the binary is absent.

## 10. Implemented source versus later design

| Accepted behavior | This phase |
|---|---|
| Plain fork/switch without inference | Preserved. |
| Single-branch carryover | Explicit divergent preparation + append/fork. |
| Multi-source branches/sessions | Same-authority authorized sources, bounded/deduplicated. |
| Manual/model candidate and preview/edit | Implemented source; model is generation-capable, no tools. |
| Additive import, new fork/session, next-prompt attachment | Implemented source; compatible target binding. |
| Explicit compaction with retained recent tail | Implemented source; no automatic threshold trigger. |
| Cross-user read and summary disclosure | Owner release to known participants/account IDs; owned team destinations. |
| Derived revocation | Conservative source/release dependency checks and output fencing, not arbitrary word-level declassification. |
| Personal credentials stay local | Extended paired device protocol with summary purpose and exact digest consent. |
| Generalized map/reduce, periodic refresh, on-hover generation | Not implemented. Oversized input fails explicitly. |
| Full per-node/group/federated sharing, independent sanitization releases | Not implemented. Unsupported broader publication remains denied. |
| Optional Jev/Laya guardians/router pipelines | Existing trusted extension hook seam only; no generalized pipeline runtime in this phase. |
| Actual compile/run, provider-quality and security assurance | Outstanding real-host acceptance, not implied by source or mock tests. |

## 11. Required acceptance

Run the supplied host script and correct all Rust/Nix failures. Exercise real source snapshots, different summarizer accounts, actual tool-bearing turns, exact migration backup/rollback, crash-after-start, duplicate Generate/Apply, concurrent target change, nested source revocation, device-v2 summary consent, and the live Pi-replacement workflow. Independent Python SQL/reference fixtures and Chromium fake transport cannot establish those results. See [BUILD-STATUS](BUILD-STATUS.md) and [current release validation](../plans/release-readiness/TODO.md).
