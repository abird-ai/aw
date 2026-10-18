# Phase 36 — typed Clap command/help architecture

Phase 36 replaces the remaining manual argv dispatch/reparsing with one typed Clap command tree while preserving the existing runtime semantics and concise bare `aw` experience.

Schema remains **SQLite v20**. Phase-35 owner/direct/no-replay invariants are unchanged.

## 1. Why this phase existed

Before Phase 36, public commands were split across two mechanisms:

- bare `aw` used one large Clap parser;
- resource commands and roles were recognized manually from `std::env::args_os()`, then reparsed by separate mini-parsers.

Consequences:

- `aw --help` did not show the real command surface;
- help text and execution grammar could drift;
- `workspace open`, `session open`, `attach`, `team`, and `device` relied on argv rewriting/reparsing;
- role-specific help inherited irrelevant options from one monolithic run parser.

Phase 36 makes Clap the single public command-definition/help source.

## 2. Root command tree

The default build exposes these typed top-level commands:

```text
workspace
session
domain
machine
fleet
identity
context
auth
provider-account
oidc-provider
oidc
oauth-connector
net
attach
team
device
agent
web
```

Bare `aw [OPTIONS] [PROMPT]...` remains the minimal interactive/immediate-run path.

`aw --web` remains a compatibility spelling for canonical `aw web`.

Prompts that begin with a reserved command word can be escaped conventionally:

```sh
aw -- web is part of the prompt
```

## 3. Resource commands

`src/resources_cli.rs` now exports reusable typed `Subcommand` enums rather than reparsing process argv.

Examples:

- `WorkspaceCommand`
- `SessionCommand`
- `DomainCommand`
- `IdentityCommand`
- `ContextCommand`
- `ProviderAccountCommand`
- `AuthCommand`
- `OidcProviderCommand`
- `OidcCommand`
- `OAuthConnectorCommand`
- `NetCommand`

The root dispatcher receives the parsed enum value directly.

There is no `nested_argv` compatibility parser and no `parse_canonical_args` argv rewrite.

## 4. Special roles are part of the same tree

The former independent parsers are now reusable Clap nodes:

- `attach::Args` + `attach::run_args`;
- `team::Command` + `team::run_command`;
- `device::Command` + `device::run_command`.

Feature-gated roles appear only in builds where their implementation is compiled.

## 5. Composable run arguments

The runtime still receives one internal `RunArgs` value, but the public parser is split into small purpose-specific groups:

- `ProviderCliArgs`
- `ToolCliArgs`
- `TargetCliArgs`
- `OpenTargetCliArgs`
- `OwnerTargetCliArgs`
- `WebCliArgs`
- `OwnerCliArgs`
- `ManagementCliArgs`
- `RootRunCliArgs`
- `OpenCliArgs`
- `WorkspaceOpenCliArgs`
- `AgentCliArgs`
- `WebCommandCliArgs`

This keeps execution logic single-source while allowing role-appropriate help.

Concrete cleanup:

- `aw session open SESSION` no longer redundantly advertises `--session` or `--workspace-layout`;
- `aw workspace open NAME` owns the optional layout path;
- `aw agent` no longer advertises an initial prompt that owner/service mode never consumed;
- `aw agent` no longer advertises `--no-state`, which durable owner mode rejects;
- remote-control tuning fields are compiled only when both `net` and the owner/web transport are available;
- reduced builds no longer retain dead role-only runtime fields.

## 6. Help is side-effect free

Help/parser construction does not:

- open a SessionStore;
- create/migrate a database;
- open a provider endpoint;
- read provider secrets;
- create a listener;
- start Git/PTY/browser resources;
- perform inference.

`aw --help` and nested `--help` are generated from the same Clap definitions used for dispatch.

## 7. Actual-binary validation

the removed cli_help_smoke helper validates the compiled executable, including:

- the complete default-build root command list;
- important Workspace/Session verbs;
- nested help for attach/team/device/agent/web;
- an escaped prompt beginning with a reserved subcommand word;
- no inference requirement for the parser/help checks.

The smoke is wired into the native validation profile.

Historical Phase-30–33 static checkers were updated to assert the typed Clap tree rather than deleted parser-wrapper implementation strings.

## 8. Preserved compatibility and boundaries

Unchanged:

- bare `aw` interactive flow;
- `aw --print ...`;
- compatibility `aw --web`;
- canonical `aw workspace open NAME`;
- canonical `aw session open SESSION`;
- `aw agent` / `serve` alias;
- resource authority semantics;
- provider/SecretStore custody;
- Phase-35 storage ownership and no-replay rules;
- Pi-like inline / Codex-like fullscreen / zero-model local web behavior.

## 9. Validation

Validated on the exact Phase-36 tree:

- Phase-36 typed CLI/reference checker: **88**;
- historical Phase29–35 checkers: **75 / 142 / 159 / 120 / 119 / 101 / 47**;
- default/minimal/web/net/OIDC feature cuts: green;
- full `cargo check --locked --workspace --all-targets`: green;
- full `cargo test --locked --workspace --all-targets`: green;
- compiled the removed cli_help_smoke helper: green;
- inline/live/local-web/fullscreen workspace actual-binary smokes: green;
- source/static validation: **807 active files**, **85 Python sources**, **689 local links**, **0 errors / 0 warnings**;
- design-doc reference validation: green;
- complete reference profile: **38/38**;
- validation plan sizes: **104 rust / 106 full**.

The separate full-host Clippy/Nix release gate remains environment-pending.

## 10. Next phase

**Phase 37 — first-run provider/model onboarding.**

When no executable provider/model is configured, bare interactive startup should offer a concise, cancellable forward flow using the same persisted provider/profile model already shared by native Settings and local-web Settings.

The onboarding phase must:

- work before inference exists;
- support only provider families/custody flows that are actually implemented;
- never fabricate or silently fall back to credentials/models;
- preserve zero-model `aw web` Settings startup;
- allow skip/cancel;
- make no provider call merely to save configuration.
