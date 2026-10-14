# aw-tools — explicit native tools

Trusted host-configured Linux file access and optional bash, implementing the `aw-agent-core` tool contract. The SDK itself does not depend on this crate.

`off` registers nothing; `read-only` adds read; `workspace` adds create-only write and exact-match edit; `trusted` also adds an unsandboxed configured shell. Model input cannot select the root, account, execution host, shell executable or policy.

No OS sandbox, arbitrary-user service, remote bridge, or credential-isolated process runner is claimed. A shell with the user's permissions can read that user's files even though environment inheritance is cleared. See the package tools/credential-placement documentation and host validation.
