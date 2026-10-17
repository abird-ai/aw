# Browser workspace

## Read-only Agent tabs

The shipping Solid shell can open an explicit **Read-only view** beside a saved
Session branch. Selecting the branch itself still activates the current Agent
through the existing idle, draft and reconciliation guards. A read-only tab
never creates or activates a Session, calls a model/tool, or changes the current
Agent, composer draft or caret.

Private unsent drafts remain bound to their owner, Session and branch. An
external target change preserves text and caret while blocking Send and private
draft publication. **Use draft here** explicitly binds the draft to the current
target without sending or publishing it. A confirmed local branch-open already
supplies that consent for unchanged text and the exact target; clearing the
text resets the binding. Read-only tab selection never retargets the draft.

Each tab names an exact Store, owner lifetime, Session and branch. Titles are
labels. The owner retains one SQLite Store for unsaved browsing and subsequent
Session creation; a read does not reopen State or initialize persistence.
Authenticated requests independently check the trusted actor's current access
and exact branch identity. Native and browser readers share the public transcript
projection, which removes invocation receipts and provider-private context.

Tabs are memory-only, with independent Find, viewport and selection. At most
eight tabs retain at most 4 MiB each and 8 MiB together. Refresh rechecks access
even when the head is unchanged; denial, logout, source replacement and expiry
clear protected content and derived state. Closed tabs reject delayed replies.
Ordinary refresh runs every five seconds, with freshness bounded to fifteen
seconds from request start. Browser paths and cached descriptors grant no local
folder or execution authority. Files/Git containers are described below; docking
remains in the [workspace plan](../plans/workspace-views/TODO.md).

## Independent Files/Git views

Files and Git keep the same Explorer categories. Their view selector and
**New Files view** / **New Git view** controls open independent navigators;
**Close view and its documents** closes only that container's browser state.
There are at most eight Files/Git containers and 32 total document tabs,
including the permanent Agent and its read-only tabs. Limits fail visibly and
preserve already-open views. These views are memory-only; shared layout and
**cross-window** docking remain later work.

Files stays under the explicitly granted startup directory. Git navigators may
independently select existing verified worktrees through opaque inventory
handles. Selecting a Git worktree changes that navigator only; it gives no
Files-root grant and does not retarget another view or the Agent. Browser paths
and display labels cannot open another server folder.

Each navigator retains its own tree/list, query, expansion and Git mode/context.
Each File/Diff document retains its own Find, viewport and selection when
switching tabs. The exact read is an explicit snapshot; **Refresh document**
revalidates that same binding. An unchanged refresh retains the editor state.
A delayed open fills only its original pending tab and cannot steal later focus.
Diffs stay with their original immutable Review after another worktree is
selected. Read-only documents never gain a composer.

When two or more compatible Files/Git views are open, the active File/Diff
source bar offers a compact **Move document to another view** selector. This
is *same-window* reparenting: the existing tab/document ID, original
source/scope, immutable Review, CodeMirror Find/scroll/selection, current
Agent/composer, and any exact in-flight read are retained. It starts no new
read or capability, and it refuses a mismatched navigator, a missing/denied
view or a duplicate of the same resource already in the destination. Closing
the origin no longer closes a moved tab; closing its destination retires
the tab and fences late replies. With only one compatible view the selector
is hidden to keep the source bar quiet. This does **not** move resources
between browser windows/processes or give browser labels/pathnames authority.

The owner retains the original Files capability and directory identity rather
than reopening its pathname. Every scoped read carries exact source and scope
IDs, with independent inventory IDs for Git opens. Closed views reject delayed
replies; logout and source replacement clear all protected view state. Confirmed
scope denial clears its documents, navigator data and derived editor state;
one unreadable file affects only that resource. Four physically outstanding
Files/Git reads, a 4 MiB reply limit and an 8 MiB Files/Git snapshot cache bound
work and retention. Hidden views retain editor state; closed or denied editors explicitly
scrub it before destruction.

## Current Phase 62/63 Solid Agent parity and rich rendering

The active root remains the compiled SolidJS application. Phase 62 carries the server-authored loaded `[Context]` / `[Skills]` resource projection and typed status/session information into the Solid Agent surface without letting browser state invent provider/tool/filesystem authority. Browser-only projection metadata is never copied into prompt submission.

Phase 63 keeps Markdown rendering safe while adding tables, nested/task lists and strikethrough. Markdown image syntax is deliberately rendered as an **inert media reference** with an explicit user-clickable safe link rather than an `<img>` element, so rendering never auto-fetches remote media. Raw HTML remains disabled and unsafe destinations remain literal/non-clickable.

The post-Phase59 CodeMirror generation/document/theme fencing remains authoritative. A newly opened file starts at the top left; switching existing scoped views preserves their independent editor state.

## Current post-Phase37 design-adaptation contract

For ordinary numeric-loopback **single-user `aw web`**, the product root is now a compiled **SolidJS** application rather than the historical hand-written three-region shell. The design intentionally combines the density/snappiness of Pi Web with a minimal ChatGPT-like conversation surface and Abird's own visual language:

- a narrow left Explorer, ordered **Sessions / Files / Git**;
- a **full command/search bar** across the top for files, sessions, open tabs and commands;
- a dedicated **document tab strip** immediately below it for Agent, files and diffs;
- a single primary Agent/content area and bottom floating composer;
- a mouse-draggable Explorer/content divider with the chosen width kept as presentation-local browser state;
- the official Abird **Round Trip** SVG and the canonical green/blue/orange palette;
- responsive mobile drawer behavior rather than permanently consuming horizontal space.

Sessions are a real searchable branch tree/list: each saved Session can expand to its durable branches/forks and selecting a branch changes the current view through the existing revision-checked API. Files is a lazy rooted tree/list with bounded server-side filename/path search; `.git`, build caches and dependency directories are hidden from the ordinary tree rather than overwhelming the explorer. Git exposes clean segmented Changes / Worktrees / Branches controls, foldable change groups, and a Tree/List switch; Changes Tree is a real collapsible path hierarchy inside each change group. Human worktree selection changes only the Git inspection context; it does not move the Agent, Files root, provider, tools or model context.

Local single-user web still does **not** require pairing and may start with **zero configured models**. Settings exposes guided provider/model setup for **ChatGPT subscription**, **Ollama Cloud**, custom OpenAI-compatible **Chat Completions**, and custom OpenAI-compatible **Responses API**. The active Solid Settings surface also edits the canonical extension-owned `ui.status_lines` model (add/remove/reorder rows, above/below/fullscreen placement, row items) plus **theme** (`System` / `Light` / `Dark`), performance/TPS, TTFT, waterfall, thinking/tool visibility, TUI mode, padding and autocomplete. `System` is the default and follows `prefers-color-scheme` live; explicit Light/Dark overrides are persisted through the same `[ui]` configuration and reflected immediately without remounting the application. Ordinary profile saves persist only secret-free metadata/environment-variable references and make no provider request. ChatGPT subscription setup is the explicit exception because the user deliberately starts an OAuth login: it uses the supported Sign in with ChatGPT dynamic-registration + PKCE flow, stores tokens only in the local SecretStore, lists models only after authorization, and activates the selected model only after explicit confirmation.

The ChatGPT subscription route uses the dedicated Responses adapter; it is never routed through the Chat Completions adapter. Requests are stateless (`store:false`) and provider-private reasoning remains private. Opaque encrypted continuation items may be retained only inside the same in-flight provider tool loop and are not rendered or journaled. Changing endpoint/model never silently projects the old model-private context into the new endpoint.

The Solid root consumes the existing authenticated server APIs; it does not move filesystem/provider authority into JavaScript. Browser fields still cannot choose arbitrary roots, provider secrets, actor/payer identities, executables, environment variables, or generic fetch targets. Team/shared browser modes retain their explicit authentication/pairing boundaries.

Browser first paint is theme-safe and deliberately minimal. The HTML loads same-origin `/theme-bootstrap.css` and `/theme-bootstrap.js` **before** the Vite application, under strict same-origin script policy and the response-local stylesheet nonce described below. The bootstrap applies a cached presentation-only theme hint immediately, reconciles it with `/api/settings` when that local settings endpoint is available, follows system appearance changes while `theme=system`, and keeps `#root` hidden behind a **logo-only transparent Round Trip mark** until the theme promise settles. The app then cross-fades in with reduced-motion respected. Theme state in `localStorage` is only a non-secret paint hint to avoid first-frame flicker; provider/OAuth credentials and authority never enter browser storage. CodeMirror/Shiki listens to the same effective-theme event so already-open code and diff views recolor without reload.

Each root HTML response receives a fresh OS-random 256-bit style nonce, shared only by that response's `aw-style-nonce` metadata and `style-src` policy. CodeMirror supplies it through `EditorView.cspNonce`; Shiki emits class rules into a nonced stylesheet using validated theme colors and supported font-style flags. Code text cannot supply arbitrary CSS. Responses remain `Cache-Control: no-store`; static assets and API responses receive no style nonce. `script-src 'self'` is unchanged, and this change adds no blanket inline-style or style-attribute permission. The existing explicit workspace-surfaces `style-src-attr 'unsafe-inline'` exception for calculated geometry remains scoped to that opt-in mode.

The historical Phase-6 multi-group/terminal/site/context-flow browser modules and typed backend APIs remain in the tree as compatibility/reference surfaces while migration into the simpler Solid shell continues. They are **not** loaded by the current root page and must not be mistaken for a second active frontend architecture.


### Post-Phase59 CodeMirror file-visibility hardening

The rooted browser `file` action was revalidated and already returns the complete UTF-8 text unchanged; the user-reported “line numbers only” failure was therefore a presentation/lifecycle bug in the active Solid CodeMirror surface, not a file-read/data-loss bug. `CodeFile` now gives `.cm-content` an explicit `var(--fg)` plain-text foreground, clears old Shiki decorations before async syntax work, generation-fences every highlight request, and applies a result only while the exact document and effective theme still match. A grammar/highlighter failure leaves readable undecorated text. When one mounted CodeMirror surface switches file identity, it scrolls to document start and explicitly resets horizontal scroll to zero, preventing a retained offset from leaving only the fixed line-number gutter visible. The tracked lazy `web/dist/chunk-code-surface.js` is rebuilt from this source.

## Enable only what the owner intends

```sh
aw web -C /absolute/checkout --provider-profile ollama --model-id INSTALLED_MODEL --web-workspace
# A second deliberate grant exposes unsandboxed owner-machine shell capability:
aw web -C /absolute/checkout --provider-profile llama --model-id SERVER_MODEL \
  --web-workspace --web-allow-terminals
# Add a known embeddable site; no arbitrary site URL is accepted from the browser:
aw web -C /absolute/checkout --provider-profile llama --model-id SERVER_MODEL \
  --web-workspace --web-site 'Docs=https://docs.example.org/'
```

These commands require a compiled aw. `--web-workspace` grants human Files reads under the explicit startup root and Git views of that checkout and its verified existing worktrees. It does **not** grant model tools. Browser terminal startup needs the additional owner flag and an interactive confirmation. The UI remains a numeric-loopback owner under its LocalWeb or paired/shared authentication boundary; neither this flag nor a reverse proxy supplies remote multi-user authentication.

Historical Phase-6 behavior required explicit **Open workspace**. Current local-single-user `aw web` mounts the workspace automatically; paired/shared modes may still use explicit capability-gated entry. No terminal or model is started by loading the shell. Surface SDK construction records trusted root/program/site capabilities but does not launch a process. Files/Git/session lists are fetched lazily by the mounted side panes and refreshed on demand; that human browsing still does not dispatch inference.

## Layout and usable content

The **current Solid root** deliberately uses one Explorer plus one primary content surface rather than reproducing the historical four-group desktop workbench. Browser geometry is presentation-local and responsive; it never selects another model/account/root. Agent refers to the **one current owner conversation/Session**; internal `chat` identifiers remain compatibility details. File and Diff tabs are read-only CodeMirror 6 surfaces, and syntax decoration is supplied by Shiki; the heavy editor/highlighter bundle is lazy-loaded only after a file/diff is opened so initial chat/workspace startup remains small. Diff uses CodeMirror MergeView over the exact cached review snapshot. Session/branch selection uses the existing idle/draft/reconciliation guards and is not a per-tab concurrent agent manager.

The older browser-local v1 reducer still models up to four content groups and 24 tab views for compatibility/reference use by retained advanced surface modules. That reducer is not the active Solid root layout. Native and browser layouts remain separate view-only schemas; no shared layout importer or cross-client layout synchronization is claimed, and the server does not receive a whole browser layout blob.

## Human file/Git reads

Files uses the explicit owner startup root, bounded read-only directory and UTF-8 file services. Navigating or reading does not advertise a model tool or inject source into model context. Symlink/traversal/hardlink restrictions are inherited from the Linux file implementation; this remains a trusted same-OS-user application, not a tenant sandbox.

Git has exclusive **Changes / Worktrees / Branches** subviews and **List / Tree** changed-file projection. The browser sends opaque inventory handles for open operations, not arbitrary Git commands, shell arguments or filesystem roots. Staged/unstaged records of the same path keep different identities. Worktree selection changes human Git scope and the initial directory of a newly created terminal only. It does not move the agent, its Files root, tools, provider or account.

The web adapter is **read-only for Git** in this slice. The native Phase 5 whole-file index update commands remain available, but are not exposed through the new HTTP action schema. Branch opening compares an object; it does not checkout. No browser fetch/push/commit/checkout/worktree creation/deletion occurs.

A diff is a cached exact review snapshot, not a live working-file promise. The current Solid CodeMirror surface displays it read-only. The retained historical workspace module and API provide row comments that append labelled feedback to the **unsent** composer, with conversation-target checks and cross-root feedback blocked. The user submits explicitly. Review caches cap at eight; inventories and source bytes retain the Git SDK's bounds. Expired handles require refresh and inspection.

## Terminal behavior and limits

The native `aw-terminal` service owns each real PTY. The browser receives bounded serialized screen cells/styles, not raw terminal escape sequences. The HTTP action worker pool and per-host resource mutex keep blocking file/Git/PTY work off the async request/render path. Ordinary terminal input goes to the shell; it is not a prompt or an agent cancellation.

1. **New terminal** requests a confirmed launch with an idempotency ID. The host grants the known root/program; the browser cannot supply an executable or environment. A failed/ambiguous launch is not automatically retried. Check Existing terminals before another launch.
2. **Claim control** acquires a 15-second memory-scoped lease for a browser-client ID. Other tabs can observe but must wait/release before typing or resizing. This is cooperative ownership between views of one authenticated principal, not multi-user identity proof.
3. Input is sequence-numbered. The server accepts the next sequence or the exact last duplicate, and rejects a changed/old/skipped packet. The browser sends one in-flight packet, preserves an uncertain packet and offers **Retry exact input** or an explicit **Discard unconfirmed input after inspection**. Discard removes the local pending packet, not already queued/executed effects; reclaim control before typing. Acknowledgement means queued, not that a command completed or its effects were rolled back.
4. **Fit active group** explicitly resizes under the control lease. Moving/hiding a group does not force a zero-size resize or spawn another process. **Older output / Live screen** read bounded scrollback. The view does not reconstruct a shell from log lines.
5. **Close view** hides it. Existing terminals reopen the same resource. **Terminate** is a separate confirmed action. Existing terminal entries expose Forget only after the resource exits, freeing its retained slot without relaunch. Browser disconnect or Agent only leaves the owner's shell alive; logout/authority expiry and owner shutdown request termination. Escaped/disowned processes can still require manual inspection.

The host retains at most four terminals, 64 launch receipts and 1,000 scrollback rows; web dimensions are 4–160 columns by 2–60 rows. Input queue and batch sizes are bounded. A missing/expired lease requires a new claim; unconfirmed old input must not be replayed under a new lease. Terminal control state/receipts are not durable across owner restart.

Visible terminal views poll canonical screen state at a bounded cadence (about 120ms), shared by views of the same terminal. No terminal poll timer runs when none is visible or the browser document is hidden. **The owner continues draining hidden PTYs**. Screen repaint is keyed to version, dimensions and scrollback, not every poll. This is deliberately a small screen-projection renderer, not xterm.js or a claim of complete terminal protocol/font/IME parity.

**A terminal is unsandboxed owner-OS authority**, regardless of model tools being off. Sanitizing environment/startup files does not prevent access to other files, sockets, network or credentials allowed to that OS user. Treat browser terminal grants with the same seriousness as a local shell; do not expose this development owner to remote users.

## Websites are bounded external content

The owner registers at most eight label/HTTPS-address pairs through `--web-site`. The initial profile rejects direct IP/localhost and common local suffixes, credentials, query strings, fragments and control characters. It permits dotted DNS names, but **does not prove their DNS resolution is public**. The trusted owner must choose appropriate destinations. No server HTTP-fetch proxy or site-control bridge exists.

Each page uses `sandbox="allow-scripts"`, without same-origin, forms, popups, top navigation, download, clipboard, camera or microphone permission. A separate aw-owned toolbar supplies the opened address, Reload, Suspend/Unload and Open in browser with `noopener noreferrer`. The CSP permits only configured frame origins; native app connections remain same-origin. Browser extension bypasses or stripping frame protection are not offered.

Many sites forbid embedding or require cookies/sandbox permissions this profile does not offer. Aw cannot reliably read a cross-origin DOM, current URL/history or detailed load failure. The status says **unverified page state**, and external-open remains available. Reload restores the configured opened address, not an unseen navigation URL. Rich Back/Forward, DOM extraction, screenshots and agent control are not implemented.

The frame DOM stays in a stable surface layer while group rectangles/tab membership change; moves do not remove/reinsert the frame or alter its source. CSS hiding keeps a live frame alive and can still cost CPU/network. Suspend/Unload and closing ask before discarding unknown form state. All pages, editor buffers and layouts are local memory in this slice; reconnect/locking disposes protected workspace views. No cookie, DOM, page history or signed URL is synchronized to teammates or a model.

This is a configured external Website surface, **not a development preview proxy/tunnel**. A local/remote app preview origin, runner mapping and mixed-content policy need a later explicit integration. A frame's localhost would mean the browser device, not the server; no attempt is made to make this silently work by proxying secrets.

## Activity and session metrics

Activity Log is a bounded projection of recent authorized turns/tool records, not a full durable audit tail. Filter and pause-follow change only the view. Session Metrics displays observed completed/failed/cancelled turns, active run count and observed invocation count from the **current displayed ancestry**. It is not a global unique billing ledger, cost estimate, infrastructure metric feed or synthetic progress percentage.

Selecting these views performs no model call. External telemetry datasource adapters, chart dashboards, autonomous pipelines and metrics alerts are not part of this phase.

## Keyboard, accessibility and account continuity

The Solid root does not globally hijack browser Cmd/Ctrl shortcuts. In the composer, Enter submits and Shift+Enter inserts a newline; IME composition is not treated as submit. Explorer controls are ordinary accessible buttons/selects and the mobile layout uses a drawer. Advanced embedded-terminal keyboard ownership rules remain those of the retained typed terminal surface when that surface is used; the Solid root does not invent a second terminal protocol.

The same submitter account selection and SDK journal are retained. There is no browser provider-token cache, secret-upload field, localStorage vault, or client-selected actor/payer authority. The only browser-triggered provider login is the explicit local-single-user Sign in with ChatGPT flow; OAuth access/refresh/ID tokens stay in the host SecretStore and only secret-free setup status/model metadata reaches the page. Shared layout/text/website/PTY authority is not inferred from having the same visible conversation. [Credential contract](CREDENTIAL-PLACEMENT.md).

## Validation

Run the shared product gate in [Build status](BUILD-STATUS.md), plus the
production Solid build when changing browser source. The six Node suites cover
pure state, lifetime and rendering rules. Rust HTTP tests exercise the actual
owner against SQLite State, including transport boundaries, exact authority,
privacy, revocation and no execution effects. Real-browser acceptance must use
the shipping compiled assets and Rust owner; mocked API transport does not
establish that result. Commands, exact commits and completed acceptance belong
in the [plan TODO](../plans/workspace-views/TODO.md).

Native Windows/macOS execution, live provider/OAuth use, signing and publication
remain separate gates. [SDK](SURFACES.md) · [Web protocol](WEB-PROTOCOL.md).

## Phase 7 shared owner note

The same packaged browser workspace can also be launched through the explicit shared-owner/server modes described by the local-owner protocol. Their peer-validated native clients share one Agent/current execution view rather than creating a second SQLite owner. Those shared/paired modes retain their own authentication semantics; the separate ordinary local-single-user `aw web` path uses `LocalWeb` no-pair loopback authority as described above. Provider secret values remain outside browser storage/config.

Browser logout revokes its access and terminates its browser surface host, but leaves the shared agent/native listener running. Closing a view does not shut down the owner. There is no shared draft or independent per-tab agent introduced by a second presentation. Existing website/terminal/surface trust limits remain.

[Local owner contract](LOCAL-OWNER.md) · [Local protocol](LOCAL-PROTOCOL.md).

## Phase 10 context wizard and revoke-aware display

The ordinary conversation adds a lazy **Context flow** action, also usable when its mounted conversation surface appears in any workspace group. It selects source sessions/branches and a compatible owned destination, prepares exact evidence, chooses a permitted summarizer account or manual candidate, previews/edits, applies one of five modes, and recovers exact operation/application receipts. Source sharing is an explicit owner action to a known participant with an account-ID disclosure list.

The feature starts no provider on open/hover/selection. A candidate edit must be saved as a new artifact before apply. Stale targets preserve the candidate. Permission revocation clears protected conversation, candidate/picker and mounted cached views; explicit New private session uses only minimal authorized recovery metadata. This does not retroactively erase prior downloads or prove a hostile client cooperates. Actual packaged JS was exercised with fake transport; Rust service and policy assurance require the host harness. [Implementation details](CONTEXT-FLOW-IMPLEMENTATION.md).
