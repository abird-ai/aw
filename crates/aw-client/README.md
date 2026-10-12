# aw-client

Independent pre-1.0 Rust SDK for the explicitly started, same-user Linux aw owner. It implements bounded length-prefixed JSON over a private filesystem Unix socket. Both endpoints verify kernel peer credentials and the current owner instance. Calls, observation and semantic prompt receipts are separate.

No HTTP client, model provider, agent loop, filesystem tool, SQLite or UI is a dependency. Local socket metadata is checked; it does not open an aw state database or load client provider configuration.

`Client::connect` opens an explicit socket; `snapshot` reads state; `call` submits typed commands; `start_follow`/`next_event` observe one run. Use another authenticated connection for cancellation while following. An ambiguous call is not automatically retried. Retain the exact `PromptInput` and use lookup-only `PromptStatus` on reconnect. Model credentials remain in the configured owner in this phase; the personal local inference bridge is future work.

See [owner contract](../../.agents/docs/LOCAL-OWNER.md) and [protocol](../../.agents/docs/LOCAL-PROTOCOL.md). Source is authored, not compiled in the authoring environment.
