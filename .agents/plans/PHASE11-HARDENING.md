# Phase 11 — native SDK boundary hardening

**Source candidate 0.12.0.** This is a correction/readiness phase after the Phase10 context-flow implementation, not another platform feature layer. Actual Rust compilation/tests and Nix execution remain unavailable in the authoring environment. Validation gate defines what the compiler host must execute; [build status](../docs/BUILD-STATUS.md) records the distinction.

## 1. Sticky stream rejection

The provider contract requires an adapter to honor the `DeltaSink` result. Before this change, a native adapter that ignored a rejected fragment and later returned a completed response could leave accepted prefix text looking like a successful answer. The supplied HTTP adapters normally propagate these errors, but third-party compiled SDK providers must not be able to turn a refused stream into success through an accidental ignored Result.

Both `Agent::prompt_as` and `SummaryExecutor::execute` now retain the first stream fault themselves. Later callbacks cannot append more text or emit more deltas. An eventual completed provider return does not erase the fault. Chat records a failed attempt (which is not reused as successful context); a summary records failure with no partial candidate. If the adapter returned provider-reported metadata, the record retains it without calling it verified billing or inventing zero usage.

The chat invocation outcome can now be `stream_rejected`. Existing consumers should treat the outcome string as an explicit failure, not enumerate only `response_received` and `failed_or_cancelled`. It is not a new SQLite or device-wire format. A native adapter that blocks forever still needs cooperative cancellation and remains trusted code; this is not isolation against malicious Rust.

## 2. Recheck after a persisted intent

Journal writes can await. An account or source release that was valid before an intent write may be invalid afterward. The loop now checks the current selected lease and source context again **after** persisting model intent, before entering/polling provider code. It also revalidates immediately before a tool implementation is entered, after its start journal and observer notification.

A refusal at that point settles a failed/interrupted attempted operation. No new provider/tool is called merely to finish the journal. A recorded start remains conservative evidence; it cannot prove the external provider was billed. An uncertain mutating intent can still require inspection even when the dispatcher believes it was denied before entry—do not erase recovery evidence or weaken an existing hazard guard.

These checks narrow known asynchronous gaps. They do not make policy revocation atomic with an external HTTP server, shell, or malicious same-user process. Already-delivered text and executed effects cannot be recalled.

## 3. Summary-specific checks

The initial authorization stage is now inside the request's cancellation/deadline scope. A pending source gate can be cancelled without waiting for it to return before the first intent exists. Intent creation and final durable settlement retain their deliberate non-abandon semantics; a blocking or failed native journal remains a recovery problem, not a provider retry.

After `SummaryJournal::begin`, the executor checks source authorization and the selected account again, then validates the returned invocation's exact Summary purpose, input digest, evidence list, selected identity and nonzero ordinal. A chat-purpose or mismatched-account record is refused before provider dispatch. The trusted store journal still owns the target, operation ID and durable provenance; these checks do not authenticate an arbitrary user-provided journal.

After every `after_summary` hook has finished, current account/source access is checked again before a candidate can be accepted. The store's final transactional checks remain necessary. No hook result is a lasting exemption from current access rules.

## 4. One extension-registration rule

`AgentBuilder` and standalone `SummaryExecutor::new` share the same extension-name/count validator: bounded unique portable names. A summary-only consumer cannot accidentally bypass the registration invariant by skipping the main agent builder. Existing valid extension implementations keep the same public trait. No dynamic library ABI, downloaded plugin execution or new dependency enters `aw-agent-core`.

## 5. Regression source

[Core regression tests](../../crates/aw-agent-core/tests/hardening.rs) deliberately supply adapters that ignore callback errors, revoking journals and output hooks, mismatched summary identities, a pending authorization gate, and a tool whose entry is counted before its returned future is polled. The tests assert failed/no-candidate outcomes and zero provider/tool entry where authority changed before dispatch.

**14 new Rust test declarations are authored, not executed here.** Run them with a real compiler:

```sh
cargo test --locked -p aw-agent-core --test hardening
```

The full host gate includes this target through the workspace all-targets suite. Python runner tests are independent evidence of the orchestration utility; they are not substitutes for these Rust regressions.

## 6. Compatibility and scope

No new API-key field, provider destination, server listener, model classifier, scheduler or CRDT service is introduced. Same per-user account selection, private-device key custody, tool root, context release, serialized history, source import/compaction and exact application receipts remain. SQLite stays schema2 and device protocol stays wire2. Provider/account failure never falls back to a common account.

The default single-binary path remains minimal. Added checks execute within existing admitted operations, not during tree rendering or UI initialization. Additional database authorization reads may have a runtime cost; do not claim this review measured performance. Unknown type/lifetime/library errors in this or earlier phases still need the real compiler gate.
