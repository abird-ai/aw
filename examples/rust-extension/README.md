# Rust extension consumer library

This independent workspace library depends only on `aw-agent-core`. Its `ExamplePolicy` implements one typed `on_hook` method for the `prompt` stage (a literal gate and request-local system contribution). The marker is not a secret detector or safety certification.

[Source](src/lib.rs) · [SDK guide](../../.agents/docs/SDK.md) · [host consumer](../sdk-host/README.md).

No shared library/dynamic plugin is loaded. The host deliberately compiles and registers this trusted Rust type. The same current SDK also supports typed tools and journaled sessions; this intentionally minimal extension example needs neither implementation dependency.
