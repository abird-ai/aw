# TODO — Read-only Agent views and composable Files/Git

Status: active
Provenance: 5a8e90ae5000fdf5edeaea3fb90be21fec086de6
Accepted checkpoint: 47ee8a03abecbb3aee84a49c0a590096d76ed1a7

Design and binding decisions: [README.md](README.md).

## Committed

### `47ee8a03abecbb3aee84a49c0a590096d76ed1a7` — accepted design checkpoint

- [x] Choose native read-only pinned Agent views before Files/Git containers, browser counterparts and docking.
- [x] Reuse stable tab IDs, bounded layout, the transcript renderer and existing Agent/SurfaceHost owners.

### `1c2ec6eff437f506fd2ea404ed6dcc3cf66c22a3` — native read-only Agent views

- [x] Extract strict exact-ID `SessionStore::read_branch_view` without minting a SessionHandle; activation retains ID-or-name compatibility.
- [x] Publish a private authorized Host reader without creating an Agent/Session or entering the execution queue.
- [x] Reuse one Host-owned SQLite Store for unsaved browsing, activation and first prompt; retain exclusive owner.lock and avoid per-view recovery or absent-State/JSONL fallback.
- [x] Carry exact Store, Session and branch identities through owner/navigation projections; titles remain labels.
- [x] Add bounded AgentView descriptors and explicit consistent v1→v2 migration; preserve IDs, pins, geometry, root binding, atomic/no-follow and 64 KiB protections.
- [x] Add explicit Open read-only Agent view and Sessions `o`; retain Enter activation, one current Agent and composer-free pinned views.
- [x] Keep independent per-view scroll/follow/find/selection, bounded recent render state and factual display limits; immutable history/export stays authoritative.
- [x] Fence view reads by exact resource, tab and monotonic request lifetime; bound pending work and expire from request start.
- [x] Revalidate authority independently of head changes and immediately hide denied/revoked/removed-source content and derived state.
- [x] Update live Workspace/Build documentation and retain Files/Git/browser/docking/left-right work as later slices.
- [x] Exercise two exact targets in an actual native PTY on this commit: independent scroll/find, Sessions `o`, Unicode/resize, unchanged unsaved Agent/draft/caret and zero execution rows; quit exited 0 and restored stty. This is separate from TestBackend coverage.

### `2809fa58902df4dd9a23fb5a9a8fbd5e57d43189` — asynchronous Find completion

- [x] Show Searching until a validated Find completion; report hit/miss/failure truthfully and reject stale/duplicate/inactive footer updates.
- [x] Pass the pinned full product gate, native Nix package/version and focused real-terminal Find acceptance on this commit; follow-up refresh/restore corrections are recorded separately below.

### `3b0dbb502005570cca30e2cd52cba60c052f3587` — Find intent and request lifetimes

- [x] Preserve committed Find intent across changed Agent content, pending/missing results and inactive-pane refresh; n/N and flash consume selection mode without later key capture.
- [x] Allocate checked UI-lifetime Find request IDs so close/restore of a tab cannot accept another lifetime's actual reply; keep cancellation/source/generation admission intact.
- [x] Two actual-executor regressions failed before the fix and passed afterward; nearest native suite passed 153 tests with one existing ignored benchmark. Final exact-commit gate, feature boundary, native package/version and real PTY acceptance passed below.

### `6b96f010995ffa49414486ae63234a90ec9102e6` — retained Windows directory bindings

- [x] Pin canonical root, ancestor and per-operation descendant directories with opened metadata handles that deny delete sharing; preserve leaf no-follow/hardlink, bounded read, cancellation and edit-CAS rules.
- [x] Compile four real-filesystem Windows regression tests on cumulative Files candidate `3851dbf`. Native Windows execution remains unchecked; Windows ancestor/directory rename or deletion is blocked while the corresponding handles live.

### `3851dbffd808491916e07a3642422bd0f7b465b0` — native scoped Files containers

- [x] Compose Files navigators and document groups in flat, stable-ID ownership under the existing pane/tab limits; retain invoking-container preview policy and Explorer focus, including split-group routing.
- [x] Bind File identity and all derived caches to exact scope/path; retain explicitly opened root capabilities and human launch roots across Agent presentation changes. Saved IDs and labels grant no access.
- [x] Add explicit folder open/reconnect through the existing palette and bounded executor; reconnect moved File tabs by their own scope, prune unused capabilities and preserve bounded display-only folder labels.
- [x] Migrate consistent native v1/v2 layouts to v3 without adding panes or changing geometry, focus, legacy IDs or exhausted allocators; reject malformed/mixed versions and preserve root binding, atomic/no-follow and 64 KiB protections.
- [x] Fence read/list/search/highlight/find work by capability binding, exact resource/owner and checked lifetime across close/rebind/restore. Preserve ordinary moved-view state; clear revoked content and reissue pending refresh/palette queries correctly.
- [x] Admit review feedback against the lossless canonical current Agent root independently of human Files roots; retain owner serialization and exact review/index admission.
- [x] Cover model/persistence, actual retained-root backend and real native Browser/executor/TestBackend regressions, including held stale replies and split-container routing.
- [x] Pass the pinned product gate, Workspace feature boundary, Windows test compilation and native Linux package/version on this exact commit; actual two-root terminal and restart/reconnect acceptance passed with zero effects and restored stty. Windows runtime remains open.

### `491f3665b4de7f9fce3cfb4b4a14066d80e205d2` — retained repository identities

- [x] Retain physical directory identities for checkout root, worktree Git directory and common directory; validate exact Git mappings before operations and fresh index application without claiming exclusion of concurrent external writers.
- [x] Exercise deterministic checkout/admin-directory/gitfile replacement with real Git; add Windows directory-identity tests with target compilation kept separate from actual Windows execution.

### `b33a4183f6032a024d1b73409d2c9134ea0aab24` — native scoped Git containers

- [x] Extend flat ownership with independent Files scope and per-navigator exact Git context bindings under existing limits; preserve dormant bindings across navigator-kind switches without retargeting another view.
- [x] Migrate consistent native v1/v2/v3 to v4 without changing geometry or reusing IDs; preserve old Files wire fields, root/atomic/no-follow/64 KiB protections, and refuse genuinely unavailable container allocation atomically.
- [x] Add explicit Git open/reconnect through the existing palette and executor; retain opened contexts through Host presentation changes. Saved IDs and bounded labels never grant access.
- [x] Route delayed reviews to the originating container's remembered group without changing focus/caret; fence closed/reconnected/restored lifetimes and keep worktree selection independent per navigator.
- [x] Retain immutable moved/open Diff snapshots against their original checkout; preserve completed Find/viewport/selection and fresh selected-index staging without changing Files/Agent roots or worktree HEADs.
- [x] Recheck queued index admission on the serialized owner; reconnect, restore and final-reference closure revoke old admission, with no replay of uncertain external effects.
- [x] Use exact focused Git/Diff context for explicit terminal launch and validate it on the executor before starting a shell; deny unknown, revoked or replaced context and preserve existing terminal bindings.
- [x] Keep Files work valid inside Git-only groups and independent Files reconnect from cancelling Git requests; update live Git/Workspace/Build documentation and remove stale phase-smoke/not-compiled claims.
- [x] Pass the exact pinned product gate, Workspace feature boundary, Windows platform/Git-library compilation and native Linux package/version; complete actual two-worktree terminal and restart/reconnect acceptance. Windows runtime remains unchecked.

### `a0ffa3ec9e3fb3d3c713d8cea1a8c694543d002e` — browser read-only Agent tabs

- [x] Retain one LocalWeb Store from startup for unsaved browsing, activation, provider custody and first-prompt creation; strict exact-ID reads never create or activate a Session.
- [x] Share the native authorized privacy projection with the browser; revalidate exact Store/owner lifetime/Session/branch, membership and local target on every read, including unchanged heads.
- [x] Add bounded memory-only views with independent Find/viewport/selection, checked request lifetimes, cancellation and expiry; scrub closed/denied pending entries before abort callbacks.
- [x] Integrate explicit read-only branch actions and composer-free Solid panels; preserve the actual composer node, draft and caret, fence stale owner observations and clear protected labels/metadata on denied access.
- [x] Resolve durable receipts before lazy creation or account selection; preserve memory-only deduplication and retained-Store owner-replacement retries after account revocation without another Session or inference.
- [x] Pass the exact pinned product gate: bootstrap, formatting, 1,649 Rust tests with 2 existing ignored, and all 125 browser tests; Workspace-only check and 132 web-only library tests passed. Native Nix package/version passed.
- [x] Actual Chrome 155 against immutable package and real Rust/SQLite owner: two branches retained independent 120-hit Find, 1,100/1,750-pixel scroll and selection; actual composer/draft/caret and owner were unchanged. Unchanged authorized refresh and a changed hidden response retained state, a held actual owner reply could not reopen its closed tab, and unchanged-head membership revocation cleared transcript/Find/selection/labels. This predates the draft-retarget follow-up below.

### `68629d8ddcdba4bb502ef21ea04d5bae006f5fdd` — explicit browser draft targets

- [x] Bind private unsent text to minimal owner/Session/branch identity; external retargeting preserves draft/caret and blocks Send and publication until explicit Use draft here, which does not submit.
- [x] Consume confirmed local branch-open consent only for unchanged text/origin and its exact canonical target, including matching SSE-before-HTTP; quiet updates and unrelated targets grant no consent.
- [x] Fence late navigation/reset UI replies by browser lifetime, preserve text rebound during accepted-send waits, and keep shared-draft exact target/protocol semantics.
- [x] Rebuild the production Solid bundle and correct canonical CLI/current LocalWeb documentation without changing runtime protocols or State schemas. Exact pinned bootstrap/format/workspace Cargo and six-suite Node gate passed: 1,649 Rust tests, 2 existing ignored, 125 Node tests; immutable Nix package/version passed. The notice clickability correction follows below.

### `94697a7899ae52eb11167519c9abc05ee3517756` — reachable draft consent and final browser acceptance

- [x] Keep the consent notice in composer flow with pointer events restored and long labels wrapped; ordinary Use draft here remains reachable at 390 px. Global error notices retain their placement.
- [x] Pass the exact pinned bootstrap/format/workspace Cargo and six-suite Node gate: 1,649 Rust tests, 2 existing ignored, 125 Node tests, no failures. Rebuild the production Solid bundle into a private directory and match all eight committed artifacts byte-for-byte.
- [x] Build the exact immutable Nix package and run `bin/aw --version`: aw 0.15.0, exit 0. Served JS/CSS matched committed bytes.
- [x] Actual Chrome 155 against that immutable binary and a private Rust/SQLite owner: two branches retained independent 120-hit Find, scroll and selection; composer node/draft/caret and active owner stayed unchanged. Unchanged authorized refresh kept selected DOM; a changed hidden response retained Find/viewport; a held real owner reply could not reopen a closed tab, and reopening allocated a fresh lifetime.
- [x] Actual unchanged-head membership revocation cleared protected transcript, Find, selection and labels and refreshed the catalog. A held real snapshot and queued old SSE could not revive cleared owner DOM after a simulated transport 401; the LocalWeb fixture has no paired-cookie expiry.
- [x] Actual final bundle at 390 px with long Session/branch labels: external retarget disabled Send/Enter, ordinary Use draft here enabled the preserved draft separately from submission, and no effectful POST occurred. Additional instrumented early DOM/transport checks covered publication consent, same-target revision, matching activation SSE-before-HTTP and unrelated-target/new-text races; these do not claim live paired/shared acceptance.
- [x] Final private fixture retained exactly two Sessions, identical branch heads/revisions and zero runs/run_events/invocation_receipts/pipeline_runs/team_runs. Chrome recorded no uncaught runtime errors; the private owner exited 0. Transient diagnostics and completed agent scratch are removed after reconciliation.

### `57a6059a1d401186ffdf1ecbd4080645290a3063` — scoped browser Files/Git views

- [x] Retain one launch Files capability and physical directory identity; legacy and scoped reads reject root replacement without reopening the pathname or granting another root.
- [x] Add strict read-only source/scope envelopes and opaque origin-bound Git inventories, verified existing worktrees and immutable reviews; preserve legacy commands and exact pre/post-read authentication.
- [x] Bound physical HTTP work and encoded responses; validate full replies before context/cache admission so oversized reads preserve older valid handles. Reject copied worktree pointers and unknown/stale source, scope and review identities.
- [x] Add flat memory-only Files/Git containers with independent navigator state, exact document identities, eight combined containers and 32 total tabs including Agent; retain four physical reads, bounded metadata/content and explicit Refresh snapshots.
- [x] Route late replies only to their original document without stealing focus; preserve stable Explorer rows, palette origin/focus and the current Agent/root/composer/draft/caret.
- [x] Retain per-document File/Diff Find, selection and viewport in stable editors; scrub protected plaintext, derived editor state and DOM before retirement on close, denied scope, source replacement or logout.
- [x] Update canonical browser/surface/protocol documentation without changing native layout v4, SQLite v33, JSONL v1 or version 0.15.0. Final corrected-product gates and immutable acceptance follow below.

### `0159f0b119779f74b22d93d2ecd552306bbd7584` — bounded merged editors and final browser acceptance

- [x] Constrain both merged Diff editors to their pane height; the scoped CSS correction restores actual independent scrolling at desktop and 390 px without changing other editors.
- [x] Exact pinned product gate passed: `bash scripts/bootstrap.sh`, `cargo fmt --all -- --check`, `cargo test --workspace --locked` (1,662 passed, 2 existing ignored), and the six-suite Node command from TODO-SPEC.md (147 passed); no failures.
- [x] Exact feature boundaries passed: `cargo test --locked -p aw-cli --no-default-features --features web --lib` (132 passed) and `cargo check --locked -p aw-cli --bin aw --no-default-features --features workspace` (only two existing dead-code warnings).
- [x] Rebuild signed frontend source with the pinned Node/Vite toolchain and `npm --prefix web run build -- --outDir PRIVATE_TMP`; all eight artifact names and bytes match committed web/dist. Exact `nix build git+file:///home/pvl/spaces/abird/src/aw?rev=0159f0b119779f74b22d93d2ecd552306bbd7584#aw --no-link --print-out-paths --no-update-lock-file --cores 8` passed; immutable `bin/aw --version` reported aw 0.15.0, exit 0. Served JS/CSS matched committed bytes.
- [x] Actual Chrome 155 against that immutable executable and private Rust/SQLite owner: duplicate launch-root Files and two existing Git worktrees retained distinct same-path documents, independent 160-hit Find, selection and 1,100/1,750-pixel scroll, the same EditorViews and Explorer row focus. Ordinary Agent/document switches caused zero extra workspace reads; actual composer/draft/caret and owner stayed unchanged. At 390 px both Diff sides scrolled and Find controls fit.
- [x] Actual HTTP rejected forged mutation/root fields, wrong source/scope/review and Files reads through Git-only worktree grants. Actual physical launch-root replacement disclosed no replacement bytes and scrubbed both retained File editors, Find, DOM selection and protected labels; the original fixture root was restored immediately. A held actual file reply could not reopen its closed document, and reopening allocated a fresh lifetime.
- [x] Actual browser limits refused a ninth container or 33rd total tab before dispatch, kept existing documents usable at capacity and reused a closed slot. Held actual discovery plus simulated source_changed required fresh explicit discovery; simulated transport 401 plus a held actual discovery and queued old SSE could not revive cleared DOM. The local fixture does not establish live paired-cookie expiry. No uncaught runtime errors occurred.
- [x] Final fixture Git HEADs plus raw/logical indexes were byte-identical; sessions/runs/run_events/invocation_receipts/pipeline_runs/team_runs all remained zero. Targeted Chrome closed and the private owner exited 0. Transient fixture and completed agent scratch are removed after coordinator reconciliation.

### `2c0742506bb9e2620c4152585021262c3c7345b5` — measured real HTTP publication and shared-draft presence correctness

- [x] Add **opt-in** real HTTP/TCP tests of the shipping `/api/session` JSON and initial SSE high-water mark against an authenticated fake local owner, with bounded 32 KiB/256 KiB/4 MiB synthetic display and simultaneous delta writes; no provider/Session/tool effects.
- [x] Measure the shipping paired Team presence POST/list endpoints with real Store authorization, 8/128-row cap, expiry, concurrent updates and fresh revocation. Three complete HTTP rounds in the pinned shell record read/write p95, wall time and CPU ticks; see [README.md](README.md). Publication changes are **not** assumed faster end-to-end.
- [x] Correct cache admission for abandoned old draft generations: successful mode or audience revision retires all old cursors of the changed Session; unrelated Session cursors stay intact. Exact checked admission epoch fences a held request after Store authorization. Presence POST explicitly returns 409 on stale/abandoned generations; historical list is empty.
- [x] Execute actual regression tests covering revoke, re-grant, mode Off, held-after-authorization stale reply, unrelated Session survival, active TTL and no provider calls. No new execution owner, permission grant, schema or listener.
- [x] Exact source-HEAD proof: `cargo test --workspace --locked --exclude aw-store -- --test-threads=4` (1,274 passed, four ignored); `cargo test --locked -p aw-store --lib -- --test-threads=4` (201 passed) and five explicit Store `--test` targets (7+29+9+23+88 passed), all in pinned Nix shell. This is **split executable coverage**, not a claimed successful monolithic `cargo test --workspace --locked`.
- [x] Exact source-HEAD: both ignored HTTP measurement tests explicitly run and pass; `cargo fmt --all -- --check`, strict `cargo clippy --locked -p aw-cli --lib -- -D warnings`, web-only Rust suite (132 passed), Workspace-only compilation (two known dead-code warnings), six Node suites (147 passed), and `cargo build --locked -p aw-cli --bin aw` with `aw 0.15.0`. No push, tag, new schema or restored-State activation.

### `e06ffa7007997310bac2c5e5c14938fde92dc320` — same-window browser reparenting substrate

- [x] Implement exact same-window File/Diff document reparenting inside the existing shipping Solid `createWorkspaceViews` owner. Keep the same document ID, original source/scope/Review, cached content, late-read lifetime and file editor state; moving starts no HTTP read, Agent activation or new grant.
- [x] Refuse nonexistent/denied/mismatched/duplicate destinations without changing source/destination view state. Moving an immutable Diff to a Git navigator inspecting another worktree retains its original Review/scope; source container closure does not close the moved document, destination closure scrubs it.
- [x] Add a compact, accessible File/Diff source-bar selector only when a second matching navigator exists. Preserve focus, original CodeMirror instance, prompt draft/caret and responsive geometry. Rebuild the embedded Solid bundle with the **same eight** asset names, updating only `web/dist/app.js` and `web/dist/style.css`.
- [x] Exact source-HEAD Node gate: the six TODO-SPEC suites run in the pinned Nix shell with **150 passed, zero failed** (three added shipping controller move/mismatch/Git-context tests). Exact web-only Rust feature test: `cargo test --locked -p aw-cli --no-default-features --features web --lib` — **132 passed**. `cargo fmt --all -- --check` passed.
- [x] Pinned exact-HEAD source-product gate executed as bounded **split commands**: `cargo test --workspace --locked --exclude aw-store -- --test-threads=4` (**1,274 passed, four ignored**), `cargo test --locked -p aw-store --lib -- --test-threads=4` (**201 passed**), and five explicit Store integration `--test` targets (**7+29+9+23+88 passed**). Rebuilt the eight frontend artifacts privately and compared all bytes to tracked `web/dist`. The single monolithic Cargo command was **not** claimed to pass.
- [x] Actual shipping browser/owner Chrome 155 acceptance of the same candidate code: private fake-provider/files-root with tools Off, **390×844 Dark**, two Files navigators, full rooted File, in-window move, close origin. Existing CodeMirror instance, unsent draft/caret, absence of extra HTTP reads, zero horizontal overflow, selector conditional visibility and six zero State run/effect counters passed; no uncaught JS errors. This is **not** cross-window docking, multi-browser authority or native-terminal proof.

### `7de865852df747d0c3280c96180ef5dacbc6c8f2` — owner-mediated File-only cross-window docking

- [x] Add a bounded, memory-only **File-only** browser handoff registry to the existing authenticated Rust web owner: independent OS-random window IDs/proofs, 90-second tickets, 180-second window expiry, 24-window/24-ticket caps, exact source/scope/owner epoch, and phases Prepared / Accepted / Declined / Released. Never grant a new file root, expose file content in ticket inspection, start a provider, or attach an Agent/PTY.
- [x] Reauthorize the original retained File through `SurfaceHost::execute_if` at prepare, receiver inspect/accept, and origin release. Enforce same-origin HTTP authorization, distinct original/receiver proofs, checked single acceptance, no release before acceptance, and explicit receiver decline/cancel with no replay or downgraded accepted receipt.
- [x] Add a restrained shipping Solid **Move File to a new window** action only for a loaded read-only File. Open from the user click, navigate using an ephemeral URL fragment removed immediately from the receiver history, independently read/render in the destination before acceptance, then retire the origin tab only after exact owner-confirmed release; preserve the original Agent, draft and provider context. No source bytes or browser credentials move in the ticket.
- [x] Fail closed on missing/changed source or grant, stale/expired window, stale original scope, forged File/path/document identity, duplicate/competing receiver, declined/unknown acknowledgement, and replacement root. Confirmed auth denial scrubs the receiver view; an unknown acceptance result retains its already-authorized read-only view without claiming success. Browser popup cancellation leaves the original tab.
- [x] Source-HEAD `cargo test --locked -p aw-cli --lib docking` — **7 passed**. Full pinned Nix Rust coverage executed as bounded split commands: `cargo test --workspace --locked --exclude aw-store -- --test-threads=4` (**1,281 passed, four ignored**); Store library (**201 passed**) and five Store integrations (**7+29+9+23+88 = 156 passed**). Exact same source-HEAD Node six-suite (**155 passed**), web-only Rust (**132 passed**), `cargo fmt --all -- --check`, strict library Clippy, Workspace-only/minimal feature builds and `cargo build --locked -p aw-cli --bin aw` passed. All eight embedded Vite assets rebuilt byte-identically from source. A monolithic workspace Cargo command is **not** represented as completed.
- [x] Actual Chrome 155 through DevTools against a private, synthetic rooted LocalWeb/SQLite owner: two distinct browser pages, receiver's independent bounded File read, single source-tab release after acceptance, unsent source draft preserved, receiver refresh after closing the source page, 390×844 Dark with no page-wide overflow, no JS runtime exceptions and all six tested Session/run/effect counters zero. Synthetic provider only, Tools Off, no real account or external surface. Test-only browser processes terminated; no deployment.

### `00559eb070cc07bd96956f8d6b23fe9394202ca5` — immutable Git Review cross-window docking

- [x] Extend the existing bounded, authenticated File handoff with a **Review-only preparation** route and a shared typed File/Review resource. Keep the same cryptographic window identity, ticket lifetime, one-receiver acceptance, explicit decline, expiry and origin Release. The existing File request remains strict, with no new schema or grant.
- [x] Reauthorize every Review using the **original retained immutable Review ID** and its exact verified Git scope through `SurfaceHost::execute_if`, at prepare, inspect, accept and release. Support existing non-root worktree scopes without installing a new Files root or navigator-wide Git context. Reject forged IDs/scopes/source/instance, evicted Reviews and replaced linked checkout directories.
- [x] Ship a compact **Move Review to a new window** control beside the File control. The receiver independently loads/renders the immutable Review through a document-only scoped read, without replaying the original Git inventory or sending source/Diff text through the handoff. Same-window File/Diff reparenting remains unchanged; neither flow retargets the Agent or carries provider/PTY/Tool grants.
- [x] Execute real Owner/Review and lifecycle tests: `cargo test --locked -p aw-cli --lib docking` — **11 passed**. Exact source-HEAD pinned Nix split gate: **1,285 non-Store Rust tests (four ignored), 201 Store library tests, 156 Store integrations**, zero failures. Eight Node suites (**161 passed**), web-only Rust (**132 passed**), formatting, strict library Clippy, workspace/minimal feature builds and eight byte-identical Vite assets passed; no claim that one monolithic workspace command finished.
- [x] Actual Chrome 155 on a private synthetic Git+SQLite LocalWeb owner: a read-only Diff opened in a second browser page, independently fetched its original immutable Review, cleaned the fragment, and remained refreshable after its origin page closed. The original unsent draft/caret survived, 390×844 Dark had no page-wide overflow, no JS exceptions occurred, fixture Git HEAD/index were byte-identical, and six SQLite Session/run/effect counters stayed zero. A separate **File** cross-window Chrome regression passed with the same invariants. Non-root Review scope was accepted through real HTTP tests and browser-model tests; a separate live non-root Chrome walk remains open.

## Uncommitted

- [~] Finish **cross-window presentation** semantics beyond the now-accepted File/Review baseline: bounded Find/viewport/selection handoff, verified destination focus/lifetime, multi-window crash/reload/ABA and paired browser acceptance. An accepted ticket is not proof a browser process is continuously alive; unknown acceptance remains inspectable without replay. Browser labels/messages cannot grant root or execution authority.
- [ ] Design and implement **native second-terminal/PTY docking** with original Runtime handle, exclusive control/lease, window closure/restoration and zero-replay proof. No Agent/PTY/model/provider ownership transfer or native saved-layout migration is authorized by the browser File/Review handoff.


## Tests and verification

- [x] Exact `e06ffa7` source checkpoint in pinned Nix shell: source-product Rust gate split by package (1,274 non-Store + 201 Store lib + 156 Store integration passed), Node six-suite gate (150 passed), web-only feature (132 passed), `cargo fmt` and byte-for-byte private Vite rebuild of eight tracked browser assets. This is not a claim of a completed monolithic `cargo test --workspace --locked` command.
- [x] Real Chrome 155 user-visible **same-window** File move with two Files navigators, 390×844 Dark, original editor/draft/caret retained, conditional control visibility, no new owner request, no JS exceptions, zero six-category SQLite effects. Cross-window/other-browser/native docking acceptance remains open.
- [x] File-only **browser** cross-window handoff tested on exact `7de8658`: seven authenticated Axum owner/expiry/revocation/decline/release tests, 155 Node tests, actual two-page Chrome 155 File reload after origin browser closure and six zero private State counters. Final Store/feature/build gates as recorded in the committed group above.
- [x] Immutable Git Review **browser** docking on exact `00559eb`: 11 authenticated owner-side File/Review lifecycle tests, 161 Node tests and real two-page Chrome Git Diff + existing File flows. Scope/Review retention, receiver re-read after origin close, unchanged Git HEAD/index and six zero private State counters passed. See the committed group above for strict/feature/product gate commands.
- [ ] Cross-window **Find/scroll/selection**, native second-terminal/PTY docking and broader crash/ABA/paired-browser acceptance are still open; the read-only browser Review ticket does not transfer execution or root authority.

- [x] Browser Files/Git retained capability and actual-owner HTTP regressions — exact pinned `cargo test --workspace --locked` and actual immutable-owner HTTP/Chrome on `0159f0b`, passed as scoped above; no model/tool/Git/terminal effects.
- [x] Browser container model regressions: bounded IDs/state/work, stale replies and denial cleanup — existing six-suite Node gate on exact `0159f0b`, 147 passed.
- [x] Shipping Solid File/Diff editor acceptance — actual Chrome 155 on immutable `0159f0b`, independent navigator/Find/viewport/selection, preserved Agent/draft/caret, limits and stale-reply refusal passed as scoped above.
- [x] Exact scoped-browser product gate — pinned bootstrap/fmt/workspace Cargo (1,662 passed, 2 existing ignored), six-suite Node (147 passed) and byte-identical eight-artifact production frontend rebuild on `0159f0b`.
- [x] Immutable package/version and actual Chrome duplicate Files/two-worktree acceptance on `0159f0b` — passed with unchanged heads/indexes, six zero State counters and owner exit 0. Additional Files roots remain unavailable without explicit trusted-owner grants.

- [x] Exact follow-up product gate/package and actual Chrome draft-retarget/late-observation/closed-view/revocation acceptance — passed on `94697a7` as recorded above, with unchanged branch heads and zero execution effects.

- [x] Browser Agent HTTP regressions against real SQLite State: exact authority/identity, unsaved owner reuse, privacy, revocation and zero execution effects — `cargo test --workspace --locked`, exact pinned `94697a7`, passed.
- [x] Browser Agent view model regressions: delayed/closed/superseded replies, duplicate targets, bounded caches, independent state, expiry and logout — existing six-suite Node command from TODO-SPEC.md, exact pinned `94697a7`, 125 passed.
- [x] Shipping Solid production build — `npm --prefix web run build -- --outDir PRIVATE_TMP`, exact pinned `94697a7`, all eight artifacts matched committed bytes. `nix build git+file:///home/pvl/spaces/abird/src/aw?rev=94697a7899ae52eb11167519c9abc05ee3517756#aw --no-link --print-out-paths --no-update-lock-file --cores 8` and full pinned product gate passed.
- [x] Actual shipping browser against the Rust owner and two real SQLite branches: independent Find/scroll, unchanged active Agent/draft/caret, denied refresh clearing, stale close protection and zero execution effects — Chrome 155 on immutable `94697a7`, passed as scoped above.

- [x] Store exact-ID/authority/revocation/active-run/history/no-effect regressions — `cargo test --workspace --locked`, pinned Nix shell on `b33a418`, passed.
- [x] Layout migration, independent descriptors, split/move/global limits and native persistence protections — same executed workspace Cargo gate on `b33a418`, passed.
- [x] Native real-Reader/executor/TestBackend cases: two targets, independent viewport/find/selection, owner/draft/caret, duplicate identities, expiry/revocation and unavailable states — same executed workspace Cargo gate on `b33a418`, passed.
- [x] Actual native PTY — `aw workspace open local --config-file CONFIG --provider-profile fixture --state-dir STATE --workspace-directory ROOT --workspace-layout LAYOUT --tools-mode off`, synthetic private fixtures. Broad two-view/Unicode/resize/`o` acceptance passed on `1c2ec6e`; focused Searching→1 match/No matches/80 matches, exact pins, draft/caret, zero effects, quit 0 and identical stty passed with the immutable Nix executable on `3b0dbb5`.
- [x] `cargo check --locked -p aw-cli --bin aw --no-default-features --features workspace` — pinned Nix shell on `b33a418`, passed.
- [x] `bash scripts/bootstrap.sh` — pinned Nix shell on `b33a418`, passed.
- [x] `cargo fmt --all -- --check` — pinned Nix shell on `b33a418`, passed.
- [x] `cargo test --workspace --locked` — pinned Nix shell on `b33a418`, 1,638 passed, 2 existing ignored, no failures.
- [x] `node --test tests/web_model.test.mjs tests/web_workspace.test.mjs tests/web_context_flow.test.mjs tests/web_multiplayer.test.mjs tests/web_agent_parity.test.mjs tests/web_rich_markdown.test.mjs` — pinned Nix shell on `b33a418`, 107 passed.
- [x] Changed-content committed-Find and close/restore ABA regressions, including inactive footer/focus and consumed flash selection — executed by the workspace Cargo gate on `b33a418`, passed.
- [x] `nix build 'git+file:///home/pvl/spaces/abird/src/aw?rev=b33a4183f6032a024d1b73409d2c9134ea0aab24#aw' --no-link --print-out-paths --no-update-lock-file` — passed; built native `bin/aw --version` exited 0 with `aw 0.15.0`.
- [~] Further native narrow/maximize, modal-focus and child-PTY acceptance, plus docking-specific checks when implemented — **partially exercised**, not closed: on the starting `a20a4bd` tree, 90 native TestBackend gallery renders passed; isolated real PTY F1/resize/explicit quit and child-shell Ctrl+C/quit-all both exited 0 with restored termios and six zero State counters. A live 65×20 Ctrl+K Z maximize/restore + F1/resize/quit smoke also passed with restored termios; exact focus/caret/split matrix, live browser screenshots and docking-specific checks remain open; see README.

- [x] Scoped Files layout regressions: two same-path scopes, container membership/global limits, legacy full/exhausted layouts, malformed/mixed versions and split/move/close/save/load — `cargo test --workspace --locked`, pinned Nix shell on `b33a418`, passed.
- [x] Actual retained-root backend regressions: distinct read/list/search results, unknown/revoked/reconnected scopes, root replacement and capability cleanup — same executed workspace Cargo gate on `b33a418`, passed.
- [x] Native Browser/executor/TestBackend acceptance: independent same-path content/find/viewport, invoking-container preview/Explorer focus, stale reads/search/highlights, explicit refresh/reconnect races, moved-tab labels and unchanged Agent/draft/effect admission — same executed workspace Cargo gate on `b33a418`, passed.
- [x] Actual scoped-Files PTY — `aw workspace open local --config-file CONFIG --provider-profile fixture --state-dir STATE --workspace-directory ROOT --workspace-layout LAYOUT --tools-mode off`, immutable Nix package on `3851dbf`, private synthetic fixtures: distinct same-path Unicode roots, independent Find/scroll, Explorer focus, exact draft/caret, save/split/load, restart with unavailable scope, explicit same-ID reconnect and 100×30→220×64 resize passed. Both app exits were 0 with identical before/after stty; sessions/runs/run_events/invocation_receipts/pipeline_runs/team_runs stayed zero.

- [x] Windows platform scoped-directory and DirectoryIdentity tests compile — `cargo check --locked -p aw-platform-win --target x86_64-pc-windows-gnu --tests`, full pinned Nix shell with Rust 1.98.1 Windows std on `b33a418`, passed.
- [ ] Windows scoped-directory and DirectoryIdentity filesystem tests — execute on an actual Windows host; reparse-point case requires symlink privilege or Developer Mode. Compilation does not supply this runtime result.

- [x] Git-container layout/persistence regressions: legacy/mixed migration, independent and dormant bindings, shared limits, split/move/close, malformed ownership/versions and atomic allocation refusal — `cargo test --workspace --locked`, pinned Nix shell on `b33a418`, passed.
- [x] Actual Git backend/executor regressions: distinct worktrees, delayed/closed/rebound/restore replies, launch discovery replacement, retained immutable reviews, context bounds/pruning and selected-index-only fresh staging — same executed workspace Cargo gate on `b33a418`, passed.
- [x] Native Git/executor/TestBackend acceptance: originating remembered-group/focus, independent Find/viewport, moved completed selection, explicit open/reconnect, mixed File views, unchanged Agent/draft, and queued terminal denial after replacement/revocation — same executed workspace Cargo gate on `b33a418`, passed.
- [x] Actual scoped-Git PTY — `aw workspace open local --config-file CONFIG --provider-profile fixture --state-dir STATE --workspace-directory ROOT --workspace-layout LAYOUT --tools-mode off`, immutable Nix package on `b33a418`, private two-worktree fixture and trusted real-Git wrapper with an explicitly armed 3-second diff delay: origin routing/Explorer focus, distinct Unicode reviews with independent 120-hit Find/scroll, selected-index stage/unstage and stale refusal, unchanged Files/Agent/draft/caret/HEADs, save/load, 100×30→220×64 resize, unavailable restart contexts and exact same-ID reconnects passed. Both app exits were 0 with byte-equal before/after stty; both indexes ended clean; sessions/runs/run_events/invocation_receipts/pipeline_runs/team_runs stayed zero.

- [x] Windows Git library compiles — `cargo check --locked -p aw-git --target x86_64-pc-windows-gnu --lib`, full pinned Nix shell with Rust 1.98.1 Windows std on `b33a418`, passed.

## Deferred

- [ ] Remote views — remain unavailable until exact machine/member/revision/expiry/offline tests exist; no local fallback or execution retargeting.
