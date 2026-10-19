# TODO — Release readiness

Status: active
Provenance: 47ee8a03abecbb3aee84a49c0a590096d76ed1a7
Accepted checkpoint: 47ee8a03abecbb3aee84a49c0a590096d76ed1a7

This plan carries unfinished validation/distribution work, not release
permission. Source 0.15.0, SQLite v33 and JSONL v1 remain unchanged.

## Committed

### `47ee8a03abecbb3aee84a49c0a590096d76ed1a7` — user-accepted historical work

- [x] Accept previously verified source/runtime/feature-cut corrections and platform-scoped native checks at this checkpoint.
- [x] Accept earlier five-target build, local assembly/Linux fixture and macOS ad-hoc signature verification within their original scope; native host execution, identity signing and publication remain separate gates.
- [x] Accept historical Rust/Nix validation and build-capacity fixes. Superseded helper counts, manifest chains and pre-rewrite commit IDs are not current runnable gates.

### `aecebbc8` — current product CI workflow

- [x] Provide a pinned Nix development-shell product gate and a separate native `.#aw` build/version job; execution status is recorded only after a current run.

### `9fbb8a3456b717c810c901460817ba1cc2317d59` — completed vault initialization lock lifetime

- [x] Explicitly release only the vault initialization lock after complete key durability work and on error/unwind, even while a duplicate open-file description survives; actual maintenance denial remains intact.
- [x] Deterministic duplicate-descriptor controls failed before the fix; six SecretStore tests and the original focused SDK test passed after it. The original `1c2ec6e` SDK failure used the same Conflict, but its exact fork timing was not captured. The cumulative `2809fa5` full gate passed.

### `2809fa58902df4dd9a23fb5a9a8fbd5e57d43189` — local Linux product gate

- [x] Align this checklist and Build status with the single product gate, without reconstructing removed phase/smoke/evidence profiles.
- [x] Remove the new Workspace-only unused-binding warning without changing control flow.
- [x] Execute the pinned full Cargo/Node gate and native Linux package/version checks on this exact commit; this does not supply hosted workflow or other-host certification.

### `3b0dbb502005570cca30e2cd52cba60c052f3587` — reviewed local product checkpoint

- [x] Pass the full pinned product gate and native Linux package/version after the Find lifecycle corrections; source version, SQLite schema and JSONL version remain unchanged.
- [x] Independently recheck the Workspace-only feature boundary and actual native terminal behavior; retain all external release gates below.

### `3851dbffd808491916e07a3642422bd0f7b465b0` — scoped Files local product checkpoint

- [x] Pass the pinned product gate and native Linux package/version after scoped Files and Windows retained-directory corrections; source version, SQLite schema and JSONL version remain unchanged.
- [x] Independently check the Workspace-only feature boundary and Windows target/test compilation. Actual Windows execution remains open and separate from compilation.
- [x] Exercise the immutable native package in real terminal sessions with two folder roots, explicit restart/reconnect, unchanged draft/caret, zero model/tool effects and restored terminal settings.

### `b33a4183f6032a024d1b73409d2c9134ea0aab24` — scoped Git local product checkpoint

- [x] Pass the pinned product gate and native Linux package/version after exact Git contexts, v4 layout migration and repository/index/terminal admission corrections; product version, SQLite schema and JSONL version remain unchanged.
- [x] Check the Workspace-only feature boundary and Windows platform-test/Git-library target compilation; native Windows execution remains open.
- [x] Exercise two existing private worktrees in actual terminal sessions, including exact selected-index staging, stale refusal, restart/reconnect, unchanged Agent/Files/draft and zero model/tool effect rows.

### `e2281d642fcdc380b0a8b328c2d71d6daa132ea0` — safe release output and native package custody

- [x] Reproduce release-helper data loss in private filesystem fixtures: unrelated files and linked output were replaced; incomplete artifacts were published. Refuse unrelated/incomplete/linked destinations before building and before publication; publish a complete flat file set by Linux atomic directory exchange, preserving prior output on build/staging failure. Cleanup never recursively deletes a caller-selected directory.
- [x] Eight focused filesystem regressions pass in the pinned Nix shell, including concurrent output changes, continuous readers, quoted paths and output preservation. Add this real filesystem gate to CI; hosted execution remains unclaimed. Validate Bash syntax, pinned Nix formatting and exact helper-source inclusion.
- [x] Evaluate all 18 native/release package derivations, including five cross-target definitions, on this source. Evaluation is not compilation or target-host execution.
- [x] Build the native Linux Nix package from a scoped immutable export of this signed commit; no reserved subtree or concurrent documentation patch enters the export. `bin/aw --version` reports `aw 0.15.0`. Product Rust/Cargo/web inputs are unchanged by the release-helper fix.
- [x] Execute that exact optimized Nix binary in all 35 native scenarios: original-Workspace migration `parser`, `vim`, `less`, `source-crash` (4); adversarial migration protocol/process-loss cases (26); existing dedicated broker `reconnect`, `competition`, `workspace-exit`, `shell-exit`, `broker-loss` (5). These are local Linux synthetic fixtures, not portable musl release-asset or other-host qualification, provider/billing execution, identity signing or publication.

### `78db9ff8db43bcc30387ef90e2bd6b4c5e736f4f`, `b3d970b7dc769cbe0694382858c6546868a5d715` — musl ABI and actual Linux x86_64 asset

- [x] Reproduce the current portable build failure on `37fbf78`: nine errors from musl's `socklen_t` ancillary lengths versus glibc's `size_t`. Convert bounded outgoing sizes into their libc field type and normalize received lengths before existing validation/arithmetic; no proof, descriptor-cardinality, closure, custody or wire-format change.
- [x] Normalize the existing filesystem magic cast for libc-dependent signedness; the filesystem allowlist is unchanged. Strict all-target platform Clippy passes for glibc and musl, without lint suppression.
- [x] Execute platform tests (10 unit + 4 process) and terminal tests (55) under each libc; explicitly execute the different-UID namespace probe under each. Ordinary Cargo's intentional namespace-test skip is not counted as execution. Formatting and the new portable CI Bash block pass.
- [x] Build `dist-linux-x86_64` from a scoped immutable export of `b3d970b`; verify the real asset's SHA-256 sidecar, executable mode, Linux x86_64 ELF header, absence of ELF interpreter/shared-library dependencies, VERSION and `aw 0.15.0` execution.
- [x] Execute the real installer against that asset using a local `file://` origin and private prefix containing spaces/quotes: replace a fixture executable, preserve inode/mtime on idempotent install, reject a forged checksum and wrong requested VERSION without replacing the installed bytes. No real user installation or remote release request.
- [x] Execute the byte-identical installed portable asset in all 35 native cases: original-Workspace parser/`vim`/`less`/source-crash (4), adversarial migration protocol/process-loss (26), persistent broker lifecycle (5). Same shell PID/parser and exclusive I/O survive source exit within the existing Linux scope; source SIGKILL cannot restore its killed terminal.
- [x] Add the real portable asset build/checksum/version step to CI so native-only compilation cannot hide this target failure. Local execution is verified; hosted workflow execution is unclaimed. Other hosts, live providers/billing, signing/notarization and release authorization remain open.

### Source `769e9ef27f5edc88f24ae3fe6be1f8ddfe388596` — CLI and source compatibility review

- [x] Compare actual Full/Minimal/Stateless offline CLI schemas and root help with accepted `47ee8a0`: no changed/removed existing flag rows or removed command paths; seven additive Linux Full command paths. Six binaries and sixteen parser/metadata controls pass with private empty HOME/XDG/cwd and no generated State/config/root files.
- [x] Compile independent consumers against both source revisions and execute synthetic layout/capabilities readers: terminal exhaustive matches and Workspace literals require source updates; v1 layout migrates to v4 with an explicit scope, while the old reader refuses v4; capabilities remains schema 1 but current parsing requires source/scope fields absent from old output. Preserve those custody/binding checks. Record scope, commands and owner decisions in [CLI compatibility](CLI-COMPATIBILITY.md).

### Source `1502a0e6428e6994f83f2ecfd6fc03701c4a5f0c` — current five-target assembly

- [x] Realize the complete current `release-all` graph on Linux x86_64: Linux ARM64, macOS ARM64 and both Windows targets compile/link successfully; Linux x86_64 reuses the byte-identical previously qualified musl asset. Linux/macOS use Full and Windows uses Minimal. No product/source change was needed.
- [x] Execute the canonical release helper into a private output directory: exactly 15 flat regular files, five correct target/profile manifest entries, matching SHA-256/size/sidecars, VERSION 0.15.0 and byte-identical installers. Both Linux ELF files have the expected architecture with no interpreter or DT_NEEDED; both Windows PE32+ files have the expected machine and import none of `libunwind.dll`, `libwinpthread-1.dll`, `libgcc_s_seh-1.dll`, `libstdc++-6.dll`, `libmcfgthread-2.dll`; Mach-O ARM64 loads only system-path dylibs.
- [x] Verify all 8,937 embedded macOS SHA-256 code-page slots, 4,096 bytes per page and code limit equal to the signature offset; a deliberately changed byte is detected in page 4. This is plain ad-hoc integrity verification, not Developer ID, native macOS loading or notarization. General `rcodesign verify` returned 1 for absent cryptographic signature and warned about its own verification limitations; it is not counted as a pass. Independent CodeDirectory extraction plus direct hash comparison supplies the bounded integrity result.

### `47e5d19818a9ed2076eb1bc3473e42332341385d` — manual native-asset gate

- [x] Define a dispatch-only five-target workflow: separate pinned Nix `dist-*` builders, read-only permissions, scoped sparse checkouts/immutable exports excluding `.agents`, and same-run exact-revision artifact transfer to matching Linux/macOS/Windows runners. No push or dispatch has occurred.
- [x] Refuse wrong native OS/architecture, source revision, VERSION, checksum or image machine before launching a private copy of the checked bytes. Windows checks the OS machine independently of Python's WOW64 process architecture. Preserve the existing Full/Minimal boundary, memory-only State, disabled tools and isolated HOME/XDG/cwd; one synthetic loopback SSE request exercises Unicode input/output without retries or real provider credentials.
- [x] Execute the signed workflow's Linux build body from its explicit export, then run its committed probe on that exact musl output: passed and byte-identical to the qualified asset. Five real admission refusal controls pass; Minimal helper coverage on a source-equivalent Linux Minimal binary passes, including Workspace refusal with no second fixture request. Workflow/Python lint pass. This is local Linux proof plus prepared hosted jobs, not execution of the Windows/macOS/ARM runners or qualification of their UI/PTY/filesystem/installer/TLS behavior.

## Uncommitted

- [ ] Complete remaining release-asset native-host execution: Linux ARM64, macOS ARM64 and Windows x86_64/ARM64, locally or with hosted jobs when available. Current five-target compilation/assembly, Linux x86_64 fixtures and the prepared [manual native workflow](../../../.github/workflows/native-release.yml) are recorded above; hosted jobs remain unexecuted and cross-builds or emulation cannot certify missing hosts.
- [ ] Agree the version/release decision after the [completed compatibility review](CLI-COMPATIBILITY.md), including source SDK revision pinning, one-way native layout upgrade and mixed-version capabilities policy; obtain explicit release authorization before distribution. 0.16.0 remains a proposal; source stays 0.15.0.
- [ ] Decide/configure Developer ID/Authenticode signing and notarization requirements; ad-hoc signatures and checksums do not supply those identities.
- [ ] Run live-provider/billing compatibility checks only when separately authorized and configured.
- [ ] Obtain an independent security review; product tests do not supply independent certification.

## Tests and verification

- [x] `47e5d19`: execute the verbatim Linux builder `run` body extracted from its signed workflow, with `PACKAGE=dist-linux-x86_64` and `GITHUB_SHA=47e5d19818a9ed2076eb1bc3473e42332341385d` in a private job directory — passed. Explicit `git archive` export excludes `.agents`; real pinned `nix build path:EXPORT#dist-linux-x86_64 --no-link --print-out-paths --no-update-lock-file --max-jobs 1 --cores 2` compiles/links and creates the four-file CI input. New output SHA-256 remains `30d6f21218b07e16fd55d1f262d6eb0a132c3c1635b8526602a0b17819ee2b74`; no broader runtime rerun is inferred.
- [x] `47e5d19`: committed `python3 tests/native_release_asset.py PRIVATE_BUNDLE aw-linux-x86_64 --revision 47e5d19818a9ed2076eb1bc3473e42332341385d` — Python 3.13.15 native Linux execution passed: version/schema/help, parser refusal, Full boundary, exact Unicode answer, one synthetic request and no HOME/XDG writes. Wrong host/revision/VERSION/checksum and a forged image machine with an updated matching checksum each refuse before launch. Private diagnostics: `/tmp/aw-native-gate-kc4in80u`.
- [x] `47e5d19`: committed `exercise` helper against the existing source-equivalent Linux Minimal binary — passed with Python 3.13.15; Workspace mode exits 2 without another fixture request. This is reduced-path Linux coverage, not execution of either Windows asset. The same Full probe also passed with pinned Nix Python 3.14.7 during development.
- [x] `47e5d19`: pinned `actionlint 1.7.12 .github/workflows/native-release.yml` and `pyflakes 3.4.0 tests/native_release_asset.py` — passed; checkout/download/setup actions and runner labels checked against upstream definitions. Rust/Node product code is unchanged; no fresh full Rust/Node gate, hosted workflow, missing native host, live provider or signing result is claimed.

- [x] `1502a0e`: from a scoped immutable Git export, `bash scripts/build-release-artifacts.sh /tmp/aw-release-assembly-9t15pxlc/release` — passed, including real `nix build .#release-all --no-link --print-out-paths --no-update-lock-file`; aggregate `/nix/store/cmn9hmycpd63yxfjn9ymdgspgr7s4bab-aw-release-all`. Export excludes all `.agents` paths and concurrent patches. No remote publication, real user installation, version change or release authorization.
- [x] `python3 /tmp/aw-release-assembly-9t15pxlc/verify.py` — complete file set, manifest/checksums, executable modes, architecture/dependency checks and installer bytes passed; assembled Linux x86_64 `--version` reports `aw 0.15.0` with no private HOME/XDG/cwd files created. LLVM 21.1.8 inspects PE imports. No foreign asset was executed; the previous native Linux custody result applies to the byte-identical x86_64 asset, without a new 35-case runtime run or full Rust/Node rerun.
- [x] `python3 /tmp/aw-release-assembly-9t15pxlc/verify-macos-hashes.py` — pinned `rcodesign 0.29.0 extract -C /dev/null code-directory-raw ASSET`, [Apple CodeDirectory layout/flags](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/cs_blobs.h), direct Python `hashlib` comparison of all slots and corrupt-page negative control passed. Assembled Mach-O equals the inspected store asset. The tool's general CMS verification refusal above remains distinct from this ad-hoc hash result; identity signing/notarization/native-host gates remain unchecked.

- [x] `769e9ef` versus accepted `47ee8a0`: pinned offline/locked Full, Minimal and Stateless CLI builds and real `cli schema`/`--help`/`--version` execution — passed; all existing exported flag rows equal, reduced schemas equal, sixteen parser/metadata controls pass. Current Full reuses the verified `b3d970b` musl asset with Git-equal Rust/Cargo/web inputs; current reduced cuts use an isolated target directory. See [review](CLI-COMPATIBILITY.md) for command forms and feature/host limits.
- [x] `769e9ef` versus `47ee8a0`: actual external Rust consumer controls — baseline exhaustive terminal match/Workspace literal compile, current consumers require source updates (E0004/E0063); current model migrates synthetic layout v1 to v4, baseline refuses v4; old capabilities payload is refused by current required source/scope fields, current payload is accepted by old type. No real user data or authority defaults used. These focused checks do not replace the full product or external release gates.

- [x] `b3d970b`: pinned glibc/musl `cargo test --locked -p aw-platform-unix --all-targets` and `cargo test --locked -p aw-terminal --all-targets` (musl adds `--target x86_64-unknown-linux-musl` and static CRT) — 14 + 55 passed per libc. Explicit `unshare --user --map-auto --map-root-user -- cargo test ... --test cross_uid_fd_handoff -- --ignored --exact different_uid_is_rejected_before_descriptor_io` passed for each libc. Strict platform all-target Clippy for each, Cargo formatting and portable CI Bash syntax passed. The pinned cross-package development shell supplies the musl compiler/linker; this is actual local execution, not compile-only evidence.
- [x] `nix build path:/tmp/aw-musl-acceptance-4lcyexci/source-fixed#dist-linux-x86_64 --no-link --print-out-paths --no-update-lock-file` on signed `b3d970b` — passed. Real sidecar/ELF/version checks, four private-prefix installer checks and 35 `AW_TEST_BINARY=PRIVATE_INSTALLED_ASSET` native scenarios passed in the pinned Nix shell. No current all-target release assembly, other-host qualification, live-provider result or complete Rust/Node suite is inferred from this focused gate.

- [x] `e2281d6`: pinned `python3 tests/release_output.test.py` — 8 passed; `bash -n scripts/build-release-artifacts.sh`, pinned `nixfmt --check nix/source.nix`, and exact Nix helper-source inclusion passed. Pre-fix fixture control: 3 failures in the original six-case suite; post-fix eight-case suite passes.
- [x] `e2281d6`: `nix eval path:/tmp/aw-native-package-uzwr5iux/final-source#packages.x86_64-linux --apply 'p: builtins.mapAttrs (_: value: value.drvPath) p' --json --no-update-lock-file` — all 18 package derivations evaluated. `nix build path:/tmp/aw-native-package-uzwr5iux/final-source#aw --no-link --print-out-paths --no-update-lock-file` — native package built; version and the 35 actual optimized-binary native scenarios above passed with `AW_TEST_BINARY=/nix/store/yc6f12xrm9g432kzqkn87rv0jyhnkyd3-aw-0.15.0/bin/aw` in the pinned Nix shell. Scoped source export contains the signed tracked compile/build inputs, excluding `.agents` and all concurrent work. No full Rust/Node suite rerun is inferred from this focused script/package validation.

- [x] `bash scripts/bootstrap.sh` — pinned Nix shell on `b33a418`, passed.
- [x] `cargo fmt --all -- --check` — pinned Nix shell on `b33a418`, passed.
- [x] `cargo test --workspace --locked` — pinned Nix shell on `b33a418`, 1,638 passed, 2 existing ignored, no failures.
- [x] `node --test tests/web_model.test.mjs tests/web_workspace.test.mjs tests/web_context_flow.test.mjs tests/web_multiplayer.test.mjs tests/web_agent_parity.test.mjs tests/web_rich_markdown.test.mjs` — pinned Nix shell on `b33a418`, 107 passed.
- [x] `nix build 'git+file:///home/pvl/spaces/abird/src/aw?rev=b33a4183f6032a024d1b73409d2c9134ea0aab24#aw' --no-link --print-out-paths --no-update-lock-file` — passed; built native `bin/aw --version` exited 0 with `aw 0.15.0`.
- [ ] Release-asset native execution — exact assets and remaining hosts; unfinished external gate.
- [ ] Version/release approval — explicit owner decision; unfinished external gate.
- [ ] Identity signing/notarization — actual configured identities and execution; unfinished external gate.
- [ ] Live-provider/billing checks — separately authorized real execution; unfinished external gate.
- [ ] Independent security review — unfinished external gate.
