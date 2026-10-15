# SDK host consumer

An independent Rust executable embeds aw-agent-core with the example extension library and its own `Provider` implementation. It does not depend on aw-cli, Axum, rustyline or the HTTP provider crate.

```sh
cargo run --locked -p aw-sdk-example
```

The output identifies itself as an SDK fixture, not an LLM. The source belongs to the real Cargo workspace; compilation/execution must be verified on the build host. [Source](src/main.rs) · [SDK guide](../../.agents/docs/SDK.md).
