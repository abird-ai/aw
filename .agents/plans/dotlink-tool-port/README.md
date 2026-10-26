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

## Hardening parity follow-up — 2026-10-26

Source baseline: dotlink `9c771d2` plus the live, uncommitted
`src/mcp/tools/{grep,mod,shell,text}.rs` review additions inspected
2026-10-26. AW baseline `b1264c9`. Dotlink may continue changing; this is
a pinned comparison, not an automatic shared-code dependency.

The audit found omissions in the previous port. This pass restores:

- `ls` globally ranked top-N selection (path/mtime), candidate admission
  before limiting, opt-in gitignore/hidden filtering and explicit incomplete
  scans; no standalone `find`.
- `grep` cumulative content/file/candidate/directory work ceilings, bounded
  heap candidate selection, control-byte/UTF-8 exclusion, exact-file glob
  filtering, parsed ignore rule and aggregate ignore-work ceilings, explicit
  pagination and partial-scan metadata.
- `read` bounded, cancellable offset scans with binary-control rejection
  even in skipped lines; no whole-file preload.
- `write`/`bwrite` optional create-only and SHA-256-conditional updates,
  no-op receipts, file staging with cleanup of temporary and empty created
  parents. Legacy file operations reuse the same platform backend.
- `edit`/`patch` text validation, mutation cancellation, staged batch
  rollback, exact matching and no-op behavior; `bedit` corrected per-range
  bytes-written receipts.
- `bread` full `format=mcp|base64|hex` argument set and full-file requirement
  for `mcp`; typed MCP media remains an explicitly unfulfilled transport
  distinction from AW's encoded JSON representation.
- `bash`/`powershell` native tool arguments including `dir`, `stdin`,
  `timeout_secs`, `max_output_bytes` under host ceilings, and explicit
  model-directed `parallel` for trusted Process calls.
- Linux `openat2` mandatory `RESOLVE_NO_XDEV`, `NO_SYMLINKS`,
  `BENEATH`; Unix ancestor SH / closest-parent EX advisory locking in sorted
  path order; Windows named cross-process mutex keys compatible with dotlink.
  Both platforms use cancellation-aware waits and staged file publication.

The full native tool/parsing layer is compatible with dotlink's argument
schema plus AW's `parallel` scheduling hint and optional conditional-write
arguments. **The native binary media transport and sandboxed shell are not
identical** and must never be claimed as such: the former requires a model-tool
media contract and provider adapters; the latter must be a separate
host-authorized sandbox mode, not a silent rewrite of AW `trusted`.

The shared `aw-agent-kernel` no_std state machine, exact ToolPolicy grants and
journal-before-effect receipt semantics remain intact.

## Latest dotlink hardening crosswalk — 2026-10-26

Reviewed dotlink `fc53fc9..9c771d2` (all intervening commits) and current
uncommitted `src/mcp/tools/{mod,ls,grep,text,shell,patch}.rs` review work.
Applicable changes were integrated into AW's existing layers, not copied as
a second executor or tool-policy system.

| Dotlink source | Applicable AW implementation |
| --- | --- |
| `716ad06` + `5f4fce7` | One scoped staged filesystem backend; multi-edit / patch receipts; independent-process writer tests |
| `05619e3` | Linux pinned safe access, macOS per-component no-follow/device checks, Windows pinned DACL-preserving IO and named cross-process mutation locks |
| `5ff9e1c` | Bounded, globally ranked `ls` top-N and lexically sorted `grep` candidates, independent of directory enumeration order |
| `e597f1b` | Whole-line read truncation and exact CRLF windows; bounded skipped-prefix scans. Dotlink stdio transport diagnostics are not a native AW tool |
| `1a32c8f` | Bounded traversal and explicit partial-scan indicators |
| `734b6e3` | Shared binary-control text detection; bounded cumulative `grep` file/byte work |
| Dotlink uncommitted `mod/ls/grep` | Aggregate `.gitignore` byte/file budgets, distinguish denied ignore rules from absent ones, classify unreadable subtrees/files as incomplete, machine-readable reasons and text markers |
| Dotlink uncommitted `text` | 32 MiB cancellable offset scan, including binary-control inspection of skipped lines |
| Dotlink uncommitted `patch` | Strict staging preflight, no-op receipts, per-file output admission; Windows-gated tests reviewed |
| `71a1702`, `b46f8c1` | AW keeps its existing host-owned Process lifecycle; Windows managed PowerShell now starts suspended, attaches to an owned Job Object, and resumes only after containment. No detached pipe reader survives cancellation |
| `724ec23` | Dotlink-specific Bubblewrap protected-path masking. AW's `trusted` shell is deliberately unsandboxed; no phantom sandbox-protection claims or copied sandbox policy |
| Dotlink uncommitted `shell` | MCP heartbeat delivery is bounded by its transport/deadline. AW uses its own journal/event publisher and Process supervisor, not a remote MCP progress channel. No second polling or notification worker was introduced |
| `3ba8142`, `5fd56a8` | Tunnel progress / poll framing is dotlink transport logic; AW native tools do not own those connections |
| `cd96358`, `d6652d6`, `afa28ea`, `a568829`, `e81dca8`, `36ff4d8` and related commits | Dotlink identity, OAuth, profile generation, and credential-selection fixes; not native filesystem/shell operations. AW's source epochs, provider credentials and exact tool authorization remain unchanged |

Additional AW-specific hardening: `write`/`bwrite` support explicit
`create_only` and `expected_sha256` without giving the model any extra
authority; no-op writes preserve inode/mtime. The Agent's existing finite
per-tool result projection preserves parseable JSON truncation envelopes.
These are deliberate improvements beyond dotlink's input schemas.

The completed native source checks cannot substitute for required native
Windows/macOS runtime verification. AW `bread(format=mcp)` is encoded
JSON, **not** typed MCP media. Sandboxed shell and typed provider-native
tool results remain explicitly separate design gates. Neither was declared
complete by the filesystem hardening commit.
