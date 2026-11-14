# Delegated browser read authority

Owner: @pvl
Status: Agent branch complete; File/Review distinct-Actor backend lifecycle complete; File/Review Solid/live acceptance pending

## Boundary

Distinct-user browser access is a delegated **read capability**, not a browser-login side effect.

A grant is memory-only and owned by the current web owner. It binds:

- exact owner principal + stable user/security epoch;
- exact recipient principal + stable user/security epoch;
- exact source and execution/site identity;
- one narrow immutable resource identity:
  - Agent branch: Store/Session/branch;
  - File: source/scope/relative path;
  - Review: source/scope/immutable review ID;
- monotonically unique grant generation;
- issued/deadline;
- active/revoked state;
- bounded parent-authority evidence when the resource owner requires it.

Paths, labels, document/tab IDs, cookies, pairing codes and docking tickets never authorize a resource.

## Admission and revalidation

Grant creation requires current owner identity plus a current, already-known recipient identity in the same authoritative Store. It does not create users, Sessions, filesystem roots, Git contexts, tools, providers or Agents.

Every receiver read produces a grant witness containing the exact grant ID, generation, recipient and resource. Before any protected response is published, the owner revalidates the witness against:

1. the same active grant/generation;
2. exact recipient principal/user/security epoch;
3. exact owner principal/user/security epoch;
4. unexpired owner lifetime;
5. current parent resource authority.

A held response is therefore unusable after revoke, owner/recipient security-epoch change, resource replacement, source/site change or grant reissue.

Revoke is monotonic. Regrant allocates a new generation; stale work from the prior generation cannot become valid because names or resource bytes happen to match again.

## Lifecycle

The registry is bounded and non-persistent. Owner logout/expiry/shutdown revokes all grants and protected receiver state must be scrubbed by the browser integration. Browser reload/reconnect has no grant-restoration path; it must reacquire an explicitly current capability.

Receiver heartbeat/ACK is presentation/liveness evidence only. It is never part of grant authority and never proves perpetual receiver liveness. Crash/partition after an ACK leaves the grant inspectable/revocable but cannot authorize source release or execution.

## Non-authority

A delegated browser read grant never grants:

- Agent activation, retargeting, prompt submission or Job control;
- Files roots beyond the exact File resource;
- Git inventory/context/write authority beyond the exact immutable Review;
- provider/account/payer/tool authority;
- PTY/process custody;
- remote-device trust.

Remote-device identity/trust remains a separate gate.

## Owner-side checkpoint — 2026-11-14

Implemented before any docking/UI integration:

- typed `Binding` for owner, recipient, source/site, exact resource and parent evidence;
- bounded memory-only registry with monotonic grant generation;
- read-only known-user lookup that cannot create an identity from an untrusted recipient;
- grant/revoke/authorize/publish-time revalidation against owner + recipient security epochs;
- owner logout/expiry/shutdown and team-member cancellation revoke all delegated grants;
- reload/restart starts with an empty registry and cannot recover an old grant ID.

Negative/race coverage is green for wrong recipient/resource, revoke-before-publish,
unchanged-resource held response, revoke/regrant generation, owner and recipient
security-epoch changes, expiry/parent change, reload after revoke, and unknown
recipient non-creation. Receiver ACK/liveness is deliberately absent from the
authority API, so a recent ACK cannot extend a grant.

Validation at this checkpoint:

- `cargo test --locked -p aw-store --lib identity_continuity` — 4/4 passed;
- `cargo test --locked -p aw-cli --lib delegation` — 8/8 passed;
- `cargo test --locked -p aw-cli --lib logout_revokes_delegated_browser_authority` — 1/1 passed;
- `cargo test --locked -p aw-cli --no-default-features --features web --lib delegation` — 8/8 passed;
- `cargo clippy --locked -p aw-cli --lib -- -D warnings` — passed;
- existing same-owner browser pairing — 7/7 passed;
- existing docking with pinned `AW_GIT` — 24/24 passed;
- existing team owner tests — 40 passed / 1 intentional ignored.

This owner-only checkpoint is superseded by the Agent-branch docking integration below.

## Distinct-Actor Agent-branch docking checkpoint — 2026-11-14

The existing Team gateway is the distinct-recipient authentication boundary: it
selects an immutable participant `AppState` before delegated routing. Browser
fields never choose an Actor. Ordinary same-owner pairing/docking remains
unchanged and does not become a cross-user grant.

The first integrated resource is an exact **read-only Agent branch**. File and
Review delegation remain separate future extensions so this slice cannot grant a
filesystem root, Git inventory/worktree or mutation authority by accident.

Implemented:

- bounded Team-owned Agent docking windows/tickets with exact authenticated
  owner/recipient, Store/source epoch, Session/branch and grant generation;
- owner-only grant creation from an already-authorized local branch; recipient
  gets no Session participation row, Agent activation or direct Store authority;
- receiver reads through the owner's strict branch reader only after current
  delegated authority passes; response encoding reuses the ordinary Agent-view
  privacy projection;
- grant/parent authority revalidated before the read, after the blocking read and
  again at publication; final publication and source Release are linearized
  against explicit revoke;
- receiver must complete a current authorized read before Accept; source then
  requests confirmation and Release requires an ACK no older than three seconds;
- Prepared-only Cancel cannot roll back an Accepted/Released handoff; a lost
  Release response therefore keeps the source view instead of guessing success;
- explicit owner revoke, owner logout/expiry/shutdown and recipient
  logout/expiry revoke the relevant grant. Recipient reload/reconnect cannot
  mint another receiver window for an already-consumed ticket;
- browser Agent-view state supports a per-grant reader/lifetime. A known 403
  scrubs transcript, Find and selection for duplicate views under that exact
  grant before abort callbacks, while an independently authorized view of the
  same branch remains intact;
- a correlation-only `#aw-agent-dock=` receiver fragment is stripped before
  disclosure. No transcript text, cookie, provider credential, tool receipt or
  execution capability is carried in the fragment/ticket.

Negative/race coverage includes wrong recipient, Accept-before-read,
revoke-before-held-response publication, unchanged content after revoke,
regrant generation, stale/fresh ACK, recipient logout, lost Release response,
retry/reload refusal and independent same-resource cache isolation. Tests assert
unchanged owner/recipient active Session/branch and zero provider calls.

Validation at this checkpoint:

- `cargo fmt --all -- --check` — passed;
- `cargo test --locked -p aw-cli --lib delegation` — 8/8 passed;
- `cargo test --locked -p aw-cli --lib delegated_agent` — 3/3 passed;
- `cargo test --locked -p aw-cli --lib web::team::tests` — 43 passed / 1
  intentional ignored measurement;
- `AW_GIT=$(command -v git) cargo test --locked -p aw-cli --lib docking` —
  25/25 passed (24 existing docking tests plus the matching delegated test);
- `cargo test --locked -p aw-cli --lib browser_pairing` — 7/7 passed;
- `cargo test --locked -p aw-store --lib identity_continuity` — 4/4 passed;
- `cargo check --locked -p aw-cli --no-default-features --features team --lib`
  — passed with the pre-existing reduced-feature `bind_execution_owner` dead-code
  warning;
- `cargo test --locked -p aw-cli --no-default-features --features team --lib delegated_agent` — 3/3 passed after the final publication-lock tightening, with only the pre-existing reduced-feature `bind_execution_owner` dead-code warning;
- `node --test tests/web_agent_parity.test.mjs tests/web_docking_handoff.test.mjs`
  — 30/30 passed;
- `cargo clippy --locked -p aw-cli --lib -- -D warnings` — passed.

The broad `node --test tests/web_*.test.mjs` attempt reached 173 passing tests,
then two unrelated dependency-backed suites failed at module load because the
local `web/node_modules` is incomplete (`yjs`, `markdown-it`). The supported
`npm ci` bootstrap could not repair it in this L5 sandbox because registry fetches
failed with `UNABLE_TO_GET_ISSUER_CERT_LOCALLY`. Do not record that broad Node
attempt as a product failure or a full green gate; the changed suites above are
fully executed and green.

Not claimed at this checkpoint:

- no Solid control/recipient UX is wired yet;
- no live Chrome/screenshot/visual acceptance;
- no delegated File/Review root/worktree authority;
- no remote-device trust or remote view enablement;
- no Agent execution retargeting, provider/tool authority or PTY custody.

That Solid integration boundary is completed below. The next independent gate is actual two-profile live-browser acceptance; remote-device trust remains later and separate.

## Shipping Solid integration checkpoint — 2026-11-14

The product UI now consumes the delegated Agent-branch authority without tying
it to multiplayer draft sharing or Workspace Files/Git surfaces:

- `/api/team/me` advertises the narrow `delegated_agent_views` capability;
- authenticated Team browsers can read `/api/team/delegation/recipients`, which
  exposes only other participant IDs/display names. The directory is metadata,
  performs no Store mutation, and does not itself grant a resource;
- ordinary owner Agent views show a recipient chooser only when Team delegation
  is available. The user must explicitly choose a recipient before creating a
  short-lived read-only link;
- delegated receiver Agent views are marked delegated and do **not** expose a
  re-delegation control;
- `#aw-agent-dock=` is stripped before any receiver read. After Team identity is
  authenticated, the received exact branch is opened as an ordinary read-only
  Agent tab backed by the per-grant reader;
- the existing 500 ms Agent-view pulse / five-second refresh cadence continues
  to revalidate delegated reads. A 403 scrubs transcript, Find and selection for
  the exact grant lifetime while independently authorized views survive;
- browser auth expiry clears recipient metadata and active share-link UI as well
  as protected Agent/Workspace views;
- generated embedded frontend assets (`web/dist/app.js`, `web/dist/style.css`)
  were rebuilt from the shipping Solid source. Dependency manifests/locks are
  unchanged.

Validation for the UI checkpoint:

- `cargo test --locked -p aw-cli --lib delegated_agent_recipient_directory`
  — 1/1 passed;
- `cargo test --locked -p aw-cli --lib delegated_agent` — 4/4 passed including
  recipient-directory metadata, exact recipient lifecycle, held revoke race and
  recipient logout;
- `cargo test --locked -p aw-cli --lib web::team::tests` — 44 passed / 1
  intentional ignored measurement;
- `cargo test --locked -p aw-cli --no-default-features --features team --lib delegated_agent`
  — 4/4 passed, with only the pre-existing reduced-feature
  `bind_execution_owner` warning;
- `cargo clippy --locked -p aw-cli --lib -- -D warnings` — passed;
- `cargo check --locked -p aw-cli --no-default-features --features team --lib`
  — passed with that same existing warning;
- `node --test tests/web_*.test.mjs` — **180/180 passed**;
- `npm run check --prefix web` — production Solid/Vite build passed, 174 modules
  transformed. The existing >500 KiB code-surface chunk warning remains;
- consecutive production rebuilds produced byte-identical hashes for all **8/8**
  embedded frontend assets;
- `git diff --check` — passed.

The L5 npm dependency bootstrap initially failed because the sandboxed `/etc`
CA symlink target was unavailable. Using the immutable Nix CA bundle at
`/nix/store/15x4d7i4p60sjqvls7riw7sd42551s19-nss-cacert-3.126/etc/ssl/certs/ca-bundle.crt`
allowed locked `npm ci --prefix web --ignore-scripts --no-audit --no-fund` to
complete. This changed only ignored `web/node_modules`; package files stayed
unchanged.

Delegated File/Review, remote-device trust, Agent execution retargeting and PTY
custody remain outside this completed Agent-branch slice.

## Distinct-Actor live-browser acceptance — 2026-11-14

The shipping Agent-branch delegation flow is now accepted in **real Chrome 157**
using two independent browser user-data directories and the current Rust Team
server/library. The private fixture had exactly two principals and private
Sessions (`alice`, `bob`), no tools/surfaces, and an in-process provider whose
only behavior was incrementing a call counter if inference were ever reached.

The live run used Chrome DevTools Protocol only as the browser automation
transport; pairing, read-only-view opening, recipient selection, link creation,
delegated receive, Find, refresh and source-release observations were performed
against the actual shipping DOM/routes. Both pages were forced to **390×844**
through `Emulation.setDeviceMetricsOverride`; source and receiver each measured
390 px `innerWidth`, document `scrollWidth`, and active Agent-panel width, with
no page-wide horizontal overflow.

Accepted facts:

- the pre-auth pairing copy correctly describes the one-use code as selecting
  its assigned browser identity; it no longer claims every second browser gets
  the same owner Actor;
- Alice and Bob authenticated as distinct principals
  `internal:delegation-live:alice` / `internal:delegation-live:bob` with
  distinct HttpOnly cookie **values** in independent Chrome profiles;
- Alice's delegation directory exposed exactly Bob as `{id, display_name}`;
- Alice opened her own read-only Agent branch through the visible `View`
  control, explicitly selected Bob, and used `Create read-only link`;
- the generated URL contained only the 64-hex `#aw-agent-dock=` correlation
  fragment. It contained neither transcript text nor Alice's unsent draft;
- Bob opened the URL in his already-authenticated profile; the fragment was
  removed before disclosure and Alice's read-only branch rendered without
  changing Bob's active private Agent/Session (`bob private`);
- Bob retained `tools=[]`, `/api/workspace` remained disabled, and the received
  delegated view exposed no recipient selector/re-delegation button;
- Bob's read-only Find state worked independently; source confirmation/release
  removed only Alice's source read-only tab;
- Alice's unsent text `alice private unsent draft` and caret at offset 7 were
  preserved exactly across the transfer;
- an authenticated browser-side Bob `/api/logout` followed by the shipping
  delegated-view refresh path scrubbed the received view and returned Bob to
  the pairing screen, while Alice remained authenticated;
- CDP observed **zero uncaught JavaScript exceptions and zero `console.error`**
  across Alice, Bob, and the delegated receiver page;
- the synthetic provider counter ended at **0**. The private SQLite fixture had
  exactly **2 Sessions** and zero `runs`, `run_events`, `invocation_receipts`,
  `pipeline_runs`, `team_runs`, `llm_operations`, `llm_attempts`, and
  `shared_draft_submissions`.

One genuine UX bug was exposed during this acceptance before the successful
run: the generic connect screen still contained historical same-owner-pairing
copy ("same owner's access"), which is false for Team participant codes. The
copy was corrected to identity-bound wording and the final Chrome run verified
the corrected text.

Post-fix product gates were rerun: `node --test tests/web_*.test.mjs` passed
**180/180**; the production Vite build transformed 174 modules and consecutive
rebuilds retained byte-identical hashes for all **8/8** embedded frontend
assets; `cargo fmt --all -- --check`, strict aw-cli library Clippy, and
`git diff --check` passed after the temporary live harness was removed.

Harness notes: dotlink's sandbox omits `/etc` Fontconfig, so Chrome initially
aborted in Skia. The final run explicitly used the immutable Nix Fontconfig
configuration at
`/nix/store/cy5jc0878z0s8zlxnn9a7x6livd6fbnx-fontconfig-conf/etc/fonts/fonts.conf`.
The ordinary CLI binary in `target/debug` was stale because the tunnel killed a
long final relink, so the final acceptance deliberately used a temporary
current-source example that called public `aw::web::team::serve` with the same
shipping library/frontend. The temporary example/browser fixtures were removed
after evidence was recorded; they are not product files.

This completes the **Agent-branch** distinct-user browser delegation gate.
Remaining work is deliberately separate: delegated File/Review capabilities,
remote-device identity/trust, post-ACK arbitrary network/process partitions,
remote/distinct-Actor PTY custody, non-Unix custody, and broker crash/reboot
restoration.

## File + immutable Review authority substrate — 2026-11-14

The next backend slice deliberately **does not attach a SurfaceHost to Team
participants or expose `/api/workspace` to a recipient**. Team remains the
distinct-Actor browser gateway with no Files/Git catalog by default. Instead,
the owner-side resource semantics are now canonicalized first so a later Team
route cannot invent a weaker second authorization engine.

Implemented:

- new `src/web/surface_read.rs` owns one typed exact retained target:
  `source_id + scope_id + owner_epoch + (File path | immutable Review ID)`;
- every retained read checks current browser auth, source identity and owner
  epoch before and after the bounded `SurfaceHost::execute_if` call;
- **File is launch-root only**. A caller cannot turn a delegated relative path
  into authority for another retained Files root/scope;
- **Review may use an already-retained Git scope**, including non-root linked
  worktree scopes, but only by exact immutable Review ID. The primitive never
  performs Git inventory selection or creates a worktree/context grant;
- the canonical Review read verifies the returned retained object is still a
  Review with the exact requested ID;
- existing same-owner File/Review docking now uses this shared exact-read
  primitive. Docking retains only its presentation/fingerprint continuity
  layer and performs a final owner/source recheck after that comparison;
- the existing delegated grant registry already binds File as
  `source/scope/path` and Review as `source/scope/review_id`, plus recipient,
  generation/security epochs and `Surface { owner_epoch }` parent evidence.

New authenticated tests combine a real `SurfaceHost`, persistent Store and a
**distinct known recipient** with that grant registry:

- exact launch-root File grant reads only the bound path; a forged scope and a
  different-path binding are denied;
- a completed File read followed by owner revoke cannot publish held unchanged
  bytes because final grant revalidation fails;
- exact retained Review grant reads the immutable held Review and denies both a
  forged grant Review ID and a direct forged retained-read ID;
- neither path invokes the model/provider.

Validation at this checkpoint:

- `AW_GIT=$(command -v git) cargo test --locked -p aw-cli --lib delegated_surface`
  — **2/2 passed**;
- `AW_GIT=$(command -v git) cargo test --locked -p aw-cli --lib docking` —
  **25/25 passed**, including linked-worktree Review-without-Files-grant,
  replacement/eviction, File fingerprint continuity, stale ACK and release
  races;
- `cargo test --locked -p aw-cli --no-default-features --features web --lib delegation`
  — **8/8 passed**. Reduced Web reports the already feature-conditional unused
  Team helpers in addition to the pre-existing `bind_execution_owner` warning;
- `cargo check --locked -p aw-cli --no-default-features --features team --lib`
  — passed with the existing reduced-feature `bind_execution_owner` warning;
- `cargo clippy --locked -p aw-cli --lib -- -D warnings` — passed;
- formatting and `git diff --check` — passed.

Not yet implemented: a cross-Actor File/Review HTTP ticket/receiver route or
shipping Solid controls. That next boundary must supply the **granting owner's
retained SurfaceHost explicitly** while continuing to keep the recipient's
normal Team `AppState` surface-free. In particular, do not make browser paths,
recipient Team membership, or a docking ticket create a Files root or Git
inventory/worktree capability.

## File + immutable Review distinct-Actor HTTP lifecycle — 2026-11-14

The source-composition and HTTP lifecycle boundary is now implemented without
widening normal Team browser authority.

### Trusted source composition

- existing `team::serve` is unchanged: Team participants still have no
  `AppState.surfaces` and no Files/Git browser catalog;
- a new opt-in Rust embedding API,
  `team::serve_with_delegated_surfaces`, accepts explicit
  `DelegatedSurfaceSource { participant_id, options }` values constructed by the
  trusted host;
- each configured source becomes a separate Team-entry-owned
  `Arc<SurfaceHost>`, **not** the participant's normal `AppState.surfaces`;
- the configured Actor is overwritten with the authenticated participant Actor,
  `allow_terminals` is forcibly false and Sites are cleared before the host is
  created. Team shutdown explicitly shuts these retained hosts down;
- `/api/team/me` exposes only the boolean `delegated_surface_views`; its ordinary
  `surfaces` field remains false. Participants without an explicit source report
  `delegated_surface_views=false`;
- `/api/workspace` therefore remains `{enabled:false}` for both granting owner
  and recipient, and `/api/workspace/action` remains forbidden. The recipient
  never receives a Files root, Git inventory/context, terminal or Site grant.

### Shared lifecycle

The existing Team delegated registry now carries either an exact Agent branch or
an exact retained Surface target. File/Review did **not** get a second ticket
state machine. `/api/dock/surface/*` reuses the same bounded:

`register -> prepare -> receive -> read -> accept -> confirm -> heartbeat -> release`

plus `status`, prepared-only `cancel`, and monotonic `revoke` semantics already
used by Agent delegation.

- windows remain bound to exact authenticated participant/principal plus the
  authoritative Team Store/browser epoch;
- Surface prepare accepts only File `{source, scope, owner_epoch, path}` or
  Review `{source, scope, owner_epoch, immutable review_id}` and an exact
  recipient. File/Review fields are mutually exclusive and strictly bounded;
- prepare performs an actual exact owner-side retained read **before issuing the
  grant**. Protected bytes are discarded; only the capability binding/ticket is
  returned. A forged/evicted Review ID therefore cannot acquire a grant;
- the resulting delegated grant remains exact recipient + source/scope/resource
  + Surface owner epoch + owner/recipient security epochs + monotonic generation;
- receive returns only the resource descriptor and grant generation, never File
  text or Review/Diff bytes;
- recipient `read` is owner-mediated through the explicit retained SurfaceHost,
  revalidates parent/grant before and after the blocking read, then revalidates
  again while holding the grant fence through final ticket-generation check and
  response construction;
- Accept still requires a successful read from the current grant generation;
  receiver ACK is liveness only and cannot extend resource authority;
- Agent and Surface route families are resource-kind fenced. A valid Surface
  ticket replayed through `/api/dock/agent/*` is denied (and vice versa);
- source Release changes only presentation ownership. The exact delegated read
  remains valid until explicit revoke/expiry, matching the established Agent and
  same-owner docking semantics;
- recipient logout continues to revoke incoming grants through the shared
  owner-side grant registry; Team owner expiry/shutdown likewise fences all
  delegated publication.

### HTTP/race verification

New real authenticated Team tests use distinct Alice/Bob cookies/Actors and a
private synthetic `SurfaceHost` retained only by Alice:

- File lifecycle: Alice has `delegated_surface_views=true` while ordinary
  Workspace stays disabled for both users; Bob cannot receive as Alice, cannot
  Accept before a read, cannot replay the ticket through Agent routes, then can
  read only `sample.rs`, Accept/Confirm/Heartbeat/Release, while both private
  Agents/Sessions remain unchanged and provider calls remain zero;
- held File race: after the owner read completes but before publication, explicit
  owner Revoke causes the held response and all later reads to return forbidden;
- Review lifecycle: Alice first retains a real immutable synthetic Git Review;
  Bob receives/read it by exact ID while `/api/workspace/action` Git inventory is
  still forbidden. A forged Review ID is denied during prepare before a grant is
  issued;
- the configured delegated host is verified to have terminals disabled and an
  empty Site list even when the trusted input options requested both.

Validation at this checkpoint:

- focused delegated/default slice: **10/10 passed** (new File/Review HTTP tests,
  Agent delegation regressions, and lower-level delegated Surface grants);
- `AW_GIT=$(command -v git) cargo test --locked -p aw-cli --lib web::team::tests`
  — **47 passed / 1 intentional ignored**;
- `AW_GIT=$(command -v git) cargo test --locked -p aw-cli --lib docking` —
  **25/25 passed**;
- `cargo clippy --locked -p aw-cli --lib -- -D warnings` — passed;
- `cargo check --locked -p aw-cli --no-default-features --features team --lib`
  — passed with only the pre-existing `bind_execution_owner` warning;
- reduced Team delegated Agent tests — **4/4 passed**;
- reduced Web grant registry — **8/8 passed**;
- formatting and `git diff --check` — passed.

Still intentionally pending for File/Review: exposing the trusted source in Team
CLI configuration (the existing CLI path still calls plain `team::serve`), Solid
document send/receive integration and exact-grant cache scrubbing, presentation
state transfer on the distinct-Actor route, and real two-profile Chrome
acceptance. Those are the next coherent product boundary; do not infer them from
the backend HTTP proof.
