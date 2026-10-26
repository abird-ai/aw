# AW native filesystem tools — dotlink parity under AW authority

Author: Prasanna Loganathar

Co-authors: ChatGPT GPT-6

Owner: @pvl

## Scope

Port dotlink's 2026-10-25 tool surface and available 2026-10-26 hardening
implementation, not its MCP server or machine access/policy model.
Source snapshots are **AW** `53206b2` and **dotlink** `fc53fc9` plus its
uncommitted tool-hardening working tree. Source/tests outrank historical plans.

### Architecture

- **Keep AW**: its one original `Agent`, `ToolRegistry`, `ToolPolicy`,
  TurnMachine, durable intent-before-effect journal, source epochs and
  explicit model-directed read/Bash waves.
- **Native tools**: `aw-tools` modules `search`, `text`, `edit`, `patch`,
  `binary`. No additional agent, executor, tool policy or model-context
  authorizer. `find` model tool retired; `ls` offers recursive glob discovery.
- **Lowest-layer primitives**: `aw-platform-{unix,win}::scoped_fs` owns
  no-follow IO, bounded line/byte windows, atomic create/update/replace,
  snapshot verification, permission/DACL protection and mutation coordination.
- **Authorization**: `--tools-mode` is the outer ceiling;
  `--tools-allow` narrows it. Exact granted actor/source/site/effect remains
  AW's only tool-dispatch authority. Child agent tool subsets stay unchanged.
- **Scheduling**: model `parallel:true` explicitly permits adjacent
  independent Read calls, not writes. File reads do not take the write mutex.
  Cooperating Unix writers take advisory directory `flock` across processes
  and a root-local mutex. External shells/editors are outside this contract.

## Decisions

- AW's 64 KiB tool arguments and 128 KiB result bounds outrank dotlink's
  larger profile-configurable transfer limits; the native file tool defaults
  are correspondingly bounded.
- New `write` and `bwrite` explicitly support atomic create/replace,
  like dotlink, versus AW's older create-only model. The retained
  `Root::create` API stays create-only but shares the staged write path.
- Multi-file edit/patch: preflight and stage all files before publication;
  every *file* is replaced atomically, but consecutive commits cannot be
  rolled back as a single transaction. Partial commit errors never invite
  replay and must be inspected.
- AW's strict no-symlink/reparse and hardlink refusal remains unchanged.
  New Unix files start at 0600, preserving AW's stronger existing default,
  rather than dotlink's ordinary 0666-minus-umask create mode.
- Binary `bread` supports base64/hex windows. Typed MCP image/audio/blob
  remains deferred because the AW agent's public ToolOutput is text-only.
  Do not weaken journal/policy to add media.
- Windows uses the same bounded operations with conservative local mutation
  locking. Its staged replacement copies a target DACL onto an **empty**
  temporary and uses `ReplaceFileW` plus backup recovery. Native Windows
  builds, filesystem tests, ACL/integrity and backup-recovery tests are still
  required before claiming Windows parity.
- Native `bash` stays AW's existing explicit **trusted unsandboxed** process;
  importing Bubblewrap would be a separate capability/authority decision,
  not a side effect of the filesystem tool port.
- No implicit migration/aliases for the retired model `find` tool or old
  `read.start_line`/single-op `edit` argument shape.

## Reference sources

- Sibling repo `/home/pvl/spaces/abird/src/dotlink/`:
  `src/mcp/tools/{ls,grep,text,edit,patch,binary}.rs`,
  `src/secure_fs.rs`, `src/staging.rs`, `src/mutation.rs`
- Dotlink plan: `.agents/plans/dotlink-tool-surface-2026-10-25/`
- Dotlink hardening: `.agents/plans/dotlink-tool-review-2026-10-26/`
- AW: `crates/aw-tools`, `crates/aw-platform-{unix,win}`,
  `crates/aw-agent-core/src/tool.rs`, `crates/aw-agent-kernel/src/turn.rs`,
  `.agents/docs/TOOLS.md`

## Verification scope

AW native tool unit tests cover all nine advertised filesystem tools,
line/exact-byte reads, multi-file edits, patch best-effort and strict failures,
binary range edits, stale snapshots, symlink refusals, private staging, and
parallel directory enumerators. Six independent AW test processes append
under the same workspace lock without losing acknowledged writes.
Windows-target compilation and execution cannot run on this Linux Nix shell
(the Rust target is not installed). Tests and gate results are recorded in
`TODO.md`.
