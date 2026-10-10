# Independent Rust process extension — JSONL v2

This is a **standalone executable**, not a Rust dylib or a second aw Agent.

Build from the aw workspace:

    cargo build --locked -p aw-process-extension-rust-example

Inspect what it advertises (discovery does not grant tool use):

    aw extension --extension-process box=/ABS/PATH/aw-process-extension-rust-example --json

Render its **untrusted text** explicitly:

    aw extension --extension-process box=/ABS/PATH/aw-process-extension-rust-example \
        render box hello name=Alex

Run it with an already configured model, a precise tool grant and optional policy hooks:

    aw --profile llama --model MODEL \
        --extension-process box=/ABS/PATH/aw-process-extension-rust-example \
        --extension-allow box:echo \
        --extension-hook box:prompt \
        --extension-hook box:tool.call \
        --extension-hook box:tool.result --print "Example extension action"

The child implements the version 1 JSONL envelope described in [accepted history](../../.agents/plans/accepted-history/TODO.md). It owns no provider/account selector, SecretStore, SQLite connection or Session history. Only the current host's explicitly granted tools and hooks are active. An external executable remains OS-user-trusted code, **not** an OS sandbox. The source/hook policy is separate from a model-generated request or remote metadata.

The executable uses std/serde_json as a standard Rust process; the core-only/alloc no_std embedded Agent E2/E3 remains separate, and R7 does not claim it is implemented.