# AI-native operations, Agents, Jobs and transcript — implementation plan

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-6, ChatGPT GPT-5.6 Sol
Owner: @pvl

Status: **design reviewed/resolved; implementation not authorized** (2026-11-10).
Design authority: [DESIGN.md](DESIGN.md); concise overview: [README.md](README.md).
Baselines at draft: AW `4a23e92a8f516971bb232b98e4fc000554bcc4ba`; dotlink `b31683c79a8ffd7c5540ac9388e607d8af6c7ffa`. Revalidate live source at implementation time. Do not modify older plans or coordinator-owned global indexes.

## Committed

None. No source implementation, tests or commit performed for this new plan.

## Phase 0 — design decisions (documentation completed)

- [x] Keep DL-150's 13 orthogonal native tools and fixed Graph tuple, status/paging/compact-result contract.
- [x] Canonical semantic IR `[operation,args]`; one provider-visible projection with grouped agents/jobs; provider original ToolCall stays immutable journal evidence.
- [x] Effective-grant filtered grouped op schema, strict per-op validation; no double-advertised aliases, no duplicated Graph-node schemas.
- [x] One original Agent implementation; an Agent ID pins exact (Session, branch); fresh Agent identity and first Turn/Job are recoverable on a lost response.
- [x] Agent=durable conversation, Job=one live execution; Graph child Agent is one node projection, not an independently managed second Job ledger.
- [x] Five effects Read/Write/Process/Control/Unknown; Graph outer Control admission **plus** exact authorization of every leaf; non-authoritative EffectSet only.
- [x] ReadOnly filesystem policy is distinct from explicitly granted Job control and bounded Agent delegation; denied operations stay denied.
- [x] `get`=status, `output`=retained bytes, `ack`=consumption, `send`=Input, `cancel`=termination, `subscribe`=future wake rules; `wait_ms` on get/output.
- [x] Parent-owned wake `settled/attention` at `now/step/turn`; interactive child with only blocked parent controller proactively hands off same Job.
- [x] Graph immutable, interactive child input separate; graph nodes reject background; typed bread/media is Graph-eligible without changing captured node modifiers.
- [x] `agents.read` has one `filter`: omitted/`default` selects latest result, `none` selects all currently authorized public categories, and a custom object matches category booleans/optional tool names; `exclude:true` removes matches from the authorized public view. Independent disclosure, byte bounds and final-answer dedup remain mandatory.
- [x] Original provider tool-call provenance distinct from resolved semantic operation for transcript tool filtering.
- [x] Distinct bash/pwsh/powershell interpreters; retained-Graph restart advanced/legacy capability preserved; peer/sibling messaging deferred.
- [x] DL-150 token policy remains normative; measure actual advertised schemas + representative model workflows, not just fewer tool names.
- [x] Keep all previous specifications, implementation and platform release gates untouched.

## Phase 1 — canonical trusted operation/effect layer (AW first)

- [ ] Obtain explicit implementation authorization; re-read live AW/dotlink source and existing dirty work before edits.
- [ ] Add Effect::Control in original AW kernel/host effect mappings with no_std/minimal/stateless support and no widening of old grants.
- [ ] Implement one trusted `PreparedOperation` resolver; preserve original provider ToolCall + effective resolved operation/source/epoch/effect/arguments as distinct journal facts.
- [ ] Advertise filtered grouped jobs/agents variants only when exact per-op grant present; strict per-op schema/argument validation and original provider correlation.
- [ ] Add `batch` outer Control admission, per-conditional-leaf exact effect/resource preflight and current-policy revocation; ReadOnly + all-Read Graph must work.
- [ ] Use original ToolPolicy/Envelope/ToolBindingAuthority/TurnJournal and only the existing scheduling/commit engine; no alternate policy/execution paths.

## Phase 2 — exact durable Agent identity and creation receipt

- [ ] Derive/bind opaque Agent ID from existing owned Session+exact branch identity, with no separate Agent mapping database or branch-name retargeting.
- [ ] New `agents.run` first-turn admission records Agent/branch/Turn/Job/replay receipt before paid effects; identical redelivery reattaches and changed envelope fails closed.
- [ ] Synchronous successful fresh Agent returns short Agent ID plus once-only answer; background returns short Agent/Job IDs; suppress duplicate wake.
- [ ] Expose retained/recoverable Agent ID to authorized `jobs.list/get` and existing Session discovery after Job expiry; no cross-process Job resurrection.
- [ ] Follow-up `agents.run({id})` requires exact bound branch/writer/lineage/current grants and creates new Turn/Job; fork/new branch gets new Agent ID.

## Phase 3 — authorized, byte-bounded Agent transcript projection

- [ ] Implement `agents.read` over current SessionStore/immutable Turn evidence; default latest committed result, one Turn; avoid full Session export on ordinary read.
- [ ] Add strict `id/filter/turn/before/limit/byte` schema: `filter="default" | "none" | {result?,messages?,tool_calls?,tool_outputs?,tools?,exclude?}`; require at least one true category, `tools` only with a true tool category, `exclude:true` to subtract matching records from the authorized public view; exact disclosure, no `include`/top-level `tools` aliases.
- [ ] Preserve older-Turn `more before=N` and oversized exact-Turn `more turn=T byte=B` with UTF-8 advancing slices, the **same filter** on continuation, stable disclosure, and no false completeness.
- [ ] `messages` is full sanitized user/assistant conversation including final answer; when combined with `result` publish final answer once; `filter:"none"` requests all **authorized** categories, not unsanitized or unlimited content.
- [ ] Preserve original provider tool wire name/call ID and verified resolved semantic name; exact operation filters cannot infer canonical identity from untrusted `op` text.
- [ ] Add host-owned projection category ceiling and current per-resource disclosure checks; deny hidden context/reasoning/system, sensitive/revoked outputs, credentials and other branches.
- [ ] Active/unreconciled branch transcript read fails closed; compacted model-context tail does not masquerade as full durable history.
- [ ] Return original truncation/unknown facts truthfully; transcript reading never invokes a provider or advances Job output ACK.

## Phase 4 — unified Job management/observation

- [ ] Add grouped `jobs.list/get/output/ack/send/cancel/subscribe` over shared canonical JobManager and product owners, exact per-op capability on each call.
- [ ] Default compact Job listing omits title and data; optional explicit detail/UI can display title; status never implicitly returns/ACKs bytes.
- [ ] `get(wait_ms)` waits on meaningful status revision; `output(wait_ms)` waits on bytes/EOF; bounded expiry is truthful nonterminal current state, not an execution timeout.
- [ ] Preserve raw-byte offsets/ACK owner, stream sealing, MCP Task and transport replay ownership; no global execution lock held while waiting.
- [ ] Keep distinct authorized interpreter launchers and interactive stdin `text xor eof` as explicit Process input effect; no implicit shell/FS grant.

## Phase 5 — factual settlement, safe wake and interactive liveness

- [ ] Register parent-owned wake on Job admission before roots run; use existing AW Actor Notify/Input/owner receipts and no generic event DB.
- [ ] Project shell settled only after factual Process status and stream writer quiescence; Graph/Agent settle from their original owners.
- [ ] Distinguish direct synchronous result vs async handoff and deduplicate fast completion, lost response, replay, revoked parent and unknown Input.
- [ ] Implement `now/step/turn` at actual Agent safe boundaries; Step includes full tool batch; steering yields new admitted Turn without mutating provider-signed messages.
- [ ] Proactively hand off Graph/Agent creation when an interactive child has only its presently blocked parent as controller; no paid-model callback inside blocked Tool call.
- [ ] Make `attention` exclusively authenticated child-originated, bounded and never forged from stdout/progress/transcript; keep parent controls serviceable while waiting.
- [ ] `jobs.subscribe` replaces/clears current recipient's future wake rule; no post-terminal retroactive invocation.

## Phase 6 — immutable Graph host Agent nodes

- [ ] Expose one transport-neutral node executor/registry entry; AW host binds exact original Agent/Session/TurnJournal, dotlink standalone never invents an Agent.
- [ ] Preserve captured initial arguments, per-node semantic result/work ceilings, conflict footprints and fixed `when` branches; no nested batch, jobs controls or agents.read.
- [ ] Reject Graph-node background, even if true, while preserving top-level background as response disposition; authorize conditional nodes before any roots start.
- [ ] Distinct Graph Agent nodes cannot concurrently write the same branch; child steering is separately admitted Input and cannot widen the original child Envelope.
- [ ] Settle one terminal Graph node only from original child Turn fact; follow-up through `agents.run({id})` is a new top-level Turn/Job, not a Graph rewrite.

## Phase 7 — typed Graph output (shared aw-tools)

- [ ] Replace text-only retained-result projection with Text/ExactRead/Bytes/Media, reusing original secure direct executor and media host validation.
- [ ] Enable `bread(format=mcp)` in Graph; preserve original captured per-node `more`/`incomplete` and dependency outcomes under outer transport/presentation limits.
- [ ] Preserve raw-byte output paging, typed MIME/hash/format permission, ACK owner and replay/native Task bytes; do not duplicate large payloads in status or transcript.

## Phase 8 — dynamic policy and typed SDK projection

- [ ] Reuse original typed hooks/Next/Defer, ToolPolicy/Envelope and journal for optional host-chosen rule/program/QuickJS/Python/MCP/model/pipeline/human evaluators.
- [ ] Decision data allow/deny/narrow/ask cannot mint missing grants, payer, root or site; fully re-resolve and reauthorize narrowed calls; bounded reviewer cost/unknown handling.
- [ ] Implement typed SDK `tools.read`, `tools.agents.run/read`, `tools.jobs.*` and generic `tools.call([op,args])` over the **same** trusted resolver; not an extra model tool.
- [ ] No ambient script OS/network/subprocess/credentials. Optional SDK watch is passive observation only.

## Phase 9 — thin dotlink integration and deliberate public cutover

- [ ] Consume one reviewed exact AW Git revision; no sibling path or copied shared Job/Graph/filesystem engine.
- [ ] Adapt dotlink grouped jobs wire to exact semantic resolver; preserve legacy job_* and authorized advanced retained-Graph restart during migration.
- [ ] Preserve OpenAI tunnel replay, native MCP Tasks, profile denies, sandbox, protected paths, runtime Job session, raw-byte output/ACK.
- [ ] Advertise only actually prepared/allowed bash/pwsh/powershell interpreters; no altered interpreter alias.
- [ ] Keep standalone dotlink without Agent run/read/wake; only route through an authenticated AW host with original owner and disclosure checks.
- [ ] Review actual generated schema and model-workflow metrics before authorizing old/new advertised schema cutover. No silent releases/versions/pushes.

## Tests and verification

- [ ] One concise provider schema/admission gate: filtered op visibility, strict invalid op/fields, original/effective call provenance, all five effects and ReadOnly Graph Control/leaf authority.
- [ ] Agent identity/branch/fork/redelivery test: one paid Turn/Job on lost response and no silent branch retarget.
- [ ] Transcript filter tests: omitted=`default`, `none`=all currently authorized public categories, custom true category selection, `exclude:true` both with whole-category and exact tool-name subtraction, invalid empty predicate, forbidden positive category denial versus benign exclusion of denied category, duplicate result/message suppression, current redaction, active/busy/compacted history and progressing byte cursor.
- [ ] Job/Graph liveness: blocked-parent interactive child immediately hands off; current Step includes all tool receipts, no zombie Graph child/double manager.
- [ ] Shell settled writer-quiescence, early/duplicate wake, revoked/unknown parent, node/Graph attention subscription and bounded wait_ms get/output.
- [ ] Typed bread/media Graph result, fixed node modifiers, no outer-envelope semantic drift, raw-byte ACK/Task/replay parity.
- [ ] Original AW no_std/minimal/stateless, shared filesystem Unix/Windows, Agent/SessionStore and dotlink stdio/MCP/Graph/OpenAI replay suites; current native platform limits tracked honestly.
- [ ] DL-150 **model-first efficiency gate**: measure generated serialized schema bytes + tokenizer estimate, first valid model tool-selection rate, total tool/model rounds and returned tokens for read, 3-node read Graph, Bash handoff, Agent create/result, default/none/custom agents.read filtering and read-only jobs Control.
- [ ] No gratuitous fixture/evidence bureaucracy; reuse existing regressions and add only targeted high-signal contract tests.
- [ ] New plan edits only; previous DL-150/previous plan files, global plan indexes, implementation source, external commits and other agents' work untouched.

## Execution guard

Documented design decisions are marked complete, not implementation. Any source edit/test/commit/push/version/release requires explicit approval. Coordinator alone promotes the plan in `.agents/plans/TODO.md`. Do not overwrite unrelated working-tree changes or reserve another agent's directory.
