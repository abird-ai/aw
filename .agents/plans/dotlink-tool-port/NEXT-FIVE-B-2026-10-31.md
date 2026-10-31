# Shared toolset TS-N6..TS-N10 — 2026-10-31

Author: @pvl
Co-authors: ChatGPT GPT-6

Incremental continuation of NEXT-FIVE-2026-10-30.md. AW original
Agent/TurnJournal/ToolPolicy remains authoritative. dotlink is read-only.
Five **sequential** phases with tests for each:

- **TS-N6: exact-source durable task fencing.** Host-verified identity,
  journal-intent generation, output and cancellation revocation, stale
  generation / conflicting settlement errors. No model-minted task grants.
- **TS-N7: bounded durable spool core.** Provide host-opened handles rather
  than new paths, track exact retained bytes, usage quotas and output pages.
  Never publish completed output until writer sealing/sync; test partial
  output, overwrite refusal and cancellation.
- **TS-N8: provider-ready typed media envelope.** A complete-byte,
  digest-checked, signature-derived media payload; explicit host capability
  negotiation and no automatic provider attachment. Keep native ToolOutput
  backward-compatible until the provider-specific adapters are authorized.
- **TS-N9: executable sandbox plan, not an implicit sandbox grant.** Build
  fail-closed Linux bwrap argument specification using an explicit validated
  host profile, without changing AW trusted shell semantics; test rejection,
  no-network defaults and no ambient root bind.
- **TS-N10: cross-feature compatibility and correctness gates.** Exercise
  sync-default vs async/durable/sandbox, original AW agent/tool policy tests,
  Unix and Windows compilation, non-UX worktree preservation; reconcile
  exact implemented vs partially integrated in plan docs.

Acceptance distinguishes **library-complete** from **host-integrated**.
A provider-ready typed envelope, file spool or bwrap command plan is not a
new model tool, a background process, a persistent replay ledger, or a native
typed MCP result. Do not claim full AW workspace validation without running
its original pinned Cargo.lock successfully on l5.

## Executed phases and source

- [x] **TS-N6 — fenced durable task identity.**
  `aw-tools-core/task_fence.rs`: source kind/id/epoch, actor, site, session,
  branch, turn, call and effect, journal-intent sequence and CAS-style
  generations. Host-only `FencedProcessSpools` rechecks the binding before
  summary/page/ack, and rejects terminal publication while real stdout/stderr
  writers have not quiesced. Tests prove stale/revoked/wrong scope.
- [x] **TS-N7 — host-owned bounded disk output.**
  `aw-tools-core/task_spool.rs` takes already-opened distinct regular files,
  writes/reads bounded byte-exact prefixes, accounts 64 MiB per stream and
  512 MiB per session, preserves historical ACKs through retention compaction
  and drains excess bytes without disk tasks. `aw-tools/process.rs` supports
  explicit `spawn_with_host_spools` using the existing original Process
  supervisor, writer leases and ToolStarted requirements. No model-provided
  filename, new task DB or shell runner.
- [x] **TS-N8 — verified media handoff.**
  `aw-tools-core/media.rs` owns a complete bounded `MediaPacket`, verifies
  signature-derived MIME/SHA-256 at dispatch, and requires a host-declared
  `TypedMediaHost` policy before attachment. AW bread uses the packet, but
  continues to emit encoded JSON, not provider-native typed ContentBlocks.
- [x] **TS-N9 — pinned Linux Bubblewrap launcher primitive.**
  `aw-tools-core/sandbox_linux.rs` checks exact host policy, prohibits
  unapproved root/runtime mounts, uses openat2 no-symlink pinned root,
  runtime and executable FDs, Bwrap FD mount flags, no-network default
  and a kill-on-drop child owner. No new AW tool grant or fallback. One
  native smoke attempt on l5 spawned bwrap but failed mounting /proc with
  Operation not permitted; runtime isolation not accepted as verified.
- [x] **TS-N10 — original pinned correctness closure.**
  Fixed Cargo.lock edges without changing original dependency versions
  (pruned unused mime_guess/unicase). Added spool fast-path after quota
  exhaustion and exact terminal-state quiescence checks; preserved one
  ToolRegistry/ToolPolicy/TurnJournal and original no_std ABI.

## Verification — real AW Cargo.lock (2026-10-31)

| Gate | Result |
| --- | --- |
| `cargo test -p aw-tools-core --all-features --lib` | **51 passed** |
| `cargo test -p aw-tools-core --lib` | **28 passed** |
| `cargo test -p aw-tools --lib` | **72 passed** |
| `cargo test -p aw-platform-unix --lib` | **15 passed** |
| `cargo test -p aw-agent-core --lib` | **70 passed, 1 ignored** |
| `cargo test --bin aw` | **383 passed, 1 ignored** |
| `cargo test -p aw-cli --lib` | **226 passed, 2 ignored** |
| `cargo check --workspace --all-targets --offline --locked` | **Passed** |
| Strict Clippy `-D warnings`, modified core/tools/platform | **Passed** |
| Windows GNU target (core all features, aw-tools/platform all targets) | **Passed compilation**, not runtime |
| `aw-agent-kernel` no_std+alloc | **Passed** |
| Cargo format, diff check and locked metadata | **Passed** |
| One monolithic full-workspace **test** invocation | **No terminal result: not counted** |
| Bubblewrap native mount/namespace test | **Blocked by host: EPERM mounting /proc** |

All locked commands ran on l5 against AW's original dependency versions after
Cargo.lock reconciliation. An ignored dependency-adjusted integration fixture
was also used earlier, but is not the final verification authority.

## Remaining gates (not completed by these phases)

- [ ] Durable model-facing `job_start/list/get/output/ack/cancel` attached to
  AW's real Session/Turn/Process persistence and source epochs, including
  restart recovery and request-key replay fencing. The library's
  TaskFence/BoundedFileSpool/FencedProcessSpools are already available.
- [ ] Provider-specific native typed image/audio/blob ContentBlock serialization
  and policy checks. Do not advertise typed media via current text ToolOutput.
- [ ] Host approval and native runtime tests before registering an actual
  sandboxed Process tool. AW's trusted shell is **still unsandboxed**.
- [ ] Real Windows NTFS ACL/job-object/FS races and macOS xattr/platform tests.
- [ ] A complete terminal result from `cargo test --workspace` with the
  required environment; earlier invocation did not return results.
- [ ] Dotlink as a live consumer of the shared crate (no dotlink modification
  is authorized in this phase).

Owner-only changes remain **uncommitted** for AW coordinator reconciliation;
no push, tag or release and no writes to dotlink or reserved UX work.

## Late dotlink drift review — 2026-10-31, l5 read-only

Dotlink advanced during this AW run beyond the earlier 70939fb baseline.
All three new commits were inspected separately:

| Commit | Change | AW disposition |
| --- | --- | --- |
| c44194f | v0.14.2: spool-writer quiescence, retention, panic/failure settlement and launcher hardening | Reused sealed RAII leases, retention accounting and host journal fences. No duplicate replay/JobManager |
| 762dfa8 | Native platform CI: portable timestamps/fixtures, spool compaction regular-file checks | AW requires regular host spool files, pinned platform metadata and Windows GNU cross-target compile |
| abfc71d | Windows lexical-path admission before resolution and invariant UTF-16 mutex naming | Adapted AW Windows scoped filesystem admission-then-path kernel mutexes, same casing and digest convention; Windows cross-target compilation passed |

Windows cross-process lock interoperability and native runtime behavior remain
unverified. Dotlink OAuth, HTTP/OpenAI tunnel replay, daemon progress and
release transport internals were not copied into AW. No dotlink files changed.
