# CLI and source compatibility review

Author: GPT-6 Codex
Owner: @pvl

Baseline: `47ee8a03abecbb3aee84a49c0a590096d76ed1a7` (accepted checkpoint).
Reviewed source: `769e9ef27f5edc88f24ae3fe6be1f8ddfe388596`.

Existing exported CLI flag entries are unchanged. Linux Full adds seven
command paths. Rust source consumers, native layouts and typed capabilities
consumers have upgrade boundaries below. Version/release approval remains open;
AW stays 0.15.0, SQLite v33 and JSONL v1.

## CLI results

Actual Linux x86_64 binaries, before → after:

| Feature cut | Command paths | Flag entries | Schema | Root help |
| --- | --- | --- | --- | --- |
| Full | 244 → 251 | 1,719 → 1,719 | 2 → 2 | Identical |
| Minimal | 23 → 23 | 107 → 107 | 2 → 2 | Identical |
| Stateless | 23 → 23 | 97 → 97 | 2 → 2 | Identical |

- No removed commands or added/removed/changed flag entries. Every existing
  `(command, typed Arg ID)` row matches, including spelling, short flag, heading,
  environment **name**, literal default, requiredness, values and description.
- Minimal and Stateless schema JSON is structurally identical across revisions.
- Full adds `aw workspace attach`, `aw workspace persistent`, and its `start`,
  `stop`, `inspect`, `adopt`, `serve` leaves. `adopt` and `serve` are hidden
  internal leaves. `inspect`/`adopt` are Linux-only; the other five paths have
  Unix source gates. Other operating systems were not executed in this review.
- Six binaries report `aw 0.15.0`. Schema/help/version probes create no config,
  State or launch-root files in empty private homes and working directories.
- Sixteen parser/metadata controls pass: invalid `cli` actions exit 2; existing
  Workspace help/`ls` alias help succeeds; new command help succeeds; missing
  required operands and an extra shell operand to `persistent adopt` exit 2.
  Explicit adopt help accepts only `<DIRECTORY>`; it does not launch a shell.
- Schema comparison does not exhaust positional arguments, conflicts or aliases.
  Existing typed CLI declarations were also reviewed; representative parser
  controls supplement the exported inventory.

## Upgrade boundaries

| Surface | Executed witness | Integration consequence |
| --- | --- | --- |
| `aw_terminal::Status` | A four-variant exhaustive match compiles at baseline; current source returns E0004 for Linux `Released`, `ExitedUnknown`, `Signalled(i32)`. | Update exhaustive matches. Preserve released custody, observed exit and unknown exit as distinct states. |
| `aw_workspace::Workspace` | Baseline field-complete struct literal compiles; current source returns E0063 for added container/scope/Git fields. | Pin source revisions and update literals. `Target::AgentView` and `Target::File.scope_id` also change source patterns/construction. |
| Native Workspace layout | Actual model readers accept their own v1/v4 JSON. Current model migrates synthetic v1 with an explicit launch `ScopeId`; baseline reader refuses v4 fields. | Forward migration is supported; a saved v4 file is unsuitable for the old binary. Preserve the old copy or use a separate `--workspace-layout-file` when testing an older revision. |
| `aw_surfaces::Capabilities` JSON | Both revisions emit `schema: 1`. Current deserializer refuses the old payload for missing `source_id`; current type also requires `view_root`. Baseline deserializer accepts current JSON by ignoring added fields. | A schema-1 value alone does not establish mixed-revision compatibility. Typed consumers and producers must coordinate source revisions; choose/document any versioning change before release. |

The [SDK contract](../../docs/SDK.md) already requires revision pinning for these
pre-1.0 source libraries. These are private workspace packages, not a claim of
stable published Rust APIs. The capabilities source/scope fields implement the
exact binding in [Web protocol](../../docs/WEB-PROTOCOL.md); missing bindings must
remain refused. Do not add guessed defaults to make old payloads parse, collapse
terminal states, or edit layout version numbers to bypass validation.

The live layout loader separately checks envelope/model version pairs, private
bounded input and exact root binding before migration. The model witnesses do
not claim execution of an older TUI against real user layouts. CLI schema v2,
native layout v4, capabilities schema 1, JSONL v1 and SQLite v33 have independent
scopes. The versioned C ABI header remains v1 and is unchanged from baseline;
this review does not claim a fresh C runtime test.

## Verification scope

- Pinned Nix shell, Rust 1.98.1; scoped immutable Git exports of both revisions.
  No reserved plan subtree or concurrent work enters the exports.
- Baseline Full/Minimal/Stateless and current Minimal/Stateless were built with
  `cargo build --offline --locked -p aw-cli --bin aw`, adding
  `--no-default-features --features minimal` or `--no-default-features` for the
  respective cuts. Current reduced builds use a separate target directory from
  baseline, avoiding cross-export timestamp/fingerprint reuse.
- Current Full uses the already qualified static musl asset from signed
  `b3d970b7dc769cbe0694382858c6546868a5d715`; Git confirms its Rust/Cargo/web
  compilation inputs equal reviewed `769e9ef`. SHA-256:
  `30d6f21218b07e16fd55d1f262d6eb0a132c3c1635b8526602a0b17819ee2b74`.
  Baseline Full and both revisions' reduced cuts execute with glibc.
- Each binary executes `cli schema`, `--help` and `--version` with private
  HOME/XDG roots and no real provider/account configuration. Compare parsed
  schema rows and root help, then execute the sixteen controls above.
- Independent private Cargo consumers link the actual baseline/current
  terminal, Workspace and surfaces crates. Compile the exhaustive match and
  struct literal; execute serialization, migration and cross-deserialization
  controls. Fixture locks retain the signed source's registry versions/checksums.
  Expected E0004/E0063 and rejected payloads are successful negative controls.
- Transient harness/results: `/tmp/aw-cli-review-1dbgoks4`; not required release
  artifacts. No production State, real grants, providers or user layouts used.

This focused review supplies no full Rust/Node rerun, native non-Linux execution,
browser mixed-version runtime test, live-provider/billing result, signing,
publication authorization or independent security certification. Remaining
gates stay open in [TODO](TODO.md).
