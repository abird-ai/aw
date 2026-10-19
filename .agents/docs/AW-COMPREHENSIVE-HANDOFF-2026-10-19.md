Author: GPT-6
Owner: @pvl
Prepared: 2026-10-19 (Singapore)
Observed source HEAD: `eb1e1a0eeb841eb53f5becdea49b565be7b35123`

# AW — comprehensive live takeover and historical handoff

> **Current-first, history-preserving handoff.** This is the entry point for
> a new coordinating agent. It consolidates what changed since the extensive
> [historical comprehensive handoff](HANDOFF.md), the live architecture and
> original phase decisions, actual validation, outstanding acceptance, and the
> next safe implementation boundary. It does **not** replace the authoritative
> [plan queue](../plans/TODO.md), [Workspace plan](../plans/workspace-views/TODO.md),
> [release plan](../plans/release-readiness/TODO.md) or Git/source.
>
> **Read both handoffs.** [HANDOFF.md](HANDOFF.md) is the retained 2026-10-17
> full-length historical design/phase synthesis (about 281 KB, twelve sections).
> Do not discard, overwrite or silently truncate it. Its earlier "current"
> statements are historical snapshots; this document covers the signed work
> of 2026-10-17 through 2026-10-19. The two together preserve the recorded
> original decisions while limiting duplication of obsolete evidence files.

## 0. Immediate status — read before acting

| Topic | Verified state at observed source HEAD |
| --- | --- |
| Project | AW, one Rust-native autonomous Agent/workspace implementation and one user-facing `aw` CLI |
| Checkout | `/home/pvl/spaces/abird/src/aw`, branch `main` |
| Source baseline | `eb1e1a0eeb841eb53f5becdea49b565be7b35123`; **revalidate**, another agent works concurrently |
| Version / State | `aw 0.15.0`; SQLite schema `v33`; JSONL `v1`; native saved layout `v4` |
| Active P1 | [release-readiness](../plans/release-readiness/TODO.md), **unfinished** |
| Active P2 | [workspace-views](../plans/workspace-views/TODO.md), **unfinished** |
| Closed plans | Historical [accepted-history](../plans/TODO.md); [handoff-process](../plans/handoff-process/TODO.md); visual [ux-convergence](../plans/ux-convergence/TODO.md) *within its scoped acceptance* |
| Icebox | [embedded-platforms](../plans/embedded-platforms/TODO.md), [restored-state-authority](../plans/restored-state-authority/TODO.md) |
| Live native docking | **Implemented:** a second Unix terminal controls the *same original* PTY while the original Workspace stays alive. Owner-controlled exclusive lease and post-drain detach receipt |
| New persistent PTYs | **Implemented:** explicitly launched broker-owned PTY survives launcher/ordinary Workspace frontend exit, same shell PID across reattachments |
| Existing live PTY promotion | **NOT implemented:** transferring already-running Workspace-owned shell to a detached broker, surviving the original owner's exit, is the next P2 frontier |
| Verified migration prerequisites | Linux exact original-master SCM_RIGHTS transfer, atomic master + pidfd escrow, reversible one-reader quiescence, bounded parser journal replay/checkpoint and one-shot guarded kernel staging |
| Irreversible migration barriers | Adopted child custodian, safe original `ChildGuard` disarm, exclusive source release/broker activation, crash/UNKNOWN semantics and real production two-owner tests |
| Authority | One original Agent, durable journal/Store, exact Actor/grant boundaries, no presentation-induced model/tool call or PTY replay |
| Reserved owner | **Never inspect** `.agents/plans/ux/**` or its staging; another agent owns it |
| Concurrent state | At the observed baseline, three tracked files under Workspace/Terminal docs were **modified by another agent**; preserve those bytes/staging |

**Explicit warning:** A passed kernel escrow or parser checkpoint is **not**
permission to activate a recipient. No `terminal-promote` command exists; do
not manufacture one before all release, child-custody and crash gates pass.

### Fresh takeover preflight

Read [`AGENTS.md`](../../AGENTS.md) before following any old instruction.
Use the available `@dotlink-a1` connector for this repo; verify it is the
requested machine before mutation.

```sh
cd /home/pvl/spaces/abird/src/aw
git rev-parse HEAD
git symbolic-ref HEAD
git status --short -- . ':(exclude).agents/plans/ux'
git diff --cached --name-only -- . ':(exclude).agents/plans/ux'
git worktree list --porcelain
git log -12 --format='%h %as %s' -- . ':(exclude).agents/plans/ux'
```

Never convert a recent snapshot into a promise that HEAD, plans, tests or
permissions are unchanged. Any modification to a file another agent owns is
a conflict to be coordinated, not a cue to reset or stash it.

## 1. Exact reading order for a new coordinating agent

1. [Root instructions](../../AGENTS.md) — signing, ownership, private State,
   disallowed operations, no new `Phase69`.
2. [Plan model](../plans/README.md), [active queue](../plans/TODO.md);
   [Workspace TODO](../plans/workspace-views/TODO.md),
   [Workspace README](../plans/workspace-views/README.md), and
   [live migration design](../plans/workspace-views/LIVE-PTY-MIGRATION.md).
3. **This handoff**, then [full historical handoff](HANDOFF.md), sections 1–12;
   the latter includes the initial purpose, original phase progression,
   P6/R0–R14 decisions, provenance of the old design docs and the source map.
4. Relevant implementation contracts: [Architecture](ARCHITECTURE.md),
   [Domain](DOMAIN-MODEL.md),
   [four Runtime primitives](RUNTIME-FOUR-PRIMITIVES-AND-SERVICES-2026-10-11.md),
   [Hook trust/policy](RUNTIME-HOOKS-TRUST-AND-POLICY-2026-10-11.md),
   [State](STATE.md), [CLI](CLI.md), [Workspace](WORKSPACE.md),
   [Terminals](TERMINALS.md), [Local owner](LOCAL-OWNER.md),
   [SDK](SDK.md), [Web protocol](WEB-PROTOCOL.md).
5. For migration specifically read code, not only design:
   `crates/aw-terminal/src/{lib,driver,checkpoint,filter}.rs`,
   `crates/aw-platform-unix/src/{fd_transfer,pid_identity}.rs`,
   `crates/aw-platform-unix/tests/live_pty_fd_handoff.rs`,
   `src/workspace/{native_dock.rs,native_dock/persistent.rs,terminal_control.rs,terminals.rs}`,
   `src/workspace/ui/{commands,interaction,render}.rs`,
   `src/main_full.rs` and `src/resources_cli.rs`.
6. Read [release-readiness](../plans/release-readiness/TODO.md),
   [restoration authority](../plans/restored-state-authority/TODO.md)
   and [embedded targets](../plans/embedded-platforms/TODO.md) before making
   claims about shipping, restore activation, firmware or other platforms.
7. Check any referenced historic link actually exists. The 2026-10-16
   history rewrite removed superseded dated plan/evidence/manifests; the
   historical handoff *describes* old records, but many of its older
   `../plans/PHASE…` links are no longer checked-out files. Preserve their
   explanatory chronology without inventing restored source files.

The full handoff is a reference, not a second mutable TODO ledger. The owner
and coordinator reconcile actual completion under the existing canonical plan.

## 2. Original product intent and user decisions

### One engine, composable presentations

- AW is one user-facing Rust program and one original Agent/TurnMachine,
  ToolRegistry/ToolPolicy and TurnJournal. Native fullscreen/inline TUI,
  browser, headless/RPC, SDK, managed local/remote views and embedded hosts
  reuse this engine; they are not separate competing Agent loops.
- The user model is **Domain → Workspace → Session**. Domain/policy identity,
  Actor, User, Machine, Fleet, Workspace origin, Session/branch, source,
  provider account, credential custodian, payer and inference/tool executor
  are deliberately **different dimensions**.
- Sessions have immutable ancestry and exact branch/writer revisions. A tab,
  pane, File root, Git checkout, selected Machine or terminal view is not
  authority to redirect an already-running Agent.
- Architecture should stay **simple, robust, intuitive, scalable,
  future-proof, composable and maintenance-light**. Fix correctness and
  regressions before speculative new abstractions; avoid duplicating
  owners or recreating State/Agent per pane.
- Historic interest in the `left-right` read-publication algorithm applies
  only when independent read-heavy workloads justify it with benchmarks,
  not as a blanket mutex replacement.

### Native/UX decisions retained

- Ratatui/crossterm native **Pi-like minimal Agent view** (no redundant top
  header, one or two factual bottom status lines), plus an expanded multi-Agent
  Workspace. Browser SolidJS surface follows the same authority and semantic
  roles. Keep readable quiet tabs, no double headers, minimal border noise,
  and a usable Files/Git/Session/Activity/Settings/Terminal hierarchy.
- One command palette across resources and actions: `>` actions, `@` files,
  `$` terminals, `#` sessions, `%` windows and `/` commands. Integrate new
  Agent/terminal actions into the current palette instead of creating a second.
- Muted blue for user prompts, restrained positive/success green, failure red,
  running/UNKNOWN neutral. **Factual labels outlive colors.** Light/Dark/System,
  narrow terminals, Unicode, keyboard focus, modal Escape and real PTY restore
  are acceptance targets, not visual mockups.
- Scroll, split/maximize, line selection, drafts, terminal resize, Git diff
  and OS process ownership have separate lifetimes. Moving view state must
  never silently execute a prompt, spawn a shell, write Git or promote a
  restored Session.
- The UX convergence plan is closed only for its **tested visual/interaction
  scope**; native PTY custody, cross-Actor browser delegation and remote
  identities are tracked elsewhere.

### Operational and authority decisions

- No silent provider/account fallback, secret disclosure, network listener,
  model call, tool grant or command retry. Provider credentials live with the
  configured executor or SecretStore; selecting another view does not transfer
  the payer, bearer or inference location.
- Model/tool **intent is durable before effect**; settle already-paid results
  even when branch attachment later fails. UNKNOWN or possibly charged
  execution remains UNKNOWN. Retries require their own original-owner
  attestations; never replay a possibly accepted command.
- Restored data is inert until exact fresh authority and independent
  cross-clone/human requirements hold; no manual marker removal, raw WAL copy,
  lock deletion, quarantine bypass or invented cost-zero result.
- One owner/service coordinates `owner.lock`; separately approved
  trusted-local direct clients obey their shared/exclusive and generation
  fences. Same-UID/Unix socket authentication alone is not a multi-user grant.
- Built-in core/no_std and alloc-based embedded components must not acquire
  std/Tokio/SQLite/UI dependencies by accident. Full/minimal/stateless
  feature boundaries are explicit.
- **Fixed baseline** unless owner separately authorizes revision:
  `aw 0.15.0`, SQLite `v33`, JSONL `v1`, native layout `v4`.
- Numbered phases stop at **68**. The user parked 64/65 Files indexing/LSP
  and E6 firmware; do not invent Phase69 or claim completed AgentC parity.
  Early AgentC/pi-herdsman were design references, not new runtime authority.

## 3. Original foundations through Phase 11

The earliest source/protocol design began in September 2026 and is preserved
in [the historical handoff](HANDOFF.md#4-foundation-design-and-the-first-distributed-product-sequence)
and the [original architecture](ARCHITECTURE.md). The current Git history was
rewritten as a progressive, buildable sequence; the historical *meaning* below
survives, not necessarily old phase hashes/individual removed file paths.

| Phase / foundation | Architectural decision / capability | Do not overclaim |
| --- | --- | --- |
| Original 0/1 + initial design | One Rust Agent, append-only Turns, typed prompts/tools, bounded context, journal/Host seams, a CLI first with optional UX | Initial authored design is not proof of later native/cross-platform execution |
| 2 | Early validation distinguishes static/reference/mock transport from actual Rust/Cargo/Nix execution | Old authoring-only checks remain authoring-only |
| 3 | Durable SQLite/JSONL journal, selected provider invocation context, account/credential separation, intent/Tool receipt discipline | Not a license to replay paid or ambiguous effects |
| 4 | Native Ratatui Workspace built around the **same** Agent; `aw-workspace` pure layout/reducer/editing crate, saved tab descriptors | Rendering does not invoke model or create a State owner |
| 5 | Native `aw-git` scoped review/index operations and `aw-terminal` real PTY/VT100 driver with bounded reactor | Human shell is explicitly unsandboxed and unrelated to model Tool grants |
| 6 | Opt-in browser Workspace and `aw-surfaces` using the existing paired owner | A browser transport does not become a second executor |
| 7 | Same-user native attach through private local socket; one process owns the original Agent/journal | Window close/reconnect does not resubmit an accepted Turn |
| 8 | Optional personal-device inference, explicit Bridge with private credential custody; native/remote view separation | Device metadata/login is not provider bearer transfer |
| 9 | Named/private team gateway, explicitly invited participants and isolated controllers/participant history | Team labels, cookie or member list do not grant arbitrary shared Tool/PTY access |
| 10 | Explicit ContextFlow source selection/summary/apply with exact source provenance and release/derived-content restrictions | Reading a source does not grant redisclosure, model execution or target append |
| 11 | Stream rejection, Source/grant recheck, summary evidence and extension-name bounds hardened | These fixes do not atomically undo an already-dispatched external effect |

Source contracts: [SDK](SDK.md), [Tools](TOOLS.md),
[Sources](SOURCES.md), [Local protocol](LOCAL-PROTOCOL.md),
[Git](GIT.md), [Terminals](TERMINALS.md),
[Context flow](CONTEXT-FLOW.md), [Team protocol](TEAM-PROTOCOL.md),
[Credential placement](CREDENTIAL-PLACEMENT.md).


## 4. Phases 12–35: distributed State and exact authority

The numbered histories here are a **design/provenance digest**; read the
[historical full chronology](HANDOFF.md#phase-chronology-durable-effects-networking-and-distributed-authority)
and current source before implementing anything across the boundaries.

| Phase(s) | Binding decision |
| --- | --- |
| 12 | Durable LLM operation/attempt identity, explicit retry claims, immutable original head and charged/UNKNOWN preservation |
| 13 | Stable Domain/Workspace/Machine/Fleet nouns, `local` default Workspace and Session execution bindings |
| 14 | Ed25519 AwNode identity, one-use signed peer pairing and exact certificate-pinned QUIC; no silent re-trust or rekey |
| 15–16 | Direct STUN/rendezvous/UDP hole punching with ordered relay fallback; routes carry no policy or replay permission |
| 17–18 | MachineLink capability intersection, expiring leases, exact scoped metadata/Files/Git/Session read services; generic shell not implied |
| 19 | Stable PeerWorkspace/RemoteSessionRef, durable source operation identity, uncertain remote creation never duplicated |
| 20–21 | Pinned Fleet membership, per-member immutable dispatch/evidence; bounded multi-room opaque relay, no authority in relay |
| 22–23 | Directional federation grants and epochs, semantic event inbox/cursor/snapshot; no raw SQL/WAL replication |
| 24–25 | Typed remote orchestration and materialization of **authorized evidence**, not arbitrary imported executable facts |
| 26–28 | Domain strict-majority/term/lease, authority failover, Workspace writer epochs/fencing; source designation not state transfer |
| 29 | Signed semantic Workspace handoff, exact immutable source facts/writer history; divergence explicit, no raw State import |
| 30 | Stable cross-host User identity and device continuity keys; author text is not login, revocation invalidates derived use |
| 31 | Native auth families and rotating encrypted SecretStore/versioned provider custody, exact executor/payer/endpoint binding |
| 32 | OIDC/OAuth code+PKCE/JWKS login/link, separate connector scopes versus provider-account spending |
| 33 | Materialized facts/context proposal/apply and disclosure review as distinct commands with inherited source restrictions |
| 34 | Original `InvocationStarted` effect lease, process/store generations, **commit paid result before branch CAS attachment** |
| 35 | Trusted-local direct shared-lock mode separated from exclusive service owner, exact current-schema/local filesystem only |

Important design refinements: [Domain](DOMAIN-MODEL.md),
[AwNet](AWNET.md), [Fleets](FLEETS.md), [device protocol](DEVICE-PROTOCOL.md),
[remote device inference](DEVICE-INFERENCE.md),
[Database](DATABASE-ARCHITECTURE.md),
[adaptation matrix](DATABASE-ADAPTATION-MATRIX.md),
[identity/credential placement](CREDENTIAL-PLACEMENT.md),
[State](STATE.md) and [local owner](LOCAL-OWNER.md).

Do not confuse distributed routing/operation *evidence* with actor execution
authority. Exact source/actor/writer/grant versions are needed at admission
and again at result attachment; unknown costs or remote outcomes cannot
be normalized into success.

## 5. Phases 36–68, R0–R14, P6: the modern product and recovery design

The historical [handoff ``5–6](HANDOFF.md#5-runtime-policy-context-and-recovery-the-governing-design)
records the full wave-by-wave progression and the former dated plan file
descriptions. Current modules implement their accepted source scope.

### Product shell, collaboration and native/browser surfaces

- **36 / CLI** — one typed Clap command tree with side-effect-free help,
  schema and noun→verb identity; avoid a second argv parser.
- **37 / onboarding** — cancelable account/model selection, `ProfileUpdate`
  for CLI/native/web Settings. Separate provider/model discovery from saving
  config; changing account/model clears unsafe carried context. Responses/OAuth
  setup never exposes private continuation to browser/Store.
- **38 / collaboration** — exact shared draft open/submit and participant
  grants; an invited peer does not gain another participant's arbitrary
  private prompt/Tool execution.
- **39–50 / source and workflow hardening** — the accepted modern product
  connects context, Source/Domain, team membership, Machine/Fleet routing,
  typed Git/file scopes, bounded APIs, exact ownership and status. Source
  code and archived [historical chronology](HANDOFF.md) outrank any older
  static completion counter.
- **51–63 / presentation, process, runtime and provider integration** —
  progressively unified existing Agent/runtime views, native/web UI, status
  and workspace services, with typed process/provider/tool paths. Refer to
  live docs and accepted Git rather than inventing a parallel phase plan.
- **64/65** deliberately skipped/parked **filesystem indexing and LSP**;
  the user explicitly asked not to add them in that phase sequence.
- **66–68** belong to the accepted numbered program; do not extend that
  series or claim extra independent platform/fleet certification from it.
- **R0–R14** — portable/no_std Agent (shared TurnMachine and
  Runtime/Actor/Message/Envelope), search and provider adapters
  (OpenAI/Anthropic/Gemini), MCP resources/tools, extension hooks,
  embedded `no_std + alloc` host, retry/compaction, project resources,
  JSONL/RPC transport, native validation and typed command schema.
  Historical parity references are research comparisons, not a second
  Agent kernel or guarantee of equivalence.

### P6 Runtime management and correctness hardening

- Four primitives **Runtime / Actor / Message / Envelope**;
  Hook verbs **Next / Defer**; exact grants live outside optional
  extension switches. Context filters and decision-model hooks may be
  scaffolded, but are not blanket permission to expose hidden prompts or
  execute tools.
- P6 original Host Services reuse one `TurnMachine` and durable
  `TurnJournal`, with one real ToolRegistry/ToolPolicy. Process services
  (`PipeProcess`/optional PTY), Actor mailbox, cancellation, bounded queue
  priority and durable encrypted Pending/Accepted/Unknown facts do not invent
  new executor rights.
- P6 8A–8C: typed extension settings/project prompt composition,
  encrypted SecretStore-owned settings, factual status/event rail, inert
  media and read-only approval projection. No unapproved microphone/camera,
  browser secret API or model-forged human consent.
- P6 9A–9C: writer/actor/input/control facts, encrypted queue and private
  Notify, authenticated local Actor RPC and explicitly leased remote Actor
  control. Input `Accepted` is *not* executed or settled; Queue `Claimed`
  with later unknown must not be automatically requeued.
- P6 9D: narrower read-only model-management and separately gated queue
  enqueue/update/delete, process stdin-close/terminate and
  future-Agent-admission pause. Original Tool intent, source and Actor
  provenance remain bound; generic model-origin stop/spawn/PTy rights
  **remain disabled**.
- P6 10B–10J: source/generation/canonical intent identity, writer epochs,
  exact provider/Tool receipt ordering and immutable final Turn/context
  coherence. Store-factual paid results and Tool outputs cannot be silently
  replaced with a UI projection.
- P6 10K–10V: SecretStore mutation fences, WAL-aware backups, encrypted
  custody inventory, owner-only bounded composite staging/publication,
  cold-restore **dual quarantine**, exact-effect review, device-consent
  prerequisites and nonexecuting dry-run. A copied key/signature alone
  cannot prove independent human authorization.
- P6 10W1–10W9: anchored original-effect provenance, coherent read-only
  SQLite snapshots, immutable review proposals, and unfulfilled requirements
  for a live independent human and cross-clone coordinator. **W9 is not a
  runnable restored-State activation.**

For complete accepted historical details and caveats, read
[HANDOFF.md ``5–6](HANDOFF.md#5-runtime-policy-context-and-recovery-the-governing-design),
[Runtime primitives](RUNTIME-FOUR-PRIMITIVES-AND-SERVICES-2026-10-11.md),
[Hook policy](RUNTIME-HOOKS-TRUST-AND-POLICY-2026-10-11.md),
[Retry](LLM-RETRY-RECOVERY.md),
[Context Flow](CONTEXT-FLOW-IMPLEMENTATION.md),
[Team identity](TEAM-IDENTITY.md),
[SDK](SDK.md), [Tools](TOOLS.md) and
[restored-State plan](../plans/restored-state-authority/TODO.md).

## 6. Current implementation architecture: who owns what?

```text
Human / operator
   CLI inline TUI ── native Workspace ── Solid paired browser
         │                   │                  │
         └─────────────── existing owner/Host ──┘
                                  │
                       original Agent + TurnMachine
                                  │
       Runtime / Actor / Message / Envelope / Hooks
                                  │
              original ToolPolicy / ToolRegistry
                                  │
                  original immutable TurnJournal
                                  │
    SQLite v33 / JSONL v1 / SecretStore / epoch & source fences

Human native PTY control (separate from model Tool grants):
  ordinary Workspace Pool owns real original child + VT100 reader
     ├─ local tab (current owner)
     └─ explicit second Unix TTY with one controller lease
        (original Workspace owner must stay alive)

  explicit "workspace persistent start" ⇒ new separate broker-owned PTY
     ├─ original broker owns *newly spawned* shell/master/reader
     └─ attach/reattach: one-use local private socket, F12 release receipt
        (survives launcher/ordinary frontend exit, not broker SIGKILL)

  proposed migration of *already-running* Workspace PTY:
    original owner Quiesced → paused reader → bounded parser checkpoint
         + kernel master/pidfd escrow → inert recipient
         [STOP: original child custody and release not yet transferred]
```

**One owner of each kind of thing:**
`aw-agent-core` owns portable Agent/turn transition rules.
`aw-store` owns durable Session/history/effect coordination.
`aw-platform-*` owns OS-specific primitive operations, not Agent policy.
`aw-terminal` owns PTY driver, input/resize, VT100 state and child lifecycle.
`src/workspace/terminals.rs` retains native PTY Pool independent of tabs.
`src/workspace/terminal_control.rs` fences controller generations.
`src/workspace/native_dock.rs` owns opt-in local viewer and invitation protocol.
`src/workspace/native_dock/persistent.rs` owns separate launch-time broker.
`src/workspace/ui/*` owns Ratatui gestures and drawing only.
`src/web.rs` and `src/web/*` supply authenticated browser *presentation*.
`web/src/*` implements SolidJS, with `web/dist/*` committed/embedded assets.

### Important resource identity / retry rules

1. A view/Tab ID is not a resource grant. Always pin root/source kind/epoch,
   Session and Store IDs, Actor generation, Process incarnation,
   original writer/grant and terminal identity.
2. Store work is not necessarily reversible after provider/tool dispatch.
   Exactly-once external execution cannot be inferred from network success
   or a request correlation digest; Unknown demands inspection.
3. A Linux PTY master file descriptor is a kernel resource, not custody of
   the original child; `ChildGuard` and parser must move by an explicit
   coordinated state transition. A duplicated master FD plus pidfd is still
   an **escrow**, not a released old reader or admitted new writer.
4. A browser handoff token is a correlation key, not a File root or
   executable capability; sibling profiles share one principal, not a
   different-user Actor.
5. No stored layout, imported snapshot, terminal invite, project prompt,
   extension, system-looking model text or restored artifact can install
   grants or start a new model/PTY execution owner.


## 7. Post-acceptance shipping history: October 17–19, 2026

This is the sequence **after** the historical [HANDOFF.md](HANDOFF.md)
source checkpoint `04bb9e0`, not a new numbered Phase69. These are
source history milestones with their *verified scope*, not a release claim.

| Signed commit | Changed behavior / decision | Evidence / remaining boundary |
| --- | --- | --- |
| `47ee8a0` | User-accepted historical validation checkpoint and rebuilt phase lineage | Does not close unchecked provider, restored-State, remote or release tests |
| `1c2ec6e`, `3851dbf`, `b33a418` | Native read-only Agent/Find, scoped Files and exact Git/Review containers | Store/source grants, replace-detection and real native tests; no Agent retarget |
| `a0ffa3e`, `57a6059`, `0159f0b` | Browser read-only Agent and scoped Files/Git with draft-consent and bounded diff editors | Existing paired owner only; no new Tool root |
| `c01cb60`, `a20a4bd` | Original historical full [HANDOFF.md](HANDOFF.md) and its process/checkpoint closure | Historic statements before later docking are not current completion |
| `e06ffa7` | Same-window File presentation identity and retained view reparenting | Original Session/composer preserved; no independent browser authority |
| `7de8658` | Original-owner fenced File handoff across browser windows | Exact immutable source read and postaccept owner release |
| `00559eb` | Immutable Git Review cross-window docking | Exact original Review scope; no Git write or new checkout authority |
| `41e66c4` | Bounded editor Find, selection, scroll and focus continuity | No source bytes or credentials in ticket |
| `e558919` | Fresh origin-initiated receiver challenge/ACK, short freshness, pagehide retirement | Short recent ACK cannot guarantee browser never crashes immediately after |
| `0efea351` | Explicit same-owner multi-profile pairing, distinct HttpOnly cookies, one-use secondary invites, manual correlation link | **Not different-user delegated accounts**; owner-wide logout/expiry |
| `2bd6749` | Native/browser semantic UX color, low-noise Agent, factual statuses, modal focus | 135 Ratatui frames, real Chrome, real PTY restore; platform-wide UX certification remains separate |
| `c98987a` | Same-machine Unix native second-terminal control of original retained PTY | Original Workspace must stay alive; F12 post-drain receipt |
| `bd87e5d` | Opt-in **new** dedicated Unix PTY broker, launcher-independent shell lifetime | New shells only; no migration of existing Workspace-owned master |
| `2feaa78` | Linux test of original live master FD escrow via `SCM_RIGHTS` | Kernel prerequisite, no product migration |
| `a1100a9` | Atomic transfer of live PTY master + exact child pidfd | One typed kernel message; no ChildGuard ownership transfer |
| `635e1ec` | Reversible single-reader quiescence on original live PTY | Original regains control on cancel/drop/timeout; no adopted driver |
| `207c552` | Bounded PTY parser replay/checkpoint and source-bound one-shot FD+pidfd staging under pause guard | Source retains child; receiver only inspects; **not terminal promotion** |
| `eb1e1a0` | Exact pidfd-directed Linux process signaling primitive for future adopter | Not approved child lifecycle, activation or process custody |

The corresponding documentation commits are in `git log`. The authoritative
recent design is [LIVE-PTY-MIGRATION.md](../plans/workspace-views/LIVE-PTY-MIGRATION.md)
plus [Workspace TODO](../plans/workspace-views/TODO.md). The first
2026-10-17 historical handoff predates the browser and native docking
milestones listed here; do not interpret its "pending docking" statements
as current status.

### Shipped native command flows (current scope)

In a running Workspace's Command Palette, `terminal-start` explicitly launches
an unsandboxed shell. `terminal-dock` prepares a one-use original-owner PTY
invitation and the CLI prints `aw workspace attach INVITE`. A second local
terminal can attach to the same original shell and detach with F12. The
original owner must remain alive.

For a *newly* launched persistent shell:

```sh
aw workspace persistent start /absolute/project /absolute/path/to/bash
aw workspace attach /absolute/private/aw-pty-<nonce>/invite
# Press F12 to detach, then explicitly stop when finished:
aw workspace persistent stop /absolute/private/aw-pty-<nonce>/invite
```

The path comes from the actual printed invitation, which follows `TMPDIR`
if configured; never invent a path. That broker uses a new Unix session,
private 0700/0600 endpoint, single active controller, short rotating one-use
tickets and a 24-hour **idle** lifetime between attachments. It never
inherits provider credentials or starts an Agent/State provider service.
Receiver crash returns control; broker loss fails closed. No implicit
re-creation after SIGKILL or reboot.

Do **not** describe `terminal-dock` (original-owner lifetime) as equivalent
to `persistent start` (new broker-owned lifetime). **Do not** introduce a
public `terminal-promote` until the next protocol is complete.


## 8. Current frontier: migrating an already-running PTY

**Current authoritative status:** Source primitives and tests through signed
`eb1e1a0` plus an in-progress concurrent documentation reconciliation;
**there is no production migration command or custody transfer**.

### Existing signed prerequisites

| Prerequisite | Implementation / proof |
| --- | --- |
| Linux same-UID FD transfer | `aw-platform-unix::fd_transfer::{send_one,receive_one}`; checked `SO_PEERCRED`, bounded 256-bit correlation, CLOEXEC, no ancillary leaks |
| Kernel master+pidfd escrow | `send_pair/receive_pair` uses exactly one `SCM_RIGHTS` message; wrong cardinality/proof/truncation denied and received FDs closed |
| Stable live child identity | `aw-platform-unix::pid_identity::{open,exited,signal}` with kernel pidfd rather than a reusable numeric PID; signaling does **not** itself grant custody |
| Reversible original-reader barrier | `aw-terminal::Terminal::quiesce`; one reader pauses after earlier queued input/replies settle; timeout/drop/resume; no second reader |
| Full bounded parser state | `crates/aw-terminal/src/checkpoint.rs`; event replay bounded to **8 MiB journal, 48 MiB wire, 32,768 events**, including successful resizes, active/inactive/alternate screen, scrollback, text attributes and partial sanitizer/UTF-8 state |
| Staged one-shot escrow | `Quiesced::stage` binds original driver, live master FD and pidfd *under* same pause generation; `Staged::send_once` does not retry unknown kernel sends |
| Real kernel fixture | `crates/aw-platform-unix/tests/live_pty_fd_handoff.rs` independently proves Bash PID stays identical while a receiver holds escrowed master; rejection keeps original controller functional |
| Real quiescence/staging fixture | Source receives and rejects a paused escrow, resumes original Bash, preserves parser state and continues new explicit shell input, never replaying the rejected offer |

**Critical distinction:** Parser replay must never resend historical device
status responses (DSR); later *new* DSR requests in an adopted parser require
normal first-time reply logic. The checkpoint alone does not authorize a
second active reader, child reaping or any historical shell command.

### Blocked irreversible protocol

Design and implement as explicit original-owner transactions:

```text
Original active:
  PTY child + master + sole reader + parser + ChildGuard
    │
    ├─ user explicitly requests promotion to an authenticated,
    │  separately launched owner; exact root/Actor/site/generation checks
    ▼
Prepared:
  receiving broker holds bounded invitation/identity but NO PTY reader
    │
    ├─ source denies new input/resize; drains prior commands/replies
    ▼
Quiesced:
  original reader ACK; owner retains child + rollback ability
    │
    ├─ checkpoint full parser; stage actual master + child pidfd
    ▼
FD Received:
  receiver holds inert FDs and validated checkpoint; does NOT read/write
    │
    ├─ broker identity/ready acknowledgement; source fences final release
    ▼
Original Released:
  irreversible child custody transition, not merely source kill
  disarm ChildGuard only after safe committed, one-reader switch
    │
    ├─ recipient verifies matching commit epoch and activates only once
    ▼
Broker Active:
  one reader/writer/process custodian + faithful parser continuation
  source can exit; same original PID; no recreation or duplicate effects
```

**This last half is NOT implemented.** Required design before code:
- Separate `adopted-child` lifecycle for transferred `pidfd` with exact
  stop, signal and reaping responsibility, avoiding a stale bare PID and
  preventing the old `ChildGuard` from killing the child after release.
- A *single-writer/single-reader* transfer barrier. Merely transmitting a
  kernel master duplicates a descriptor; it does not stop the original
  reader thread.
- Explicit source/broker release acknowledgements, monotonic epoch and
  one-use source-life ticket, with crash test matrix. Before release,
  failure **rolls back** to the still-live original; after a committed
  release, recipient is active **once** and source must never reactivate.
- Handle source crash/receiver crash at each step, lost ACK after one
  side committed, unknown command writes, timeout, duplicate messages,
  child-exit races, UID/root/Actor mismatch, forged/replayed or expired
  proof, replacement filesystem endpoints and wrong-pidfd admission.
- Deterministic error messages for `UNKNOWN` rather than inferred success,
  automatic restart, replayed shell input or a made-up billing result.

### Acceptance test before claiming "implemented"

A real Bash started by **legacy Workspace** (not `persistent start`)
must remain the same numeric PID and preserve both main/alternate screen,
partial escape state, scrollback, Unicode, attributes, cursor and input
modes after the original Workspace owner **actually exits**, and the
receiving broker must be the only reader/controller.

Exercise normal shell, long-output streams, `vim`/`less` alternate screen,
resizes, bracketed paste, Ctrl+C, input queue pressure, pending protocol
replies, crash/packet loss and exact postrelease termination. Verify no
new Agent, model, Tool, SQLite run/effect, Git write or provider dispatch.
Test every rollback/ambiguous boundary. No truncated/trivial snapshot or
"new shell with the same title" is acceptable proof.

### Right next steps for a successor

1. Revalidate live HEAD, three tracked concurrent documentation edits and
   the pending protocol design. Read `aw-terminal::driver`, `ChildGuard`,
   `Quiesced`/`Staged` and the existing private broker owner.
2. Design and test **adopted child custody** *in isolation* using the
   already verified kernel pidfd, with no public command and a synthetic
   process. Specify who can reap a nonchild vs original child, and what
   happens when original source crashes pre/postcommit.
3. Design a bounded, versioned broker `Prepare/FD Received/Release`
   transaction and single-reader activation fence using existing
   controller generations; evaluate ambiguity/rollback before coding.
4. Integrate full parser checkpoint and receipt without duplicating
   historical output/replies. Preserve source visibility and rollback
   through every reversible stage.
5. Only then add a deliberately invoked native promotion UX/CLI and
   comprehensive **real two-process end-to-end** acceptance. Record
   exact commits/tests in the existing Workspace plan; do not invent
   another numbered Phase or extra Agent engine.

## 9. Validation performed for this handoff

All commands below were executed on the observed source
`eb1e1a0eeb841eb53f5becdea49b565be7b35123`, with pre-existing
other-agent documentation modifications deliberately preserved.

| Gate | Actual result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo test --locked -p aw-platform-unix --test live_pty_fd_handoff` | **4 passed**, actual independent processes |
| `cargo test --locked -p aw-platform-unix --lib` | **7 passed** |
| `cargo clippy --locked -p aw-platform-unix --all-targets -- -D warnings` | PASS |
| `cargo test --locked -p aw-terminal --lib` | **20 passed** |
| `cargo clippy --locked -p aw-terminal --all-targets -- -D warnings` | PASS |
| `cargo test --workspace --locked --exclude aw-store -- --test-threads=4` | **1,355 passed, 4 ignored; 0 failed**, 103 targets |
| `cargo test --locked -p aw-store --lib -- --test-threads=4` | **201 passed, 0 failed** |
| Five `aw-store` integrations: `composite_design`, `context_flow`, `custody_inventory`, `resources`, `store` | **7 + 29 + 9 + 23 + 88 = 156 passed, 0 failed** |
| Twelve browser Node suites (web model/workspace/context/multiplayer/agent parity/rich Markdown/review/handoff/presentation/lifecycle/sibling browser/UX convergence) | **174 passed, 0 failed** |
| `cargo test --locked -p aw-cli --no-default-features --features web --lib` | **139 passed, 0 failed** |

**Interpretation:** This is **split-package test coverage**, not a
claim that a single uninterrupted `cargo test --workspace --locked`
was run. Independent native Windows/macOS host execution, production
billing, human/OIDC account operations and third-party security
certification were **not** executed in this handoff. Do not turn a
previous accepted source compilation into new external certifications.

### Reproducible Nix entry

```sh
cd /home/pvl/spaces/abird/src/aw
nix --extra-experimental-features 'nix-command flakes' develop --no-update-lock-file -c \
  cargo fmt --all -- --check

nix --extra-experimental-features 'nix-command flakes' develop --no-update-lock-file -c \
  cargo test --locked -p aw-platform-unix --test live_pty_fd_handoff

nix --extra-experimental-features 'nix-command flakes' develop --no-update-lock-file -c \
  cargo test --workspace --locked --exclude aw-store -- --test-threads=4

nix --extra-experimental-features 'nix-command flakes' develop --no-update-lock-file -c \
  cargo test --locked -p aw-store --lib -- --test-threads=4

nix --extra-experimental-features 'nix-command flakes' develop --no-update-lock-file -c \
  cargo test --locked -p aw-store \
    --test composite_design --test context_flow --test custody_inventory \
    --test resources --test store -- --test-threads=4
```

Follow [BUILD-STATUS.md](BUILD-STATUS.md), the actual package `Cargo.toml`
and [release readiness](../plans/release-readiness/TODO.md) for current
build/release commands. Never run an example that could invoke a real
provider/tool without inspecting configuration and explicit authority.

## 10. Open roadmap beyond PTY migration

| Plan/status | Meaning; required next proofs |
| --- | --- |
| P2 [workspace-views](../plans/workspace-views/TODO.md) | **Active**; live PTY owner migration still open; cross-user Actor admission, independently scoped grants, verified remote/Machine/Fleet view access, browser post-ACK crash/partitions and Windows/macOS host acceptance separate |
| P1 [release-readiness](../plans/release-readiness/TODO.md) | **Active**; 0.15.0 is **not** a release authorization; remaining platform asset execution, native signing/notarization, version decision (0.16.0 only a proposal), live provider/billing approval and independent security review |
| [restored-state-authority](../plans/restored-state-authority/TODO.md) | **Icebox/blocking design**; fresh independent human challenge, durable cross-clone coordinator and exact effect reconciliation; cold backup review is not activation |
| [embedded-platforms](../plans/embedded-platforms/TODO.md) | **Icebox**; optional firmware/RTOS/physical transport/linker/host acceptance beyond demonstrated no_std/alloc boundary |
| [UX convergence](../plans/ux-convergence/TODO.md) | **Closed for visual/interaction scope** only; 135 native gallery frames, semantic Light/Dark/System and 16/256/truecolor, Chrome 16 scenarios. Does not close PTY/Actor authority |
| [handoff-process](../plans/handoff-process/TODO.md) | **Closed** simplified plan/checklist flow and signed scoped handoff; historical evidence archives are not a required workflow |

### Non-goals to avoid accidental scope creep

- No Phase69, arbitrary Files indexing/LSP from parked 64/65,
  no firmware E6 implementation under a PTY task.
- No replaced second Agent, per-tab ToolRegistry, copied
  Store-owner lock, remote credential fallback or model-visible
  "terminate any process" grant.
- No magic restart of a killed broker; owner-independent shell lifetime
  from *launcher/frontend exit* is not *reboot/crash-tolerant durable
  process* persistence.
- No automatic duplicate input after timeout or partially acknowledged
  `SCM_RIGHTS` receipt, no promotion from copied token/file path.
- No overwrite/reset/commit of an unrelated agent's pending
  `.agents/plans/workspace-views` changes or reserved `ux` subtree.
- No platform release, push, tag or deployment without **fresh explicit
  authorization**.

## 11. Signed commits, shared Git index and concurrent ownership

Another agent may commit at any time. The new agent must:
- Never inspect or touch `.agents/plans/ux/**`, including its index entries.
  Git status/diff/checks should exclude it.
- Treat **all other modified files** as owned by others until author and
  scope are established. At preparation this included
  `.agents/docs/TERMINALS.md`,
  `.agents/plans/workspace-views/LIVE-PTY-MIGRATION.md` and
  `.agents/plans/workspace-views/TODO.md`. These pending edits are valuable
  current work, not clutter to discard.
- Create owned scratch at `.agents/runs/<plan>-<agent>/TODO.md` only
  when executing a real slice. Scratch is ephemeral and never an
  authoritative completion marker.
- Stage **exact owned paths** in a temporary `GIT_INDEX_FILE` built from
  the *current expected parent*, verify `git diff --cached --check` and
  exact path sets, sign with the project SSH key, verify, then CAS
  `git update-ref` and reconcile only owned index entries. Never alter
  someone else's staging or stash to obtain a clean status.
- If HEAD moves, **revalidate** before any commit. Preserve intervening
  signed source/design changes, compare overlap and rebase your *own*
  patch rather than rewinding the shared branch.
- No unsigned commit, force-push, tag, deployment, reset, directory-wide
  prune or destructive cleanup without the required separate approval.
  A commit is **not** permission to push.
- Do not commit real credentials, private State/DB, transcripts, generated
  bundles, synthetic fixture directories, binary output or logs.
- The detached other-worktree reference
  `/home/pvl/.local/share/kilo/worktree-pool/aw-9a708eb8bcca/rebel-explanation`
  appeared prunable in the observed Git metadata. It is unrelated; **do
  not prune** or rewrite it without its owner.

### Commit/review example (pseudocode, not a blind paste script)

```text
EXPECTED_PARENT = fresh git rev-parse HEAD
read owned files, check status excluding ux
create unique temporary GIT_INDEX_FILE
git read-tree EXPECTED_PARENT in that index
git add -- EXACT_OWNED_PATHS
git diff --cached --check; compare exact staged paths
git commit-tree -S TREE -p EXPECTED_PARENT -m MESSAGE
verify SSH signature using a temporary allowedSignersFile
git update-ref refs/heads/main NEW EXPECTED_PARENT
reconcile only exact owned paths into real index
recheck non-reserved worktree/index and signature
```

The "no read of reserved staging" rule is stricter than merely not
committing it. A conventional broad `git status` or `git diff --cached`
without path exclusions is inappropriate in this concurrent setup.


## 12. Source and specification map — canonical living set

The following is a **complete map of current top-level live design docs**
as observed on 2026-10-19. Read what the task touches; the historical
[HANDOFF.md](HANDOFF.md) also explains the older original-phase docs that
were pruned from today's working tree.

| Area | Authoritative live specification |
| --- | --- |
| Main product design | [ARCHITECTURE](ARCHITECTURE.md), [DOMAIN-MODEL](DOMAIN-MODEL.md) |
| Ownership, history, SQL, restore | [STATE](STATE.md), [DATABASE-ARCHITECTURE](DATABASE-ARCHITECTURE.md), [DATABASE-ADAPTATION-MATRIX](DATABASE-ADAPTATION-MATRIX.md), [DATABASE-DESIGN-REFINEMENT-V2.1](DATABASE-DESIGN-REFINEMENT-V2.1.md), [DATABASE-DESIGN-REFINEMENT-V2.2](DATABASE-DESIGN-REFINEMENT-V2.2.md) |
| Runtime/portable Agent | [RUNTIME-FOUR-PRIMITIVES-AND-SERVICES](RUNTIME-FOUR-PRIMITIVES-AND-SERVICES-2026-10-11.md), [RUNTIME-HOOKS-TRUST-AND-POLICY](RUNTIME-HOOKS-TRUST-AND-POLICY-2026-10-11.md), [PI-CONTEXT-PARITY](PI-CONTEXT-PARITY.md) |
| CLI/SDK and Status | [CLI](CLI.md), [SDK](SDK.md), [UI-EXTENSIONS](UI-EXTENSIONS.md), [BUILD-STATUS](BUILD-STATUS.md), [SOURCES](SOURCES.md) |
| Accounts and identity | [CREDENTIAL-PLACEMENT](CREDENTIAL-PLACEMENT.md), [TEAM-IDENTITY](TEAM-IDENTITY.md), [TEAM-PROTOCOL](TEAM-PROTOCOL.md) |
| Local owner and transport | [LOCAL-OWNER](LOCAL-OWNER.md), [LOCAL-PROTOCOL](LOCAL-PROTOCOL.md), [WEB-PROTOCOL](WEB-PROTOCOL.md) |
| Native/browser UX | [WORKSPACE](WORKSPACE.md), [BROWSER-WORKSPACE](BROWSER-WORKSPACE.md), [SURFACES](SURFACES.md), [TERMINALS](TERMINALS.md), [GIT](GIT.md), [GIT-WORKFLOW](GIT-WORKFLOW.md) |
| Devices and federation | [AWNET](AWNET.md), [FLEETS](FLEETS.md), [FLEET-ADAPTATION-MATRIX](FLEET-ADAPTATION-MATRIX.md), [DEVICE-PROTOCOL](DEVICE-PROTOCOL.md), [DEVICE-INFERENCE](DEVICE-INFERENCE.md) |
| Context/model/tool safety | [CONTEXT-FLOW](CONTEXT-FLOW.md), [CONTEXT-FLOW-IMPLEMENTATION](CONTEXT-FLOW-IMPLEMENTATION.md), [LLM-RETRY-RECOVERY](LLM-RETRY-RECOVERY.md), [TOOLS](TOOLS.md) |
| User archives/workflow | [ARCHIVE-WORKFLOW](ARCHIVE-WORKFLOW.md), [GIT-WORKFLOW](GIT-WORKFLOW.md), [original HANDOFF](HANDOFF.md) |
| Live per-plan status | [plan layout](../plans/README.md), [queue](../plans/TODO.md), [Workspace TODO](../plans/workspace-views/TODO.md), [Workspace design](../plans/workspace-views/README.md), [Live migration protocol](../plans/workspace-views/LIVE-PTY-MIGRATION.md), [release readiness](../plans/release-readiness/TODO.md), [restored State](../plans/restored-state-authority/TODO.md), [embedded targets](../plans/embedded-platforms/TODO.md), [UX convergence](../plans/ux-convergence/TODO.md), [handoff-process](../plans/handoff-process/TODO.md) |

### Rust and browser module map

```text
Cargo.toml / Cargo.lock / flake.nix / nix/ / .github/workflows/
crates/
  aw-agent-core       portable TurnMachine, journal traits, original Agent
  aw-store            SQLite, receipts, State/SecretStore, restore quarantine
  aw-workspace        pure layout and view descriptions
  aw-terminal         original child, PTY driver, quiescence, replay journal
  aw-platform-unix    pidfd, SO_PEERCRED, SCM_RIGHTS, process ownership
  aw-platform         cross-platform facade
  aw-git              exact Git/review scope
  aw-surfaces         typed human service/resource projections
  aw-client           private local IPC client
  aw-inference        personal-device inference boundary
  other crates        consult workspace Cargo.toml (actual dependency graph)
src/
  main_full.rs        typed full CLI dispatch
  resources_cli.rs    Workspace/persistent terminal commands
  workspace/          native TUI and retained original PTY Pool
  workspace/native_dock.rs
  workspace/native_dock/persistent.rs
  workspace/terminal_control.rs
  workspace/terminals.rs
  web.rs, web/        existing paired browser owner and scoped resource routes
  attach.rs          native current-owner client
  awnet/             MachineLink, leased distributed read/control
  context_flow.rs, sessions.rs, model_*.rs, subagents.rs
web/
  src/               SolidJS, CodeMirror, bounded docking/presentation
  dist/              committed frontend assets compiled into Rust
tests/
  native_persistent_pty.py               5 real process/TTY cases
  web_docking_*.test.mjs, web_ux_convergence.test.mjs
crates/aw-platform-unix/tests/live_pty_fd_handoff.rs
crates/aw-terminal/src/checkpoint.rs
.agents/
  docs/              live contracts + original and current handoffs
  plans/             canonical queue and scoped active/closed checklists
  runs/              ephemeral coordinator scratch (not authoritative)
```

Avoid treating directory names in this *map* as executable commands.
Use actual current source, features and CLI schema to resolve names.

## 13. Startup prompt preserved inside this handoff

Give the next agent the entire following instruction (also supplied in the
user-facing response). It makes the handoff itself discoverable so the
next agent can load the full historical context without past conversation.

> You are taking over the **AW Rust repository** at
> `/home/pvl/spaces/abird/src/aw` through the available `@dotlink-a1`
> connector. Start by reading `AGENTS.md`,
> `.agents/plans/README.md`, `.agents/plans/TODO.md` and
> `.agents/docs/AW-COMPREHENSIVE-HANDOFF-2026-10-19.md` **in full**.
> Then read `.agents/docs/HANDOFF.md` (the exhaustive historical context)
> and `.agents/plans/workspace-views/LIVE-PTY-MIGRATION.md` plus the
> current Workspace TODO/README, terminal, architecture, Runtime and
> State contracts. Revalidate live HEAD, signed history and non-reserved
> worktree/index before doing anything. Another agent owns
> `.agents/plans/ux/**`; never list/read/stage/write it or inspect its
> index. Preserve every other agent's pending edits.
>
> The **current next priority** is safe **promotion of an already-running
> Workspace-owned Linux PTY into the persistent broker without changing
> its original Bash PID**, but production migration is NOT implemented.
> Verified source prerequisites include reversible one-reader quiescence,
> bounded VT100 replay checkpoint, atomic live PTY-master/pidfd escrow
> and one-shot source staging; none transfers child custody or grants
> broker execution. The next implementation step is isolated adopted-child
> process ownership with correct original `ChildGuard` disarm, followed
> by an owner-fenced Prepare→Quiesce→FD Received→Original Released→Broker
> Active transaction and crash/UNKNOWN acceptance. Do not create a new
> shell, read the PTY concurrently, replay input or call `terminal-promote`
> implemented before the full real-process proof.
>
> Follow exact current source and `.agents/plans/workspace-views/TODO.md`.
> Work in small reviewed composable phases, write only owned paths,
> use synthetic private fixtures with Tools Off and no provider calls,
> run pinned Rust/Node/PTY regression gates, and update canonical docs
> only as coordinator. Preserve original Agent/Actor/Store/ToolPolicy
> boundaries, signed commits and version `0.15.0` / SQLite `v33` /
> JSONL `v1`; do not push, tag, deploy or do destructive Git operations
> without explicit user approval. Report precise done/open status and
> next risks. Do not invent Phase69 or treat old archived plan links
> as files that still exist.
>
> First report the live HEAD, exact open migration gates, any existing
> concurrent edits and the next safe subphase. Then proceed with tests
> and implementation without reopening already settled design choices.

## 14. Accuracy / maintenance policy for this handoff

This is a comprehensive **index and synthesis** of recorded decisions,
not a fabricated verbatim recovery of deleted chat messages. Original
historical rationale is preserved in [HANDOFF.md](HANDOFF.md);
current facts and pending work live in actual source, Git commits and
[the plan queue](../plans/TODO.md). Never manufacture missing older
phase files, external certification, live provider tests or a completed
source-to-broker child promotion. If another owner updates HEAD or the
migration plan after this document's observed baseline, consult the
newer committed source and signed checklist **before** making changes.

No version bump, SQLite/JSONL migration, release, push, deployment,
new credentials or model/tool effect was required to create this handoff.
