import { createEffect, createMemo, createSignal, For, lazy, on, onCleanup, onMount, Show, untrack } from "solid-js";
import { render } from "solid-js/web";
import { api, workspaceAction } from "./api.js";
import { renderMarkdown } from "./markdown.js";
import { Icon } from "./icons.jsx";
import { AgentViewPanel } from "./agent-view-panel.jsx";
import { createAgentViews } from "./agent-views.js";
import { createWorkspaceViews } from "./workspace-views.js";
import { createDocumentDocking, takeDocumentDockTicket } from "./document-docking.js";
import { applyThemePreference, finishBoot } from "./theme.js";

const CodeFile = lazy(() =>
  import("./code-surface.jsx").then(module => ({ default: module.CodeFile }))
);
const CodeDiff = lazy(() =>
  import("./code-surface.jsx").then(module => ({ default: module.CodeDiff }))
);
import { acceptDelta, acceptSnapshot, accountChoices, canSend, draftTargetChanged, isSubmitKey, needsDraftPublicationConsent, submission } from "../model.js";
import "./styles.css";

const MARK = "/abird-round-trip-mark.svg";

function Mark() {
  return <img src={MARK} alt="" aria-hidden="true" />;
}

function toolPresentation(tool) {
  const status = String(tool?.status || "unknown").toLowerCase();
  const failedStatus = ["failed", "denied", "interrupted"].includes(status);
  if (failedStatus || tool?.output?.is_error === true) {
    return { tone: "failure", symbol: "×", status: failedStatus ? status : "failed" };
  }
  if (status === "succeeded") return { tone: "success", symbol: "✓", status };
  return { tone: "neutral", symbol: status === "running" ? "·" : "?", status };
}

function Message(props) {
  const assistant = () => props.role === "assistant";
  return (
    <article class={"message " + props.role} aria-label={assistant() ? "Assistant" : "Your prompt"}>
      <div class="message-head">
        <strong>{assistant() ? "aw" : "You"}</strong>
        <Show when={props.outcome}><span>{props.outcome}</span></Show>
      </div>
      <Show when={assistant()} fallback={<div class="message-body">{props.text}</div>}>
        <Show when={props.showThinking && props.outcome === "streaming" && !String(props.text || "").trim()}>
          <div class="thinking-row">Thinking…</div>
        </Show>
        <div class="message-body" innerHTML={renderMarkdown(props.text || "")} />
      </Show>
      <Show when={props.tools?.length}>
        <div class="tool-stack">
          <For each={props.tools}>
            {tool => {
              const state = () => toolPresentation(tool);
              return (
                <div class={"tool-row " + state().tone}>
                  <span class="tool-symbol" aria-hidden="true">{state().symbol}</span>
                  <span class="tool-label">
                    <Show when={props.showTools !== false} fallback={<>tool {tool.call?.name || "tool"} · {state().status} · hidden</>}>
                      {tool.call?.name || "tool"}<span class="tool-status"> · {state().status}</span>
                    </Show>
                  </span>
                </div>
              );
            }}
          </For>
        </div>
      </Show>
    </article>
  );
}

function ContextCard(props) {
  return (
    <details class="context-card">
      <summary>
        {props.block.kind === "compaction" ? "Compacted" : "Imported"} context ·{" "}
        {(props.block.sources || []).length} sources
      </summary>
      <div class="message-body" innerHTML={renderMarkdown(props.block.text || "")} />
    </details>
  );
}


function ResourceHeader(props) {
  const context = () => [
    ...(props.snapshot?.resources?.context || []),
    ...((props.snapshot?.context_blocks || []).map(block => {
      const count = (block.sources || []).length;
      return `${block.kind || "context"} · artifact ${block.artifact_id || "unknown"} · ${count} source${count === 1 ? "" : "s"}`;
    })),
  ];
  const skills = () => props.snapshot?.resources?.skills || [];
  return (
    <section class="agent-resources" aria-label="Loaded Agent resources">
      <div class="resource-section"><strong>Context</strong><For each={context()} fallback={<span class="muted">none loaded</span>}>{item => <span>{item}</span>}</For></div>
      <div class="resource-section"><strong>Skills</strong><For each={skills()} fallback={<span class="muted">none loaded</span>}>{item => <span>{item}</span>}</For></div>
    </section>
  );
}

function statusValue(item, snapshot) {
  if (!snapshot) return "";
  switch (item) {
    case "model": return snapshot.model || "";
    case "profile": return snapshot.profile || "";
    case "agent": return snapshot.saved?.title || "Agent";
    case "branch": return snapshot.saved?.branch_name || "";
    case "tools": return snapshot.tools?.length ? `${snapshot.tools.length} tools` : "tools off";
    case "session": return snapshot.saved?.session_id || "";
    case "state": return snapshot.pending ? "running" : snapshot.needs_reconciliation ? "inspect" : "ready";
    default: return "";
  }
}

function AgentStatus(props) {
  const items = () => {
    const seen = new Set();
    return (props.ui?.status_lines || [])
      .flatMap(line => [...(line.items || []), ...(line.right_items || [])])
      .filter(item => {
        if (seen.has(item) || !statusValue(item, props.snapshot)) return false;
        seen.add(item);
        return true;
      });
  };
  return <Show when={items().length}><div class="browser-agent-status"><For each={items()}>{item => <span>{statusValue(item, props.snapshot)}</span>}</For></div></Show>;
}

function basename(path = "") {
  const parts = String(path).split("/").filter(Boolean);
  return parts.at(-1) || path || "";
}

function dirname(path = "") {
  const parts = String(path).split("/").filter(Boolean);
  return parts.length > 1 ? parts.slice(0, -1).join("/") : "";
}

function isTreeNoise(path = "") {
  return String(path).split("/").some(part =>
    [".git", "target", "node_modules", ".direnv", ".next", ".cache", "__pycache__"].includes(part)
  );
}

function ViewToggle(props) {
  return (
    <div class="view-toggle" role="group" aria-label="View style">
      <button
        class={props.value === "tree" ? "active" : ""}
        onClick={() => props.onChange("tree")}
        title="Tree view"
        aria-label="Tree view"
      >
        <Icon name="tree" size={13} />
      </button>
      <button
        class={props.value === "list" ? "active" : ""}
        onClick={() => props.onChange("list")}
        title="List view"
        aria-label="List view"
      >
        <Icon name="list" size={13} />
      </button>
    </div>
  );
}

function NavigatorSearch(props) {
  return (
    <label class="navigator-search">
      <Icon name="search" size={13} />
      <input
        value={props.value}
        onInput={event => props.onInput(event.currentTarget.value)}
        placeholder={props.placeholder || "Search"}
        aria-label={props.placeholder || "Search"}
      />
      <Show when={props.value}>
        <button
          type="button"
          class="search-clear"
          onClick={() => props.onInput("")}
          aria-label="Clear search"
        >
          <Icon name="close" size={12} />
        </button>
      </Show>
    </label>
  );
}

function CommandBar(props) {
  const categories = [
    [">", "Actions"], ["!", "Windows"], ["@", "Files"], ["#", "Sessions"],
    ["$", "Terminals"], ["/", "Commands"], ["&", "Agents"],
  ];
  const [query, setQuery] = createSignal("");
  const [open, setOpen] = createSignal(false);
  const [files, setFiles] = createSignal({ state: "idle", entries: [] });
  // undefined awaits a new nonempty query's first match; null is deliberately
  // unarmed (blank query or a removed target); a string pins an actual row.
  const [selectedKey, setSelectedKey] = createSignal(null);
  let input;
  let dialog;
  let launcher;
  let invokingFocus;
  let timer;
  let request = {};
  let searchAbort;
  let fileSearchContainer = null;

  const parsed = createMemo(() => {
    const value = query().trimStart();
    const category = categories.find(([prefix]) => value.startsWith(prefix))?.[0] || "";
    return { category, body: (category ? value.slice(1) : value).trimStart() };
  });

  const commands = () => [
    { key: "new", label: "New Agent", meta: "Action", icon: "plus", target: "chat", run: props.newChat },
    { key: "chat", label: "Go to Agent", meta: "View", icon: "chat", target: "chat", run: props.chat },
    { key: "sessions", label: "Show Sessions", meta: "Explorer", icon: "tree", target: "nav", run: () => props.nav("sessions") },
    ...(props.workspaceEnabled ? [
      { key: "files", label: "Show Files", meta: "Explorer", icon: "file", target: "nav", run: () => props.nav("files") },
      { key: "git", label: "Show Git", meta: "Explorer", icon: "git", target: "nav", run: () => props.nav("git") },
    ] : []),
    { key: "settings", label: "Open Settings", meta: "Action", icon: "settings", target: "settings", run: props.settings },
  ];

  createEffect(() => {
    const { category, body } = parsed();
    const value = body.trim();
    props.workspaceToken;
    clearTimeout(timer);
    searchAbort?.abort();
    const token = request = {};
    if (!open() || !props.workspaceEnabled || value.length < 2 || (category && category !== "@")) {
      setFiles({ state: "idle", entries: [] });
      return;
    }
    try { fileSearchContainer ||= untrack(() => props.fileSearchContainer()); }
    catch (error) { setFiles({ state: "error", entries: [], error: error.message }); return; }
    searchAbort = new AbortController();
    const signal = searchAbort.signal;
    setFiles({ state: "loading", entries: [] });
    timer = setTimeout(async () => {
      try {
        const result = await props.searchFiles(fileSearchContainer, value, { signal });
        if (token === request) setFiles({ state: "ready", entries: result.entries || [] });
      } catch (error) {
        if (token === request) setFiles({ state: "error", entries: [], error: error.message });
      }
    }, 120);
  });

  const items = createMemo(() => {
    const { category, body } = parsed();
    const needle = body.trim().toLowerCase();
    const match = value => !needle || String(value).toLowerCase().includes(needle);
    const includes = prefix => !category || category === prefix;
    const result = includes(">") ? commands().filter(item => match(item.label)) : [];

    if (includes("!")) {
      for (const tab of props.tabs || []) {
        if (match(tab.title) || match(tab.path || "")) {
          result.push({
            key: "tab:" + tab.id,
            label: tab.title,
            meta: "Window · " + (tab.path || "Agent"),
            icon: tab.kind === "chat" || tab.kind === "agent-view" ? "chat" : tab.kind === "review" ? "git" : "file",
            target: tab.id === "chat" ? "chat" : "tab",
            run: () => props.activateTab(tab.id),
          });
        }
      }
    }
    if (includes("#")) {
      for (const session of props.sessions || []) {
        for (const branch of session.branches || []) {
          if (!match(session.title) && !match(session.directory) && !match(branch.name)) continue;
          result.push({
            key: "session:" + session.id + ":" + branch.id,
            label: session.title || "Untitled",
            meta: "Session · " + branch.name + (session.shared ? " · shared" : ""),
            icon: "chat",
            target: "chat",
            run: () => props.openSession(session.id, branch.name, Boolean(session.shared)),
          });
        }
      }
    }
    if (includes("&") && props.agent && match(props.agent.title)) {
      result.push({ key: "agent:chat", label: props.agent.title, meta: "Current Agent",
        icon: "chat", target: "chat", run: props.chat });
    }
    if (includes("@")) {
      for (const file of files().entries) {
        if (file.kind === "file") {
          result.push({
            key: "file:" + file.containerId + ":" + file.path,
            label: basename(file.path),
            meta: "File · " + (dirname(file.path) || "."),
            icon: "file",
            target: "tab",
            run: () => props.openFile(file.containerId, file.path),
          });
        }
      }
    }
    return result.slice(0, 60);
  });

  // A blank palette is for browsing. Opening it must never arm an effectful
  // first row; arrows or a click deliberately select one.
  createEffect(on(query, () => setSelectedKey(parsed().body.trim() ? undefined : null)));
  createEffect(() => {
    const rows = items();
    const key = selectedKey();
    if (key === undefined && rows.length) setSelectedKey(rows[0].key);
    else if (typeof key === "string" && !rows.some(item => item.key === key)) setSelectedKey(null);
  });
  const selected = createMemo(() => {
    const index = items().findIndex(item => item.key === selectedKey());
    return index < 0 ? null : index;
  });
  createEffect(() => {
    const index = selected();
    if (index !== null) queueMicrotask(() =>
      dialog?.querySelector(`#command-option-${index}`)?.scrollIntoView({ block: "nearest" }));
  });

  const status = createMemo(() => {
    const { category, body } = parsed();
    if (category === "$") return "Terminals are available in the native workspace.";
    if (category === "/") return "Slash commands are available in the native app.";
    if (category === "@" && !props.workspaceEnabled) return "File browsing is unavailable for this owner.";
    if (!category || category === "@") {
      if (files().state === "loading") return "Searching files…";
      if (files().state === "error") return files().error || "File search unavailable. Change the query to try again.";
      if (category === "@" && body.trim().length < 2) return "Type at least two characters to search files.";
    }
    return items().length ? "" : "No matches";
  });

  function show() {
    if (open()) { input?.focus(); return; }
    const element = document.activeElement;
    invokingFocus = { element };
    if (element instanceof HTMLTextAreaElement || element instanceof HTMLInputElement) {
      invokingFocus.selection = [element.selectionStart, element.selectionEnd, element.selectionDirection];
    }
    setQuery("");
    fileSearchContainer = null;
    setSelectedKey(null);
    setOpen(true);
  }

  function close() {
    clearTimeout(timer);
    request = {};
    searchAbort?.abort();
    dialog?.close();
    setOpen(false);
    const { element, selection } = invokingFocus || {};
    if (element?.isConnected) {
      element.focus({ preventScroll: true });
      if (selection && selection[0] !== null) element.setSelectionRange(...selection);
    } else launcher?.focus({ preventScroll: true });
  }

  async function choose(index = selected()) {
    if (index === null) return;
    const item = items()[index];
    if (!item) return;
    close();
    const origin = document.activeElement;
    const succeeded = await item.run();
    // A slow read must not take focus from a control the user chose meanwhile.
    const focusUnchanged = document.activeElement === origin ||
      (!origin?.isConnected && document.activeElement === document.body);
    if (succeeded !== false && !open() && focusUnchanged) props.focusSurface(item.target);
  }

  function category(prefix) {
    setQuery((parsed().category === prefix ? "" : prefix) + parsed().body);
    input?.focus();
  }

  function onKeyDown(event) {
    if (event.isComposing) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const length = items().length;
      const forward = event.key === "ArrowDown";
      const value = selected();
      const next = value === null ? (forward ? 0 : length - 1) :
        (value + (forward ? 1 : -1) + length) % length;
      setSelectedKey(length ? items()[next].key : null);
    } else if (event.key === "Enter") {
      event.preventDefault();
      void choose();
    }
  }

  function globalKey(event) {
    if (event.defaultPrevented || event.isComposing || event.altKey) return;
    const modifier = event.ctrlKey || event.metaKey;
    const key = event.key.toLowerCase();
    if ((key === "f1" && !modifier && !event.shiftKey) ||
        (modifier && ((key === "p" && event.shiftKey) || (key === "k" && !event.shiftKey)))) {
      // Settings and other modal inputs own their keyboard interaction.
      if (document.querySelector('[aria-modal="true"]') && !open()) return;
      event.preventDefault();
      show();
    }
  }

  onMount(() => window.addEventListener("keydown", globalKey));
  onCleanup(() => {
    clearTimeout(timer);
    request = {};
    searchAbort?.abort();
    window.removeEventListener("keydown", globalKey);
  });

  return (
    <>
      <button class="command-bar" ref={launcher} onClick={show} aria-haspopup="dialog" aria-expanded={open()} aria-label="Open Command Palette">
        <Icon name="search" size={14} /><span class="command-label">Search or run a command…</span><kbd>Ctrl K</kbd>
      </button>
      <Show when={open()}>
        <dialog class="command-palette" aria-modal="true" aria-labelledby="command-title"
          ref={element => {
            dialog = element;
            queueMicrotask(() => {
              if (!element.isConnected || !open()) return;
              element.showModal();
              input?.focus();
            });
          }}
          onCancel={event => { event.preventDefault(); close(); }}
          onKeyDown={event => {
            if (event.key !== "Tab" || event.ctrlKey || event.metaKey || event.altKey) return;
            const controls = [...event.currentTarget.querySelectorAll('button:not(:disabled), input:not(:disabled)')];
            const first = controls[0];
            const last = controls.at(-1);
            if ((event.shiftKey && event.target === first) || (!event.shiftKey && event.target === last)) {
              event.preventDefault();
              (event.shiftKey ? last : first)?.focus();
            }
          }}
          onClick={event => {
            if (event.target !== event.currentTarget) return;
            const bounds = event.currentTarget.getBoundingClientRect();
            if (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom) close();
          }}>
          <header class="command-palette-head"><h2 id="command-title">Command Palette</h2>
            <button class="icon-button" onClick={close} aria-label="Close Command Palette"><Icon name="close" size={14} /></button>
          </header>
          <div class="command-search"><Icon name="search" size={16} />
            <input ref={input} value={query()} onInput={event => setQuery(event.currentTarget.value)} onKeyDown={onKeyDown}
              role="combobox" aria-label="Search commands and resources" aria-autocomplete="list" aria-expanded="true"
              aria-controls="command-results" aria-activedescendant={selected() === null ? undefined : `command-option-${selected()}`}
              placeholder="Search, or type a category prefix…" autocomplete="off" spellcheck={false} />
          </div>
          <div class="command-categories" role="group" aria-label="Search category">
            <button aria-pressed={!parsed().category} onClick={() => { setQuery(parsed().body); input?.focus(); }}>All</button>
            <For each={categories}>{([prefix, label]) =>
              <button aria-pressed={parsed().category === prefix} onClick={() => category(prefix)}><span>{prefix}</span> {label}</button>
            }</For>
          </div>
          <div class="command-menu" id="command-results" role="listbox" aria-label="Command Palette results" aria-busy={files().state === "loading"}>
            <For each={items()}>{(item, index) => (
              <div id={`command-option-${index()}`} role="option" aria-selected={index() === selected()}
                class={index() === selected() ? "selected" : ""}
                onMouseDown={event => event.preventDefault()} onClick={() => void choose(index())}>
                <Icon name={item.icon} size={14} /><span class="command-label">{item.label}</span><span class="command-meta">{item.meta}</span>
              </div>
            )}</For>
          </div>
          <Show when={status()}><div class="command-empty" role="status">{status()}</div></Show>
          <footer class="command-footer"><span>↑ ↓ choose · Enter open · Esc cancel</span><span>F1 · Ctrl Shift P</span></footer>
        </dialog>
      </Show>
    </>
  );
}

function SessionNavigator(props) {
  const [expanded, setExpanded] = createSignal(new Set());
  const [view, setView] = createSignal("tree");
  const [query, setQuery] = createSignal("");

  const filtered = createMemo(() => {
    const needle = query().trim().toLowerCase();
    if (!needle) return props.sessions;
    return (props.sessions || []).filter(session =>
      String(session.title || "").toLowerCase().includes(needle)
      || String(session.directory || "").toLowerCase().includes(needle)
      || (session.branches || []).some(branch => String(branch.name || "").toLowerCase().includes(needle))
    );
  });

  createEffect(() => {
    const current = props.snapshot?.saved?.session_id;
    if (current && !expanded().has(current)) {
      const next = new Set(expanded());
      next.add(current);
      setExpanded(next);
    }
  });

  function toggle(id) {
    const next = new Set(expanded());
    next.has(id) ? next.delete(id) : next.add(id);
    setExpanded(next);
  }

  return (
    <div class="navigator">
      <div class="navigator-toolbar">
        <NavigatorSearch value={query()} onInput={setQuery} placeholder="Search sessions" />
        <ViewToggle value={view()} onChange={setView} />
      </div>
      <div class="navigator-caption">Sessions</div>
      <Show
        when={view() === "tree"}
        fallback={
          <For each={filtered()}>
            {session => (
              <For each={session.branches || []}>
                {branch => {
                  const active = () =>
                    props.snapshot?.saved?.session_id === session.id
                    && (props.snapshot?.saved?.branch_id === branch.id
                      || props.snapshot?.saved?.branch_name === branch.name);
                  return (
                    <div class="session-branch-actions">
                    <button
                      class={"nav-row " + (active() ? "active" : "")}
                      onClick={() => props.openSession(session.id, branch.name, Boolean(session.shared))}
                      title={session.directory}
                    >
                      <Icon name="chat" size={13} />
                      <span class="nav-row-main">
                        <span class="nav-row-name">{session.title || "Untitled"}</span>
                        <span class="nav-row-detail">{branch.name}{session.shared ? " · shared" : ""}</span>
                      </span>
                    </button>
                    <button class="mini-icon nav-readonly" type="button" disabled={!props.canInspect || session.shared}
                      onClick={() => props.openReadonly(session, branch)}
                      aria-label={`Open read-only view of ${session.title || "Untitled"} · ${branch.name}`} title={session.shared ? "Read-only views of shared entries are unavailable." : "Open read-only view"}>View</button>
                    </div>
                  );
                }}
              </For>
            )}
          </For>
        }
      >
        <For each={filtered()}>
          {session => {
            const activeSession = () => props.snapshot?.saved?.session_id === session.id;
            const branches = () => session.branches || [];
            const open = () => expanded().has(session.id) || Boolean(query().trim());
            return (
              <>
                <button
                  class={"nav-row session-row " + (activeSession() ? "active-parent" : "")}
                  onClick={() => toggle(session.id)}
                  title={session.directory}
                >
                  <span class="chevron">
                    <Icon name={open() ? "chevronDown" : "chevronRight"} size={12} />
                  </span>
                  <Icon name="chat" size={13} />
                  <span class="nav-row-name">{session.title || "Untitled"}</span>
                  <Show when={session.shared}><span class="nav-row-shared">shared</span></Show>
                  <span class="nav-row-count">{branches().length}</span>
                </button>
                <Show when={open()}>
                  <For each={branches()}>
                    {branch => {
                      const active = () =>
                        activeSession()
                        && (props.snapshot?.saved?.branch_id === branch.id
                          || props.snapshot?.saved?.branch_name === branch.name);
                      return (
                        <div class="session-branch-actions">
                        <button
                          class={"nav-row branch-row " + (active() ? "active" : "")}
                          onClick={() => props.openSession(session.id, branch.name, Boolean(session.shared))}
                        >
                          <span class="tree-guide" />
                          <Icon name="branch" size={12} />
                          <span class="nav-row-name">{branch.name}</span>
                        </button>
                        <button class="mini-icon nav-readonly" type="button" disabled={!props.canInspect || session.shared}
                          onClick={() => props.openReadonly(session, branch)}
                          aria-label={`Open read-only view of ${session.title || "Untitled"} · ${branch.name}`} title={session.shared ? "Read-only views of shared entries are unavailable." : "Open read-only view"}>View</button>
                        </div>
                      );
                    }}
                  </For>
                </Show>
              </>
            );
          }}
        </For>
      </Show>
      <Show when={!filtered().length}>
        <div class="navigator-empty">{query() ? "No matching sessions" : "No saved sessions yet."}</div>
      </Show>
    </div>
  );
}

function sameEntries(left, right) {
  if (left === right) return true;
  if (!left || !right) return false;
  const keys = Object.keys(left);
  return keys.length === Object.keys(right).length && keys.every(key => left[key] === right[key]);
}

function FilesNavigator(props) {
  const listings = createMemo(() => props.state?.listings || {}, undefined, { equals: sameEntries });
  const nodes = createMemo(() => new Map(Object.entries(listings()).map(([path, result]) => [path, result.entries || []])));
  const expandedPaths = createMemo(() => props.state?.navigator.expanded || [""], undefined, { equals: sameEntries });
  const expanded = createMemo(() => new Set(expandedPaths()));
  const search = createMemo(() => props.state?.search);
  const view = createMemo(() => props.state?.navigator.view || "tree");
  const query = createMemo(() => props.state?.navigator.query || "");
  const searching = () => Boolean(query().trim() && props.state?.pending);
  const [appliedQuery, setAppliedQuery] = createSignal("");
  let timer;
  let request = {};

  function patch(value) {
    try { props.controller.patchNavigator(props.id, value); }
    catch (error) { props.fail(error.message); }
  }
  const setView = value => patch({ view: value });
  const setQuery = value => patch({ query: value });
  const setExpanded = value => patch({ expanded: [...value] });

  async function load(path = "", force = false) {
    if (!force && nodes().has(path)) return;
    await props.controller.loadNavigator(props.id, { op: "files", path });
  }

  onMount(() => load(""));
  onCleanup(() => { request = {}; clearTimeout(timer); });

  createEffect(() => {
    const value = query().trim();
    clearTimeout(timer);
    const token = request = {};
    setAppliedQuery("");
    if (!value) return;
    timer = setTimeout(async () => {
      const accepted = await props.controller.loadNavigator(props.id, { op: "file_search", query: value, limit: 120 });
      if (accepted && token === request) setAppliedQuery(value);
    }, 120);
  });

  async function toggle(entry) {
    if (entry.kind !== "directory") {
      if (entry.kind === "file") props.openFile(props.id, entry.path);
      return;
    }
    if (view() === "list" || query()) {
      await load(entry.path);
      return;
    }
    const next = new Set(expanded());
    if (next.has(entry.path)) {
      next.delete(entry.path);
      setExpanded(next);
    } else {
      next.add(entry.path);
      setExpanded(next);
      await load(entry.path);
    }
  }

  function treeRows(path = "", depth = 0, out = []) {
    for (const entry of nodes().get(path) || []) {
      if (isTreeNoise(entry.path)) continue;
      out.push({ entry, depth });
      if (entry.kind === "directory" && expanded().has(entry.path)) {
        treeRows(entry.path, depth + 1, out);
      }
    }
    return out;
  }

  const visible = createMemo(() => {
    if (query().trim()) {
      return (appliedQuery() === query().trim() ? search()?.entries || [] : [])
        .map(entry => ({ entry, depth: 0, search: true }));
    }
    if (view() === "tree") return treeRows("", 0, []);
    const seen = new Set();
    const rows = [];
    for (const entries of nodes().values()) {
      for (const entry of entries) {
        if (seen.has(entry.path) || isTreeNoise(entry.path)) continue;
        seen.add(entry.path);
        rows.push({ entry, depth: 0, search: false });
      }
    }
    rows.sort((a, b) => {
      if (a.entry.kind !== b.entry.kind) return a.entry.kind === "directory" ? -1 : 1;
      return a.entry.path.localeCompare(b.entry.path);
    });
    return rows;
  });
  const visibleByPath = createMemo(() => new Map(visible().map(row => [row.entry.path, row])));

  async function refreshFiles() {
    const originalQuery = query().trim();
    const accepted = await props.controller.refreshNavigator(props.id);
    if (accepted && originalQuery && query().trim() === originalQuery) setAppliedQuery(originalQuery);
    if (!originalQuery && !query().trim()) {
      for (const path of expanded()) {
        if (!props.state || props.state.unavailable) break;
        if (path) await load(path, true);
      }
    }
  }

  return (
    <div class="navigator">
      <div class="navigator-toolbar">
        <NavigatorSearch value={query()} onInput={setQuery} placeholder="Search files" />
        <ViewToggle value={view()} onChange={setView} />
        <button class="mini-icon" onClick={refreshFiles} title="Refresh files" aria-label="Refresh files">
          <Icon name="refresh" size={13} />
        </button>
      </div>
      <div class="navigator-caption">
        <span title={props.state?.label}>Files · {props.state?.label || "Unavailable"}</span>
        <Show when={searching()}><span class="navigator-status">searching…</span></Show>
      </div>
      <Show when={props.state?.error}><div class="navigator-empty" role="status">{props.state.error}</div></Show>
      <div class="tree-list">
        <For each={visible().map(row => row.entry.path)}>
          {path => {
            const row = () => visibleByPath().get(path);
            const entry = () => row()?.entry || { kind: "unavailable", path: "", name: "" };
            const directory = () => entry().kind === "directory";
            const open = () => directory() && expanded().has(path);
            const selected = () => props.activePath === path;
            return (
              <button
                class={"nav-row file-row " + (selected() ? "active" : "") + (directory() ? " directory" : "")}
                style={{ "--depth": String(row()?.depth || 0) }}
                onClick={() => toggle(entry())}
                title={entry().path}
              >
                <span class="tree-indent" />
                <span class="chevron">
                  <Show when={directory()}>
                    <Icon name={open() ? "chevronDown" : "chevronRight"} size={11} />
                  </Show>
                </span>
                <Icon name={directory() ? "folder" : "file"} size={13} />
                <span class="nav-row-main">
                  <span class="nav-row-name">{row()?.search || view() === "list" ? basename(entry().path) : entry().name}</span>
                  <Show when={row()?.search || view() === "list"}>
                    <span class="nav-row-detail">{dirname(entry().path)}</span>
                  </Show>
                </span>
              </button>
            );
          }}
        </For>
      </div>
      <Show when={!visible().length && !searching()}>
        <div class="navigator-empty">{query() ? "No matching files" : "This folder is empty."}</div>
      </Show>
    </div>
  );
}

function GitNavigator(props) {
  const mode = createMemo(() => props.state?.navigator.mode || "changes");
  const context = createMemo(() => props.state?.scope_id || "");
  const emptyInventory = { items: [] };
  const inventory = createMemo(() => props.state?.inventory || emptyInventory);
  const collapsedPaths = createMemo(() => props.state?.navigator.collapsed || [], undefined, { equals: sameEntries });
  const collapsed = createMemo(() => new Set(collapsedPaths()));
  const view = createMemo(() => props.state?.navigator.view || "tree");
  const query = createMemo(() => props.state?.navigator.query || "");
  const loading = () => Boolean(props.state?.pending);
  const unavailable = createMemo(() => Boolean(props.state?.unavailable));
  function patch(value) {
    try { props.controller.patchNavigator(props.id, value); }
    catch (error) { props.fail(error.message); }
  }
  const setMode = value => patch({ mode: value });
  const setView = value => patch({ view: value });
  const setQuery = value => patch({ query: value });
  const setCollapsed = value => patch({ collapsed: [...value] });

  const load = (nextMode = mode()) => props.controller.loadNavigator(props.id, { op: "git_list", mode: nextMode });

  createEffect(() => {
    const nextMode = mode();
    context();
    if (!untrack(unavailable)) void load(nextMode);
  });

  const items = createMemo(() => {
    const needle = query().trim().toLowerCase();
    const source = inventory().items || [];
    if (!needle) return source;
    return source.filter(item =>
      String(item.label || "").toLowerCase().includes(needle)
      || String(item.path || "").toLowerCase().includes(needle)
      || String(item.key || "").toLowerCase().includes(needle)
      || String(item.group || "").toLowerCase().includes(needle)
    );
  });

  const groups = createMemo(() => {
    if (mode() !== "changes") return [{ name: "", rows: items() }];
    const order = ["Conflicts", "Staged", "Unstaged", "Untracked"];
    const map = new Map(order.map(name => [name, []]));
    for (const item of items()) {
      const name = item.group || "Other";
      if (!map.has(name)) map.set(name, []);
      map.get(name).push(item);
    }
    return [...map]
      .filter(([, rows]) => rows.length)
      .map(([name, rows]) => ({ name, rows }));
  });

  function open(item) {
    try {
      patch({ selected: item.id });
      const target = { inventory_id: inventory().inventory_id, item_id: item.id };
      if (mode() === "worktrees") void props.controller.selectWorktree(props.id, target);
      else props.openReview(props.id, target);
    } catch (error) {
      props.fail(error.message);
    }
  }

  function fold(group) {
    const next = new Set(collapsed());
    next.has(group) ? next.delete(group) : next.add(group);
    setCollapsed(next);
  }

  function changeTreeRows(group) {
    const root = { dirs: new Map(), files: [] };
    for (const item of group.rows) {
      const parts = String(item.path || item.label || "").split("/").filter(Boolean);
      if (!parts.length) continue;
      let node = root;
      for (const part of parts.slice(0, -1)) {
        if (!node.dirs.has(part)) node.dirs.set(part, { dirs: new Map(), files: [] });
        node = node.dirs.get(part);
      }
      node.files.push(item);
    }
    const out = [];
    function walk(node, prefix, depth) {
      for (const [name, child] of [...node.dirs.entries()].sort((a, b) => a[0].localeCompare(b[0]))) {
        const path = prefix ? prefix + "/" + name : name;
        const key = "dir:" + group.name + ":" + path;
        out.push({ kind: "folder", key, name, path, depth });
        if (!collapsed().has(key)) walk(child, path, depth + 1);
      }
      for (const item of node.files.slice().sort((a, b) => String(a.path || "").localeCompare(String(b.path || "")))) {
        out.push({ kind: "item", item, depth });
      }
    }
    walk(root, "", 0);
    return out;
  }

  function rowName(item) {
    if (mode() === "changes") {
      const path = item.path || item.label || "";
      return view() === "tree" ? basename(path) : path;
    }
    if (mode() === "worktrees") {
      return view() === "tree" ? (item.label || basename(item.path || "")) : (item.path || item.label || "");
    }
    if (mode() === "branches") {
      const label = item.label || item.key || "";
      return view() === "tree" ? label.replace(/^refs\/heads\//, "") : label;
    }
    return item.label || item.path || item.key || "";
  }

  function rowDetail(item) {
    if (mode() === "worktrees") {
      return view() === "tree" ? (item.path || "") : (item.label || item.status || "");
    }
    if (mode() === "branches") return item.status || "";
    return "";
  }

  return (
    <div class="navigator">
      <div class="git-mode-row">
        <div class="segmented git-modes" role="tablist" aria-label="Git view">
          <For each={[["changes", "Changes"], ["worktrees", "Worktrees"], ["branches", "Branches"]]}>
            {entry => (
              <button
                class={mode() === entry[0] ? "active" : ""}
                onClick={() => setMode(entry[0])}
                role="tab"
                aria-selected={mode() === entry[0]}
              >
                {entry[1]}
              </button>
            )}
          </For>
        </div>
        <ViewToggle value={view()} onChange={setView} />
      </div>
      <div class="navigator-toolbar git-toolbar">
        <NavigatorSearch value={query()} onInput={setQuery} placeholder={"Search " + mode()} />
        <button class="mini-icon" onClick={() => void props.controller.refreshNavigator(props.id)} title="Refresh Git" aria-label="Refresh Git">
          <Icon name="refresh" size={13} />
        </button>
      </div>
      <div class="navigator-caption git-context">
        <span class="branch-pill" title={props.state?.label}><Icon name="branch" size={11} />{inventory().branch || props.state?.label || "Unavailable"}</span>
        <Show when={loading()}><span class="navigator-status">refreshing…</span></Show>
      </div>
      <Show when={props.state?.error}><div class="navigator-empty" role="status">{props.state.error}</div></Show>
      <div class="tree-list">
        <For each={groups()}>
          {group => {
            const rows = createMemo(() => changeTreeRows(group));
            const byKey = createMemo(() => new Map(rows().map(row => [row.kind === "folder" ? row.key : row.item.id, row])));
            return (
            <>
              <Show when={group.name}>
                <button class="git-group" onClick={() => fold(group.name)}>
                  <Icon name={collapsed().has(group.name) ? "chevronRight" : "chevronDown"} size={11} />
                  <span>{group.name}</span>
                  <span class="nav-row-count">{group.rows.length}</span>
                </button>
              </Show>
              <Show when={!group.name || !collapsed().has(group.name)}>
                <Show
                  when={mode() === "changes" && view() === "tree"}
                  fallback={
                    <For each={group.rows}>
                      {item => (
                        <button class={"nav-row git-row " + (props.state?.navigator.selected === item.id ? "active" : "")} onClick={() => open(item)} title={item.path || item.key}>
                          <span class={"git-status status-" + (String(item.status || "").replace(/[^a-z0-9_-]+/gi, "_").toLowerCase() || "none")}>{item.status || "·"}</span>
                          <Icon name={mode() === "branches" ? "branch" : mode() === "worktrees" ? "folder" : "file"} size={13} />
                          <span class="nav-row-main">
                            <span class="nav-row-name">{rowName(item)}</span>
                            <Show when={rowDetail(item)}>
                              <span class="nav-row-detail">{rowDetail(item)}</span>
                            </Show>
                          </span>
                        </button>
                      )}
                    </For>
                  }
                >
                  <For each={rows().map(row => row.kind === "folder" ? row.key : row.item.id)}>
                    {key => {
                      const row = () => byKey().get(key);
                      return (
                      <Show
                        when={row()?.kind === "folder"}
                        fallback={
                          <button
                            class={"nav-row git-row git-tree-item " + (props.state?.navigator.selected === key ? "active" : "")}
                            style={{ "--git-depth": String(row()?.depth || 0) }}
                            onClick={() => { if (row()?.item) open(row().item); }}
                            title={row()?.item?.path || row()?.item?.key}
                          >
                            <span class={"git-status status-" + (String(row()?.item?.status || "").replace(/[^a-z0-9_-]+/gi, "_").toLowerCase() || "none")}>{row()?.item?.status || "·"}</span>
                            <Icon name="file" size={13} />
                            <span class="nav-row-name">{basename(row()?.item?.path || row()?.item?.label || "")}</span>
                          </button>
                        }
                      >
                        <button
                          class="nav-row git-row git-tree-folder"
                          style={{ "--git-depth": String(row()?.depth || 0) }}
                          onClick={() => fold(key)}
                          title={row()?.path}
                        >
                          <span class="git-status" />
                          <Icon name={collapsed().has(key) ? "chevronRight" : "chevronDown"} size={11} />
                          <Icon name="folder" size={13} />
                          <span class="nav-row-name">{row()?.name}</span>
                        </button>
                      </Show>
                    ); }}
                  </For>
                </Show>
              </Show>
            </>
          ); }}
        </For>
      </div>
      <Show when={!items().length && !loading()}>
        <div class="navigator-empty">{query() ? "No matching Git items" : "Nothing to show."}</div>
      </Show>
    </div>
  );
}

const STATUS_ITEMS = ["model", "provider", "profile", "effort", "directory", "agent", "branch", "tools", "session", "session-tps", "state"];
const STATUS_REGIONS = [
  ["above-composer", "Above composer"],
  ["below-composer", "Below composer"],
  ["fullscreen-status", "Fullscreen"],
];

function normalizedUi(value = {}) {
  return {
    status_lines: (value.status_lines || []).map(line => ({
      region: line.region,
      items: [...(line.items || [])],
      right_items: [...(line.right_items || [])],
    })),
    tui_mode: value.tui_mode || "inline",
    theme: value.theme || "system",
    editor_padding_x: Number(value.editor_padding_x ?? 0),
    output_padding: Number(value.output_padding ?? 0),
    workspace_editor_padding_x: Number(value.workspace_editor_padding_x ?? 1),
    workspace_output_padding: Number(value.workspace_output_padding ?? 1),
    autocomplete_max_visible: Number(value.autocomplete_max_visible ?? 5),
    show_performance: value.show_performance ?? true,
    show_ttft: value.show_ttft ?? false,
    show_waterfall: value.show_waterfall ?? false,
    show_thinking: value.show_thinking ?? true,
    show_tool_calls: value.show_tool_calls ?? true,
  };
}

function Settings(props) {
  const [state, setState] = createSignal(null);
  const [selected, setSelected] = createSignal("");
  const [draft, setDraft] = createSignal({
    name: "",
    kind: "ollama",
    base_url: "http://127.0.0.1:11434",
    default_model: "",
    models: "",
    api_key_env: "",
    allow_insecure_http: false,
    make_default: false,
  });
  const [error, setError] = createSignal("");
  const [chatSetup, setChatSetup] = createSignal(null);
  const [chatModel, setChatModel] = createSignal("");
  const [uiDraft, setUiDraft] = createSignal(normalizedUi());
  const selectedProfile = createMemo(() =>
    state()?.profiles?.find(profile => profile.name === selected()) || null
  );
  const oauthSelected = createMemo(() => Boolean(selectedProfile()?.profile?.oauth_connector));
  let chatPoll;

  onCleanup(() => {
    if (chatPoll) clearTimeout(chatPoll);
  });

  async function pollChatGpt(operationId) {
    try {
      const value = await api("/api/settings/chatgpt/status");
      const setup = value.setup;
      if (!setup || setup.operation_id !== operationId) return;
      setChatSetup(setup);
      if (setup.status === "ready") {
        if (!chatModel() && setup.models?.length) setChatModel(setup.models[0].slug);
        return;
      }
      if (["failed", "configured", "cancelled", "expired"].includes(setup.status)) return;
      chatPoll = setTimeout(() => void pollChatGpt(operationId), 900);
    } catch (e) {
      setError(e.message);
    }
  }

  async function beginChatGpt() {
    setError("");
    if (chatPoll) clearTimeout(chatPoll);
    const popup = window.open("about:blank", "aw-chatgpt-signin", "popup,width=720,height=760");
    try {
      const started = await api("/api/settings/chatgpt/start", {});
      setChatSetup({
        operation_id: started.operation_id,
        status: "waiting",
        authorization_url: started.authorization_url,
        models: [],
      });
      if (popup) popup.location.href = started.authorization_url;
      void pollChatGpt(started.operation_id);
    } catch (e) {
      popup?.close();
      setError(e.message);
    }
  }

  async function finishChatGpt() {
    const setup = chatSetup();
    if (!setup || setup.status !== "ready" || !chatModel()) return;
    setError("");
    try {
      const next = await api("/api/settings/chatgpt/finish", {
        operation_id: setup.operation_id,
        model: chatModel(),
      });
      setState(next);
      setSelected(next.default_profile || "");
      fill(next.default_profile || "", next);
      setChatSetup({ ...setup, status: "configured" });
      await props.changed();
    } catch (e) {
      setError(e.message);
    }
  }

  async function closeSettings() {
    const setup = chatSetup();
    if (setup && ["starting", "waiting", "ready"].includes(setup.status)) {
      if (chatPoll) clearTimeout(chatPoll);
      try {
        const result = await api("/api/settings/chatgpt/cancel", {
          operation_id: setup.operation_id,
        });
        if (result.remote_revocation_confirmed === false) {
          setError("Local ChatGPT credentials were cleared, but remote revocation was not confirmed. You can review app access in ChatGPT settings.");
          return;
        }
      } catch (e) {
        setError(e.message);
        return;
      }
    }
    applyThemePreference(state()?.ui?.theme || "system", { persist: false });
    props.close();
  }

  async function load() {
    try {
      const value = await api("/api/settings");
      setState(value);
      setUiDraft(normalizedUi(value.ui));
      applyThemePreference(value.ui?.theme || "system", { persist: true });
      const name = value.default_profile || value.profiles?.[0]?.name || "";
      setSelected(name);
      fill(name, value);
    } catch (e) {
      setError(e.message);
    }
  }
  onMount(load);

  function fill(name, value = state()) {
    const entry = value?.profiles?.find(item => item.name === name);
    if (!entry) return;
    const p = entry.profile;
    setDraft({
      name: entry.name,
      kind: p.kind,
      base_url: p.base_url,
      default_model: p.model || "",
      models: (p.models || []).join("\n"),
      api_key_env: p.api_key_env || "",
      allow_insecure_http: Boolean(p.allow_insecure_http),
      make_default: Boolean(entry.is_default),
    });
  }

  function preset(kind) {
    if (kind === "ollama-cloud") {
      setDraft({ name: "ollama-cloud", kind: "ollama", base_url: "https://ollama.com", default_model: "", models: "", api_key_env: "OLLAMA_API_KEY", allow_insecure_http: false, make_default: true });
    }
    if (kind === "local") {
      setDraft({ name: "ollama", kind: "ollama", base_url: "http://127.0.0.1:11434", default_model: "", models: "", api_key_env: "", allow_insecure_http: false, make_default: true });
    }
    if (kind === "chat") {
      setDraft({ name: "openai-compatible", kind: "openai", base_url: "https://api.openai.com/v1", default_model: "", models: "", api_key_env: "OPENAI_API_KEY", allow_insecure_http: false, make_default: true });
    }
    if (kind === "responses") {
      setDraft({ name: "responses-compatible", kind: "openai-responses", base_url: "https://api.openai.com/v1", default_model: "", models: "", api_key_env: "OPENAI_API_KEY", allow_insecure_http: false, make_default: true });
    }
  }

  async function save(event) {
    event.preventDefault();
    setError("");
    try {
      const body = {
        ...draft(),
        default_model: draft().default_model.trim() || null,
        models: draft().models.split(/\r?\n/).map(value => value.trim()).filter(Boolean),
        api_key_env: draft().api_key_env.trim() || null,
      };
      const next = await api("/api/settings/profile", body);
      setState(next);
      setSelected(body.name);
      fill(body.name, next);
      await props.changed();
    } catch (e) {
      setError(e.message);
    }
  }

  async function makeSelectedDefault() {
    const profile = selectedProfile();
    if (!profile) return;
    setError("");
    try {
      const next = await api("/api/settings/default", { name: profile.name });
      setState(next);
      setSelected(profile.name);
      fill(profile.name, next);
      await props.changed();
    } catch (e) {
      setError(e.message);
    }
  }

  async function removeSelected() {
    const profile = selectedProfile();
    if (!profile || profile.builtin) return;
    const message = oauthSelected()
      ? "Disconnect this ChatGPT account from aw?"
      : "Remove profile " + profile.name + "?";
    if (!confirm(message)) return;
    setError("");
    try {
      const next = await api("/api/settings/profile/remove", { name: profile.name });
      setState(next);
      const name = next.default_profile || next.profiles?.[0]?.name || "";
      setSelected(name);
      fill(name, next);
      if (next.notice) setError(next.notice);
      await props.changed();
    } catch (e) {
      setError(e.message);
    }
  }


  function patchUi(patch) {
    setUiDraft(current => ({ ...current, ...patch }));
  }

  function patchStatusLine(index, patch) {
    setUiDraft(current => {
      const lines = current.status_lines.map((line, at) =>
        at === index ? { ...line, ...patch } : line
      );
      return { ...current, status_lines: lines };
    });
  }

  function toggleStatusItem(index, item, side) {
    setUiDraft(current => {
      const lines = current.status_lines.map((line, at) => {
        if (at !== index) return line;
        const field = side === "right" ? "right_items" : "items";
        const other = side === "right" ? "items" : "right_items";
        const enabled = line[field].includes(item);
        const next = enabled
          ? line[field].filter(value => value !== item)
          : [...line[field], item];
        return {
          ...line,
          [field]: next,
          [other]: enabled ? line[other] : line[other].filter(value => value !== item),
        };
      });
      return { ...current, status_lines: lines };
    });
  }

  function addStatusLine() {
    setUiDraft(current => {
      if (current.status_lines.length >= 32) return current;
      return {
        ...current,
        status_lines: [
          ...current.status_lines,
          { region: "below-composer", items: ["model"], right_items: [] },
        ],
      };
    });
  }

  function removeStatusLine(index) {
    setUiDraft(current => ({
      ...current,
      status_lines: current.status_lines.filter((_, at) => at !== index),
    }));
  }

  function moveStatusLine(index, delta) {
    setUiDraft(current => {
      const next = index + delta;
      if (next < 0 || next >= current.status_lines.length) return current;
      const lines = current.status_lines.slice();
      [lines[index], lines[next]] = [lines[next], lines[index]];
      return { ...current, status_lines: lines };
    });
  }

  async function savePresentation() {
    setError("");
    try {
      const value = uiDraft();
      if (value.status_lines.some(line => !line.items.length && !line.right_items.length)) {
        setError("Each status row needs at least one left or right item, or remove the row.");
        return;
      }
      const next = await api("/api/settings/ui", {
        status_lines: value.status_lines,
        tui_mode: value.tui_mode,
        theme: value.theme,
        editor_padding_x: Number(value.editor_padding_x),
        output_padding: Number(value.output_padding),
        workspace_editor_padding_x: Number(value.workspace_editor_padding_x),
        workspace_output_padding: Number(value.workspace_output_padding),
        autocomplete_max_visible: Number(value.autocomplete_max_visible),
        show_performance: Boolean(value.show_performance),
        show_ttft: Boolean(value.show_ttft),
        show_waterfall: Boolean(value.show_waterfall),
        show_thinking: Boolean(value.show_thinking),
        show_tool_calls: Boolean(value.show_tool_calls),
      });
      setState(next);
      setUiDraft(normalizedUi(next.ui));
      applyThemePreference(next.ui?.theme || "system", { persist: true });
      await props.changed(next.ui);
    } catch (e) {
      setError(e.message);
    }
  }

  return (
    <div class="scrim" onMouseDown={event => event.target === event.currentTarget && void closeSettings()}>
      <section class="modal" role="dialog" aria-modal="true" aria-label="Settings">
        <header class="modal-head">
          <h2>Settings</h2>
          <button class="quiet" onClick={() => void closeSettings()}>Close</button>
        </header>
        <div class="settings">
          <h3>Providers & models</h3>
          <div class="provider-grid">
            <button class="provider-card" onClick={beginChatGpt}>
              <strong>ChatGPT subscription</strong>
              <span>Continue with ChatGPT using the supported OAuth + Responses flow. Tokens stay in local SecretStore.</span>
            </button>
            <button class="provider-card" onClick={() => preset("ollama-cloud")}>
              <strong>Ollama Cloud</strong>
              <span>Guided native Ollama endpoint with an environment-reference credential.</span>
            </button>
            <button class="provider-card" onClick={() => preset("chat")}>
              <strong>Chat Completions compatible</strong>
              <span>Custom endpoint using the OpenAI-compatible streaming chat wire format.</span>
            </button>
            <button class="provider-card" onClick={() => preset("responses")}>
              <strong>Responses API compatible</strong>
              <span>Custom endpoint using the streaming OpenAI Responses wire format.</span>
            </button>
          </div>

          <Show when={chatSetup()}>
            <div class="context-card">
              <strong>ChatGPT · {chatSetup().status}</strong>
              <Show when={chatSetup().status === "waiting"}>
                <p class="settings-note">
                  Finish sign-in in the browser window.
                  <Show when={chatSetup().authorization_url}>
                    {" "}<a href={chatSetup().authorization_url} target="_blank" rel="noopener noreferrer">Open sign-in again</a>
                  </Show>
                </p>
              </Show>
              <Show when={chatSetup().status === "ready"}>
                <div class="row">
                  <select value={chatModel()} onChange={event => setChatModel(event.currentTarget.value)}>
                    <For each={chatSetup().models || []}>
                      {model => <option value={model.slug}>{model.display_name} · {model.slug}</option>}
                    </For>
                  </select>
                  <button class="primary" type="button" onClick={finishChatGpt}>Use this model</button>
                </div>
              </Show>
              <Show when={chatSetup().status === "failed" || chatSetup().status === "expired"}>
                <div class="error">{chatSetup().error || (chatSetup().status === "expired" ? "ChatGPT setup expired. Start it again when ready." : "ChatGPT sign-in failed.")}</div>
              </Show>
              <Show when={chatSetup().status === "configured"}>
                <p class="settings-note">Connected. The profile is now available to this local owner.</p>
              </Show>
            </div>
          </Show>

          <div class="profile-list">
            <For each={state()?.profiles || []}>
              {profile => (
                <button
                  class={"profile-chip " + (selected() === profile.name ? "active" : "")}
                  onClick={() => { setSelected(profile.name); fill(profile.name); }}
                >
                  {profile.name}{profile.is_default ? " · default" : ""}
                </button>
              )}
            </For>
          </div>

          <Show
            when={!oauthSelected()}
            fallback={
              <div class="context-card">
                <strong>Connected with ChatGPT</strong>
                <p class="settings-note">
                  {selectedProfile()?.profile?.model || "No model selected"} · Responses API · credentials in local SecretStore
                </p>
                <div class="row">
                  <Show when={!selectedProfile()?.is_default}>
                    <button class="quiet" type="button" onClick={makeSelectedDefault}>Make default</button>
                  </Show>
                  <button class="quiet" type="button" onClick={removeSelected}>Disconnect</button>
                </div>
              </div>
            }
          >
            <form onSubmit={save}>
              <div class="settings-grid">
                <label class="field">
                  <span>Profile name</span>
                  <input value={draft().name} onInput={event => setDraft({ ...draft(), name: event.currentTarget.value })} />
                </label>
                <label class="field">
                  <span>Provider</span>
                  <select value={draft().kind} onChange={event => setDraft({ ...draft(), kind: event.currentTarget.value })}>
                    <option value="ollama">Ollama</option>
                    <option value="openai">Chat Completions</option>
                    <option value="openai-responses">Responses API</option>
                  </select>
                </label>
                <label class="field settings-wide">
                  <span>Base URL</span>
                  <input value={draft().base_url} onInput={event => setDraft({ ...draft(), base_url: event.currentTarget.value })} />
                </label>
                <label class="field settings-wide">
                  <span>Default model</span>
                  <input value={draft().default_model} onInput={event => setDraft({ ...draft(), default_model: event.currentTarget.value })} placeholder="Explicit model ID" />
                </label>
                <label class="field settings-wide">
                  <span>Additional models · one per line</span>
                  <textarea rows="3" value={draft().models} onInput={event => setDraft({ ...draft(), models: event.currentTarget.value })} />
                </label>
                <label class="field settings-wide">
                  <span>API-key environment variable</span>
                  <input value={draft().api_key_env} onInput={event => setDraft({ ...draft(), api_key_env: event.currentTarget.value })} placeholder="Secret value is never saved here" />
                </label>
              </div>
              <div class="row">
                <label><input type="checkbox" checked={draft().make_default} onChange={event => setDraft({ ...draft(), make_default: event.currentTarget.checked })} /> Make default</label>
                <label><input type="checkbox" checked={draft().allow_insecure_http} onChange={event => setDraft({ ...draft(), allow_insecure_http: event.currentTarget.checked })} /> Allow non-loopback HTTP</label>
                <button class="primary" type="submit">Save profile</button>
                <Show when={selectedProfile() && !selectedProfile().builtin}>
                  <button class="quiet" type="button" onClick={removeSelected}>Remove</button>
                </Show>
              </div>
            </form>
          </Show>
          <p class="settings-note">
            Saving configuration makes no provider/model request. Changing profile/model never imports old model context into the new endpoint.
          </p>
          <h3>Presentation</h3>
          <p class="settings-note">
            Status rows are owned by the status-line extension. Add, remove, reorder, or place rows above/below the inline composer or in fullscreen without changing the core renderer.
          </p>
          <div class="presentation-grid">
            <label class="field">
              <span>Theme</span>
              <select
                value={uiDraft().theme}
                onChange={event => {
                  const theme = event.currentTarget.value;
                  patchUi({ theme });
                  applyThemePreference(theme, { persist: false });
                }}
              >
                <option value="system">System</option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
              </select>
            </label>
            <label class="field">
              <span>TUI startup mode</span>
              <select value={uiDraft().tui_mode} onChange={event => patchUi({ tui_mode: event.currentTarget.value })}>
                <option value="inline">Inline</option>
                <option value="fullscreen">Fullscreen</option>
              </select>
            </label>
            <label class="field">
              <span>Autocomplete rows</span>
              <input
                type="number"
                min="3"
                max="20"
                value={uiDraft().autocomplete_max_visible}
                onInput={event => patchUi({ autocomplete_max_visible: Number(event.currentTarget.value) })}
              />
            </label>
            <label class="field">
              <span>Agent editor padding</span>
              <input
                type="number"
                min="0"
                max="3"
                value={uiDraft().editor_padding_x}
                onInput={event => patchUi({ editor_padding_x: Number(event.currentTarget.value) })}
              />
            </label>
            <label class="field">
              <span>Agent output padding</span>
              <input
                type="number"
                min="0"
                max="3"
                value={uiDraft().output_padding}
                onInput={event => patchUi({ output_padding: Number(event.currentTarget.value) })}
              />
            </label>
            <label class="field">
              <span>Workspace editor padding</span>
              <input
                type="number"
                min="0"
                max="3"
                value={uiDraft().workspace_editor_padding_x}
                onInput={event => patchUi({ workspace_editor_padding_x: Number(event.currentTarget.value) })}
              />
            </label>
            <label class="field">
              <span>Workspace output padding</span>
              <input
                type="number"
                min="0"
                max="3"
                value={uiDraft().workspace_output_padding}
                onInput={event => patchUi({ workspace_output_padding: Number(event.currentTarget.value) })}
              />
            </label>
          </div>

          <div class="presentation-toggles">
            <label><input type="checkbox" checked={uiDraft().show_performance} onChange={event => patchUi({ show_performance: event.currentTarget.checked })} /> TPS / performance</label>
            <label><input type="checkbox" checked={uiDraft().show_ttft} onChange={event => patchUi({ show_ttft: event.currentTarget.checked })} /> TTFT</label>
            <label><input type="checkbox" checked={uiDraft().show_waterfall} onChange={event => patchUi({ show_waterfall: event.currentTarget.checked })} /> Waterfall</label>
            <label><input type="checkbox" checked={uiDraft().show_thinking} onChange={event => patchUi({ show_thinking: event.currentTarget.checked })} /> Thinking activity</label>
            <label><input type="checkbox" checked={uiDraft().show_tool_calls} onChange={event => patchUi({ show_tool_calls: event.currentTarget.checked })} /> Tool details</label>
          </div>

          <div class="status-lines-editor">
            <div class="status-lines-head">
              <div>
                <strong>Status rows</strong>
                <span>{uiDraft().status_lines.length} / 32</span>
              </div>
              <button class="quiet" type="button" onClick={addStatusLine} disabled={uiDraft().status_lines.length >= 32}>+ Add row</button>
            </div>
            <For each={uiDraft().status_lines}>
              {(line, index) => (
                <div class="status-line-card">
                  <div class="status-line-head">
                    <select
                      value={line.region}
                      onChange={event => patchStatusLine(index(), { region: event.currentTarget.value })}
                      aria-label={"Status row " + (index() + 1) + " region"}
                    >
                      <For each={STATUS_REGIONS}>
                        {region => <option value={region[0]}>{region[1]}</option>}
                      </For>
                    </select>
                    <span class="status-line-number">Row {index() + 1}</span>
                    <div class="status-line-actions">
                      <button class="mini-icon" type="button" onClick={() => moveStatusLine(index(), -1)} disabled={index() === 0} aria-label="Move status row up">↑</button>
                      <button class="mini-icon" type="button" onClick={() => moveStatusLine(index(), 1)} disabled={index() === uiDraft().status_lines.length - 1} aria-label="Move status row down">↓</button>
                      <button class="mini-icon" type="button" onClick={() => removeStatusLine(index())} aria-label="Remove status row"><Icon name="close" size={12} /></button>
                    </div>
                  </div>
                  <div class="status-item-grid">
                    <div>
                      <div class="status-line-number">Left</div>
                      <For each={STATUS_ITEMS}>
                        {item => (
                          <label class={"status-item-chip " + (line.items.includes(item) ? "active" : "")}>
                            <input
                              type="checkbox"
                              checked={line.items.includes(item)}
                              onChange={() => toggleStatusItem(index(), item, "left")}
                            />
                            <span>{item}</span>
                          </label>
                        )}
                      </For>
                    </div>
                    <div>
                      <div class="status-line-number">Right</div>
                      <For each={STATUS_ITEMS}>
                        {item => (
                          <label class={"status-item-chip " + (line.right_items.includes(item) ? "active" : "")}>
                            <input
                              type="checkbox"
                              checked={line.right_items.includes(item)}
                              onChange={() => toggleStatusItem(index(), item, "right")}
                            />
                            <span>{item}</span>
                          </label>
                        )}
                      </For>
                    </div>
                  </div>
                </div>
              )}
            </For>
            <Show when={!uiDraft().status_lines.length}>
              <div class="navigator-empty">No status rows. Add one whenever you want status-line output again.</div>
            </Show>
          </div>
          <div class="row presentation-save">
            <button class="primary" type="button" onClick={savePresentation}>Save presentation</button>
          </div>
          <Show when={error()}><div class="error">{error()}</div></Show>
        </div>
      </section>
    </div>
  );
}

function CollaborationPanel(props) {
  const mode = () => props.collaboration?.policy?.mode || "off";
  const owner = () => !props.sharedSession;
  const granted = participant =>
    (props.collaboration?.grants || []).some(grant =>
      grant.status === "active"
      && participant?.user_id === grant.user_id
      && grant.grant?.can_read
    );

  return (
    <div class="collab-panel">
      <div class="collab-head">
        <div>
          <strong>{props.sharedSession ? "Shared prompt" : "Collaborate"}</strong>
          <span>
            {props.active
              ? `Live · r${props.draft?.accepted_revision ?? 0}`
              : props.sharedSession
                ? "Explicitly shared Session"
                : `Mode · ${mode()}`}
          </span>
        </div>
        <button class="mini-icon" type="button" onClick={props.close} aria-label="Close collaboration">
          <Icon name="close" size={13} />
        </button>
      </div>

      <Show when={owner()}>
        <div class="collab-mode">
          <span>Shared draft mode</span>
          <select value={mode()} onChange={event => props.setMode(event.currentTarget.value)}>
            <option value="off">Off</option>
            <option value="auto">Auto</option>
            <option value="on">On</option>
          </select>
        </div>
        <div class="collab-people">
          <For each={props.collaborators || []}>
            {participant => (
              <div class="collab-person">
                <span>{participant.display_name}</span>
                <button
                  class={granted(participant) ? "quiet active" : "quiet"}
                  type="button"
                  onClick={() => props.share(participant.id, !granted(participant))}
                >
                  {granted(participant) ? "Revoke" : "Share"}
                </button>
              </div>
            )}
          </For>
          <Show when={!props.collaborators?.length}>
            <span class="collab-muted">No other authenticated participants.</span>
          </Show>
        </div>
      </Show>

      <Show when={props.publishChoice}>
        <div class="collab-choice">
          <strong>Your current composer text is private.</strong>
          <span>Joining will not publish it unless you explicitly share it.</span>
          <div>
            <button class="primary" type="button" onClick={props.publishPrivate}>Share this draft</button>
            <button class="quiet" type="button" onClick={props.keepPrivate}>Keep private</button>
          </div>
        </div>
      </Show>

      <Show when={!props.publishChoice && !props.active && mode() !== "off"}>
        <button class="primary collab-join" type="button" onClick={props.join}>
          {props.sharedSession ? "Join shared prompt" : "Start shared prompt"}
        </button>
      </Show>

      <Show when={!props.publishChoice && !props.active && mode() === "off"}>
        <span class="collab-muted">
          {owner() ? "Choose Auto or On to enable shared prompt drafts." : "The Session owner has collaboration Off."}
        </span>
      </Show>

      <Show when={props.active}>
        <div class="collab-live">
          <div class="collab-presence">
            <span class="presence-dot" />
            <span>Shared text is live</span>
            <For each={props.presence || []}>
              {presence => <span class="presence-person">{presence.display_name}</span>}
            </For>
          </div>
          <button class="quiet" type="button" onClick={props.leave}>Leave · keep text private</button>
        </div>
      </Show>
    </div>
  );
}

function App() {
  const [snapshot, setSnapshot] = createSignal(null);
  const [paired, setPaired] = createSignal(false);
  const [pairCode, setPairCode] = createSignal("");
  const [connection, setConnection] = createSignal("Connecting…");
  const [online, setOnline] = createSignal(false);
  const [error, setError] = createSignal("");
  const [prompt, setPromptValue] = createSignal("");
  const [draftOrigin, setDraftOrigin] = createSignal(null);
  const [posting, setPosting] = createSignal(false);
  const [account, setAccount] = createSignal(null);
  const [sessions, setSessions] = createSignal([]);
  const [teamInfo, setTeamInfo] = createSignal(null);
  const [collaborators, setCollaborators] = createSignal([]);
  const [collaboration, setCollaboration] = createSignal(null);
  const [sharedDraft, setSharedDraft] = createSignal(null);
  const [sharedPresence, setSharedPresence] = createSignal([]);
  const [collabPanel, setCollabPanel] = createSignal(false);
  const [publishChoice, setPublishChoice] = createSignal(false);
  const [nav, setNav] = createSignal("sessions");
  const [mobileNav, setMobileNav] = createSignal(false);
  const [dockingDocument, setDockingDocument] = createSignal(null);
  const [settings, setSettings] = createSignal(false);
  const [presentationUi, setPresentationUi] = createSignal(normalizedUi({}));
  const [workspaceEnabled, setWorkspaceEnabled] = createSignal(false);
  const [tabs, setTabs] = createSignal([{ id: "chat", kind: "chat", title: "Agent" }]);
  const [activeTab, setActiveTab] = createSignal("chat");
  const [sidebarWidth, setSidebarWidth] = createSignal(
    Math.min(520, Math.max(220, Number(localStorage.getItem("aw.sidebar.width")) || 286)),
  );
  let resizingSidebar = false;
  // The correlation fragment is removed from history before any API request.
  // It is held only in this renderer's memory, never browser storage.
  let pendingDockTicket = takeDocumentDockTicket(window.location, window.history);
  let source;
  let observationLifetime = {};
  let workspaceDiscoveryLifetime = {};
  let sharedClient = null;
  let sharedDraftOrigin = null;
  let composerTextarea = null;
  let presencePending = null;
  let presenceLastSent = 0;
  let sessionCatalogRequest = 0;
  let deniedCatalogRefresh = null;
  const tabTargets = new Map();
  const tabElementId = id => "workspace-tab-" + encodeURIComponent(id);
  const [viewChange, setViewChange] = createSignal(null);
  const [viewSource, setViewSource] = createSignal(null);
  const [workspaceChange, setWorkspaceChange] = createSignal(null);
  const [workspaceSource, setWorkspaceSource] = createSignal(null);
  const [workspaceToken, setWorkspaceToken] = createSignal({});
  const [selectedContainers, setSelectedContainers] = createSignal({ files: null, git: null });
  const presentationControllers = new Map();
  const bindPresentation = (documentId, controller) => {
    if (controller) presentationControllers.set(documentId, controller);
    else presentationControllers.delete(documentId);
  };
  const workspaceViews = createWorkspaceViews({
    request: async (body, { signal }) => {
      const lifetime = observationLifetime;
      try { return await workspaceAction(body, { signal }); }
      catch (error) {
        if (lifetime === observationLifetime && error.status === 401) expireBrowserAccess();
        throw error;
      }
    },
    externalDocumentCount: () => tabs().filter(tab => tab.kind !== "file" && tab.kind !== "review").length,
    onChange: event => untrack(() => {
      if (event.type === "clear" || event.type === "source") presentationControllers.clear();
      setWorkspaceChange({});
      const documents = workspaceViews.listDocuments();
      const live = new Set(documents.map(document => document.id));
      setTabs(current => {
        const kept = current.filter(tab => (tab.kind !== "file" && tab.kind !== "review") || live.has(tab.id));
        for (const tab of current) if (!kept.includes(tab)) tabTargets.delete(tab.id);
        const present = new Set(kept.map(tab => tab.id));
        return [...kept, ...documents.filter(document => !present.has(document.id))
          .map(document => ({ id: document.id, kind: document.kind, title: document.kind === "file" ? "File" : "Diff" }))];
      });
      if (activeTab().startsWith("workspace-document-") && !live.has(activeTab())) setActiveTab("chat");
      const containers = workspaceViews.listContainers();
      setSelectedContainers(current => Object.fromEntries(["files", "git"].map(kind => [kind,
        containers.some(container => container.id === current[kind] && container.kind === kind)
          ? current[kind] : containers.find(container => container.kind === kind)?.id || null])));
      if (["clear", "source", "scope-denied", "refresh", "close-container"].includes(event.type)) setWorkspaceToken({});
      if (event.type === "clear") {
        workspaceDiscoveryLifetime = {};
        setWorkspaceSource(null);
      }
    }),
  });
  const workspaceContainers = createMemo(() => { workspaceChange(); return workspaceViews.listContainers(); });
  const workspaceDocumentIds = createMemo(() => { workspaceChange(); return workspaceViews.listDocuments().map(document => document.id); });
  const workspaceDocument = id => { workspaceChange(); return workspaceViews.getDocument(id); };
  const workspaceContainer = id => { workspaceChange(); return workspaceViews.getContainer(id); };
  function clearWorkspaceViews() {
    workspaceViews.clear();
    setWorkspaceSource(null);
    setWorkspaceEnabled(false);
  }
  const documentDocking = createDocumentDocking({
    api, views: workspaceViews,
    capture: documentId => presentationControllers.get(documentId)?.capture() || null,
    restore: (documentId, state) => presentationControllers.get(documentId)?.restore(state) || false,
    currentOwner: () => {
      const scope = workspaceSource(), owner = snapshot();
      return scope && owner ? {
        source_id: scope.source_id, scope_id: scope.view_root.id,
        owner_epoch: owner.instance,
      } : null;
    },
    select: (containerId, documentId, kind) => {
      const navigator = kind === "review" ? "git" : "files";
      setSelectedContainers(current => ({ ...current, [navigator]: containerId }));
      setNav(navigator);
      setActiveTab(documentId);
      setMobileNav(false);
    },
    close: documentId => closeTab(documentId),
    popup: () => window.open("about:blank", "_blank"),
    onProgress: (documentId, active) => setDockingDocument(active ? documentId : null),
    isRendered: documentId => activeTab() === documentId
      && Boolean(document.querySelector('.surface.code-surface:not([hidden]) .cm-editor')),
  });
  async function receivePendingDocumentDock() {
    if (!pendingDockTicket || !paired()) return;
    const ticket = pendingDockTicket;
    pendingDockTicket = null;
    if (!workspaceEnabled() || !workspaceSource()) {
      setError("The original read-only workspace owner is unavailable here. No view was transferred.");
      return;
    }
    try { await documentDocking.receive(ticket); }
    catch (error) { setError(error.message); }
  }
  const agentViews = createAgentViews({
    read: async (target, { signal }) => {
      const lifetime = observationLifetime;
      try { return await api("/api/agent-view", target, 15000, { signal }); }
      catch (error) {
        if (lifetime !== observationLifetime) throw error;
        if (error.status === 401) expireBrowserAccess();
        else if (error.status === 403 && deniedCatalogRefresh !== lifetime) {
          deniedCatalogRefresh = lifetime;
          queueMicrotask(() => {
            if (lifetime !== observationLifetime) return;
            void loadSessions(lifetime).finally(() => { if (deniedCatalogRefresh === lifetime) deniedCatalogRefresh = null; });
          });
        }
        throw error;
      }
    },
    onChange: () => {
      setViewChange({});
      const live = new Set(agentViews.list().map(view => view.id));
      setTabs(current => {
        const removed = current.filter(tab => tab.kind === "agent-view" && !live.has(tab.id));
        for (const tab of removed) tabTargets.delete(tab.id);
        return removed.length ? current.filter(tab => tab.kind !== "agent-view" || live.has(tab.id)) : current;
      });
      if (activeTab().startsWith("agent-view-") && !live.has(activeTab())) setActiveTab("chat");
    },
  });
  const pinnedIds = createMemo(() => { viewChange(); return agentViews.list().map(view => view.id); });
  const pinnedView = id => { viewChange(); return agentViews.get(id); };
  function clearAgentViews(reason, keepCatalogRequest = false) {
    if (!keepCatalogRequest && sessionCatalogRequest < Number.MAX_SAFE_INTEGER) sessionCatalogRequest += 1;
    agentViews.clear(reason);
    setViewSource(null);
    setSessions([]);
  }
  function expireBrowserAccess() {
    observationLifetime = {};
    const expiredSource = source;
    source = null;
    expiredSource?.close();
    clearAgentViews("Browser access expired.");
    clearWorkspaceViews();
    setPaired(false);
    setOnline(false);
    setConnection("Not paired");
  }
  function tabTitle(tab) {
    if (tab.kind === "file" || tab.kind === "review") {
      const document = workspaceDocument(tab.id);
      return document?.data ? (tab.kind === "review" ? "Diff: " : "") + basename(document.data.path || "file") : tab.title;
    }
    if (tab.kind !== "agent-view") return tab.title;
    const data = pinnedView(tab.id)?.snapshot;
    return data ? `View: ${data.title || "Untitled"} · ${data.branch_name}` : tab.title;
  }
  function tabDescription(tab) {
    if (tab.kind !== "file" && tab.kind !== "review") return tabTitle(tab);
    const document = workspaceDocument(tab.id);
    if (!document?.data) return tabTitle(tab);
    const owner = workspaceContainer(document.containerId);
    const peers = workspaceContainers().filter(container => container.kind === owner?.kind);
    const position = peers.findIndex(container => container.id === document.containerId) + 1;
    return [tabTitle(tab), document.data.root || owner?.label, document.data.path, document.data.basis,
      owner ? `${owner.kind === "files" ? "Files" : "Git"} view ${position}` : ""].filter(Boolean).join(" · ");
  }
  function pulseAgentViews() {
    agentViews.expire();
    if (document.hidden) return;
    const now = performance.now();
    for (const view of agentViews.list().slice().sort((a, b) => a.refreshAt - b.refreshAt)) {
      if (!view.pending && view.refreshAt <= now) void agentViews.refresh(view.id);
    }
  }
  createEffect(on(() => snapshot()?.instance, (instance, previous) => {
    if (previous && instance !== previous) {
      clearAgentViews("Owner changed; reopen the read-only view.");
      clearWorkspaceViews();
    }
  }));


  const choices = createMemo(() => accountChoices(snapshot()));
  const surface = createMemo(() =>
    tabs().find(tab => tab.id === activeTab()) || tabs()[0] || { id: "chat", kind: "chat", title: "Agent" }
  );
  const activeFilePath = containerId => {
    const document = workspaceDocument(activeTab());
    return document?.kind === "file" && document.containerId === containerId && document.data ? document.data.path : null;
  };
  createEffect(() => {
    const id = activeTab();
    void tabs();
    queueMicrotask(() => {
      if (activeTab() === id) {
        tabTargets.get(id)?.parentElement?.scrollIntoView({ block: "nearest", inline: "nearest" });
      }
    });
  });
  const currentSession = createMemo(() => {
    const id = snapshot()?.saved?.session_id;
    return id ? sessions().find(session => session.id === id) || null : null;
  });
  const agentTabTitle = createMemo(() => {
    const value = snapshot()?.saved?.title?.trim();
    return value ? `Agent: ${value}` : "Agent";
  });
  createEffect(() => {
    const nextTitle = agentTabTitle();
    setTabs(current => {
      const chat = current.find(tab => tab.id === "chat");
      if (!chat || chat.title === nextTitle) return current;
      return current.map(tab => tab.id === "chat" ? { ...tab, title: nextTitle } : tab);
    });
  });
  const teamSharing = createMemo(() => Boolean(teamInfo()?.peer_sharing && snapshot()?.saved));
  const sharedActive = createMemo(() => Boolean(sharedDraft() && sharedClient?.active?.()));
  const draftRetargetRequired = createMemo(() => Boolean(snapshot()) && !sharedActive()
    && draftTargetChanged(draftOrigin(), snapshot(), prompt()));

  // Keep only a target key: an unsent draft must never retain old history.
  function draftTarget(value = snapshot()) {
    return value ? {
      instance: value.instance,
      saved: value.saved ? {
        session_id: value.saved.session_id,
        branch_id: value.saved.branch_id,
      } : null,
    } : null;
  }

  function sameDraftTarget(left, right) {
    return left === right || Boolean(left && right && !draftTargetChanged(left, right, "draft"));
  }

  function setPrompt(value, origin = undefined) {
    if (!value.trim()) setDraftOrigin(null);
    else if (origin) setDraftOrigin(origin);
    else if (!prompt().trim() || !draftOrigin()) setDraftOrigin(draftTarget());
    return setPromptValue(value);
  }

  function useDraftHere() {
    const current = snapshot();
    if (!current || sharedActive() || !prompt().trim()) return;
    setDraftOrigin(draftTarget(current));
    setError("");
    composerTextarea?.focus();
  }

  createEffect(() => {
    const all = choices();
    if (!all.length) {
      setAccount(null);
      return;
    }
    const current = account();
    if (!current || !all.some(choice => choice.account_id === current.account_id && choice.model === current.model)) {
      const preferred =
        all.find(choice => choice.account_id === snapshot()?.profile && choice.model === snapshot()?.model) ||
        all[0];
      setAccount(preferred);
    }
  });

  async function detectTeam(lifetime = observationLifetime) {
    try {
      const info = await api("/api/team/me");
      if (lifetime !== observationLifetime) return null;
      if (!info?.peer_sharing) {
        setTeamInfo(info || null);
        setCollaborators([]);
        setCollaboration(null);
        return info || null;
      }
      setTeamInfo(info);
      try {
        const value = await api("/api/team/collaborators");
        if (lifetime !== observationLifetime) return null;
        setCollaborators(value.participants || []);
      } catch {
        if (lifetime !== observationLifetime) return null;
        setCollaborators([]);
      }
      return info;
    } catch {
      if (lifetime !== observationLifetime) return null;
      setTeamInfo(null);
      setCollaborators([]);
      setCollaboration(null);
      return null;
    }
  }

  async function loadCollaboration(lifetime = observationLifetime) {
    if (lifetime !== observationLifetime) return null;
    if (!teamInfo()?.peer_sharing || !snapshot()?.saved) {
      setCollaboration(null);
      return null;
    }
    try {
      const value = await api("/api/team/session/collaboration");
      if (lifetime !== observationLifetime) return null;
      setCollaboration(value);
      return value;
    } catch {
      if (lifetime !== observationLifetime) return null;
      setCollaboration(null);
      return null;
    }
  }

  async function loadSessions(lifetime = observationLifetime) {
    if (lifetime !== observationLifetime) return;
    if (sessionCatalogRequest >= Number.MAX_SAFE_INTEGER) {
      clearAgentViews("Session list lifetime exhausted; reload the browser.");
      setSessions([]);
      return;
    }
    const ticket = ++sessionCatalogRequest;
    try {
      const catalog = await api("/api/sessions");
      if (lifetime !== observationLifetime || ticket !== sessionCatalogRequest || catalog.source_epoch !== snapshot()?.instance) return;
      const nextSource = typeof catalog.store_id === "string" && catalog.store_id
        ? { store_id: catalog.store_id, source_epoch: catalog.source_epoch } : null;
      const currentSource = viewSource();
      if (currentSource && (currentSource.store_id !== nextSource?.store_id || currentSource.source_epoch !== nextSource?.source_epoch)) {
        clearAgentViews("Store changed; reopen the read-only view.", true);
      }
      setViewSource(nextSource);
      const own = (catalog.sessions || []).map(session => ({
        ...session,
        shared: false,
      }));
      let shared = [];
      if (teamInfo()?.peer_sharing) {
        try {
          shared = ((await api("/api/team/shared-sessions")).sessions || []).map(session => ({
            ...session,
            shared: true,
          }));
        } catch {}
      }
      const installed = viewSource();
      if (lifetime !== observationLifetime || ticket !== sessionCatalogRequest || catalog.source_epoch !== snapshot()?.instance
        || installed?.store_id !== nextSource?.store_id || installed?.source_epoch !== nextSource?.source_epoch) return;
      const seen = new Set();
      setSessions([...own, ...shared].filter(session => {
        if (seen.has(session.id)) return false;
        seen.add(session.id);
        return true;
      }));
    } catch (error) {
      if (lifetime !== observationLifetime) return;
      if (ticket === sessionCatalogRequest && error.status === 401) expireBrowserAccess();
      else if (ticket === sessionCatalogRequest && error.status === 403) clearAgentViews("Session read access changed.");
    }
  }

  async function connectWorkspace(lifetime = observationLifetime) {
    const owner = snapshot()?.instance;
    const discovery = workspaceDiscoveryLifetime;
    try {
      const value = await api("/api/workspace");
      if (lifetime !== observationLifetime || discovery !== workspaceDiscoveryLifetime || owner !== snapshot()?.instance) return;
      const enabled = Boolean(value?.enabled);
      if (enabled) {
        if (value.instance !== owner) return;
        workspaceViews.setSource(value.capabilities);
        setWorkspaceSource({ source_id: value.capabilities.source_id, view_root: value.capabilities.view_root });
      } else clearWorkspaceViews();
      setWorkspaceEnabled(enabled);
      if (!enabled && nav() !== "sessions") setNav("sessions");
    } catch (error) {
      if (lifetime !== observationLifetime || discovery !== workspaceDiscoveryLifetime || owner !== snapshot()?.instance) return;
      if (error.status === 401) expireBrowserAccess();
      else if (error.status === 403) clearWorkspaceViews();
      if (!workspaceEnabled() && nav() !== "sessions") setNav("sessions");
    }
  }

  const presenceClientId = "web-" + crypto.randomUUID();

  function detachSharedDraft({ keepText = true, message = "" } = {}) {
    const text = sharedClient?.currentText?.() ?? prompt();
    const origin = sharedDraftOrigin;
    sharedClient?.destroy?.();
    sharedClient = null;
    sharedDraftOrigin = null;
    setSharedDraft(null);
    setSharedPresence([]);
    setPublishChoice(false);
    if (keepText) setPrompt(text, origin);
    if (message) setError(message);
  }

  async function leaveSharedDraft(keepText = true) {
    const current = sharedDraft();
    if (current) {
      try {
        await api("/api/team/draft/presence", {
          draft_id: current.draft_id,
          generation: current.generation,
          client_id: presenceClientId,
          cursor_anchor: 0,
          cursor_head: 0,
          active: false,
        });
      } catch {}
    }
    detachSharedDraft({ keepText });
  }

  async function ensureSharedClient() {
    if (sharedClient) return sharedClient;
    const module = await import("./multiplayer.js");
    sharedClient = module.createSharedDraftClient({
      api,
      onText: value => setPrompt(value, sharedDraftOrigin),
      onState: value => {
        if (sharedDraftOrigin && value.target) {
          sharedDraftOrigin = draftTarget({ instance: sharedDraftOrigin.instance, saved: value.target });
          if (prompt().trim()) setDraftOrigin(sharedDraftOrigin);
        }
        setSharedDraft(value);
      },
      onError: error => {
        const text = sharedClient?.currentText?.() ?? prompt();
        const origin = sharedDraftOrigin;
        sharedClient?.destroy?.();
        sharedClient = null;
        sharedDraftOrigin = null;
        setSharedDraft(null);
        setSharedPresence([]);
        setPrompt(text, origin);
        setError(
          error?.status === 409
            ? "The shared prompt changed or was sealed. Your local edit is still here as a private draft."
            : (error?.message || "Shared draft update failed; your local text was kept private."),
        );
      },
    });
    return sharedClient;
  }

  async function loadPresence(lifetime = observationLifetime) {
    const current = sharedDraft();
    if (!current) {
      setSharedPresence([]);
      return;
    }
    try {
      const value = await api("/api/team/draft/presence/list", {
        draft_id: current.draft_id,
        generation: current.generation,
      });
      if (lifetime !== observationLifetime) return;
      setSharedPresence(value.presence || []);
    } catch {
      if (lifetime !== observationLifetime) return;
      setSharedPresence([]);
    }
  }

  async function publishPresence(active = true) {
    const current = sharedDraft();
    if (!current || !sharedClient?.active?.()) return;
    const anchor = active ? (composerTextarea?.selectionStart ?? 0) : 0;
    const head = active ? (composerTextarea?.selectionEnd ?? anchor) : 0;
    try {
      await api("/api/team/draft/presence", {
        draft_id: current.draft_id,
        generation: current.generation,
        client_id: presenceClientId,
        cursor_anchor: anchor,
        cursor_head: head,
        active,
      });
      presenceLastSent = Date.now();
    } catch {}
  }

  function queuePresence() {
    if (!sharedActive()) return;
    const elapsed = Date.now() - presenceLastSent;
    if (elapsed >= 150) {
      void publishPresence(true);
      return;
    }
    clearTimeout(presencePending);
    presencePending = setTimeout(() => void publishPresence(true), 150 - elapsed);
  }

  async function beginCollaboration({ publishPrivate = false } = {}) {
    if (!teamSharing() || sharedActive() || draftRetargetRequired()) return;
    const privateText = prompt();
    const origin = draftOrigin();
    const target = draftTarget();
    const lifetime = observationLifetime;
    if (!publishPrivate && needsDraftPublicationConsent(privateText, sharedActive())) {
      setPublishChoice(true);
      setCollabPanel(true);
      return;
    }
    setError("");
    try {
      const opened = await api("/api/team/draft/open", { activate_auto: true });
      const client = await ensureSharedClient();
      const openedTarget = draftTarget({ instance: target.instance, saved: opened.target });
      if (lifetime !== observationLifetime || prompt() !== privateText
        || !sameDraftTarget(origin, draftOrigin()) || !sameDraftTarget(target, draftTarget())
        || !sameDraftTarget(target, openedTarget)) {
        throw new Error("The active Agent or draft changed. Review the draft before sharing it here.");
      }
      sharedDraftOrigin = openedTarget;
      client.install(opened);
      if (publishPrivate && privateText) {
        client.setText(privateText);
        await client.flush();
      }
      setPublishChoice(false);
      setCollabPanel(true);
      await loadPresence();
      queuePresence();
    } catch (error) {
      setError(error.message);
    }
  }

  async function setCollaborationMode(mode) {
    try {
      const value = await api("/api/team/session/mode", { mode });
      setCollaboration(current => ({ ...(current || {}), policy: value.policy }));
      if (mode === "off" && sharedActive()) await leaveSharedDraft(true);
      await Promise.allSettled([loadCollaboration(), loadSessions()]);
    } catch (error) {
      setError(error.message);
    }
  }

  async function setParticipantShare(participantId, enabled) {
    try {
      const currentMode = collaboration()?.policy?.mode || "off";
      const value = await api("/api/team/session/share", {
        participant_id: participantId,
        enabled,
        mode: enabled && currentMode === "off" ? "on" : undefined,
      });
      if (value.policy) {
        setCollaboration(current => ({ ...(current || {}), policy: value.policy }));
      }
      await Promise.allSettled([loadCollaboration(), loadSessions()]);
    } catch (error) {
      setError(error.message);
    }
  }

  function connectEvents(lifetime = observationLifetime) {
    if (lifetime !== observationLifetime) return;
    const previous = source;
    source = null;
    previous?.close();
    const connection = new EventSource("/api/events");
    source = connection;
    const current = () => lifetime === observationLifetime && source === connection;
    const listen = (name, callback) => connection.addEventListener(name, event => { if (current()) callback(event); });
    connection.onopen = () => { if (current()) { setOnline(true); setConnection("Connected"); } };
    connection.onerror = () => { if (current()) { setOnline(false); setConnection("Reconnecting…"); } };
    listen("snapshot", event => {
      try {
        setSnapshot(current => acceptSnapshot(current, JSON.parse(event.data)));
        setPaired(true);
        void detectTeam(lifetime).then(() => {
          if (current()) return Promise.allSettled([loadSessions(lifetime), loadCollaboration(lifetime)]);
        });
      } catch {
        void refresh();
      }
    });
    listen("changed", () => void refresh(false));
    listen("context_revoked", () => {
      clearAgentViews("Read access changed; reopen permitted views.");
      clearWorkspaceViews();
      void refresh(false);
    });
    listen("delta", event => {
      try {
        const next = acceptDelta(snapshot(), Number(event.lastEventId), JSON.parse(event.data));
        if (next.resync) void refresh(false);
        else if (next.applied) setSnapshot(next.snapshot);
      } catch {
        void refresh(false);
      }
    });
    listen("draft_changed", event => {
      try {
        const value = JSON.parse(event.data);
        if (sharedClient?.applyRemote?.(value)) {
          setSharedDraft(current => current && Number.isSafeInteger(value.accepted_revision)
            ? { ...current, accepted_revision: Math.max(current.accepted_revision, value.accepted_revision) }
            : current);
        }
      } catch {}
    });
    listen("draft_presence_changed", () => {
      if (sharedActive()) void loadPresence(lifetime);
    });
    listen("draft_scope_changed", () => {
      void Promise.allSettled([loadCollaboration(lifetime), loadSessions(lifetime)]);
      if (sharedActive()) {
        void sharedClient.refresh().catch(() => {
          if (!current()) return;
          detachSharedDraft({
            keepText: true,
            message: "Collaboration access changed. Your local text remains private.",
          });
        });
      }
    });
    listen("draft_submitted", () => {
      if (sharedActive()) void sharedClient.refresh().catch(() => undefined);
    });
    listen("draft_settled", () => {
      const saved = snapshot()?.saved;
      if (!saved) return;
      const shared = Boolean(currentSession()?.shared);
      void openSession(saved.session_id, saved.branch_name || saved.branch_id, shared, {
        preserveDraft: false,
        quiet: true,
      });
    });
    listen("auth_expired", expireBrowserAccess);
  }

  async function loadPresentation() {
    try {
      const config = await api("/api/settings");
      if (config?.ui) setPresentationUi(normalizedUi(config.ui));
    } catch {}
  }

  async function refresh(reconnect = true) {
    let lifetime = observationLifetime;
    try {
      const next = await api("/api/session");
      if (lifetime !== observationLifetime) return;
      setSnapshot(current => acceptSnapshot(current, next));
      setPaired(true);
      setError("");
      setConnection("Connected");
      setOnline(true);
      await detectTeam(lifetime);
      if (lifetime !== observationLifetime) return;
      await Promise.allSettled([loadSessions(lifetime), connectWorkspace(lifetime), loadCollaboration(lifetime)]);
      if (lifetime !== observationLifetime) return;
      if (reconnect) connectEvents(lifetime);
    } catch (e) {
      if (lifetime !== observationLifetime) return;
      if (e.status === 401) {
        expireBrowserAccess();
        lifetime = observationLifetime;
        try {
          const config = await api("/api/settings");
          if (lifetime !== observationLifetime) return;
          if (config?.local_single_user) {
            if (config.ui) setPresentationUi(normalizedUi(config.ui));
            const next = await api("/api/session");
            if (lifetime !== observationLifetime) return;
            setSnapshot(next);
            setPaired(true);
            setSettings(!next?.identity);
            connectEvents(lifetime);
            await detectTeam(lifetime);
            if (lifetime !== observationLifetime) return;
            await Promise.allSettled([loadSessions(lifetime), connectWorkspace(lifetime), loadCollaboration(lifetime)]);
            return;
          }
        } catch {
          if (lifetime !== observationLifetime) return;
        }
        setPaired(false);
        setConnection("Not paired");
        setOnline(false);
      } else {
        setError(e.message);
      }
    }
  }

  onMount(() => {
    const pulse = setInterval(pulseAgentViews, 500);
    const wake = () => pulseAgentViews();
    document.addEventListener("visibilitychange", wake);
    window.addEventListener("focus", wake);
    onCleanup(() => {
      clearInterval(pulse);
      document.removeEventListener("visibilitychange", wake);
      window.removeEventListener("focus", wake);
    });
    void loadPresentation();
    finishBoot();
    void refresh().then(() => void receivePendingDocumentDock());
  });
  onCleanup(() => {
    documentDocking.dispose();
    presentationControllers.clear();
    observationLifetime = {};
    const previous = source;
    source = null;
    previous?.close();
    agentViews.dispose();
    workspaceViews.dispose();
    clearTimeout(presencePending);
    if (sharedActive()) void publishPresence(false);
    sharedClient?.destroy?.();
  });

  async function pair(event) {
    event.preventDefault();
    setError("");
    try {
      await api("/api/pair", { code: pairCode().trim() });
      setPairCode("");
      await refresh();
      await receivePendingDocumentDock();
    } catch (e) {
      setError(e.message);
    }
  }

  function showNav(kind) {
    if (kind !== "sessions" && !workspaceEnabled()) {
      setError("Start aw with workspace surfaces enabled to browse Files or Git.");
      return false;
    }
    setNav(kind);
    if (kind !== "sessions" && !selectedContainers()[kind]) {
      try { ensureWorkspaceContainer(kind); }
      catch (error) { setError(error.message); }
    }
    setMobileNav(false);
    return true;
  }

  function ensureWorkspaceContainer(kind) {
    const current = selectedContainers()[kind];
    if (current && workspaceViews.getContainer(current)) return current;
    const id = workspaceViews.openContainer(kind);
    setSelectedContainers(value => ({ ...value, [kind]: id }));
    return id;
  }

  async function newWorkspaceContainer(kind) {
    const lifetime = observationLifetime;
    const owner = snapshot()?.instance;
    try {
      if (!workspaceSource()) await connectWorkspace();
      if (lifetime !== observationLifetime || owner !== snapshot()?.instance) return false;
      const id = workspaceViews.openContainer(kind);
      setSelectedContainers(value => ({ ...value, [kind]: id }));
      return id;
    } catch (error) { setError(error.message); return false; }
  }

  function closeWorkspaceContainer(kind) {
    const id = selectedContainers()[kind];
    if (id) workspaceViews.closeContainer(id);
  }

  async function searchWorkspaceFiles(id, query, { signal }) {
    if (workspaceViews.getContainer(id)?.kind !== "files") throw new Error("The Files view closed. Reopen the palette to choose another view.");
    const result = await workspaceViews.searchFiles(id, query, 40, { signal });
    return { entries: (result?.entries || []).map(entry => ({ ...entry, containerId: id })) };
  }

  function showChat() {
    setActiveTab("chat");
    setMobileNav(false);
    return true;
  }

  function focusPaletteSurface(target) {
    queueMicrotask(() => {
      if (target === "chat") composerTextarea?.focus();
      else if (target === "tab") tabTargets.get(activeTab())?.focus();
      else if (target === "settings") document.querySelector('.modal[role="dialog"] button')?.focus();
      else if (target === "nav") {
        if (window.matchMedia("(max-width: 820px)").matches) setMobileNav(true);
        document.querySelector('.scoped-navigator:not([hidden]) .navigator input, .scoped-navigator:not([hidden]) .navigator button')?.focus();
      }
    });
  }

  function upsertTab(tab) {
    setTabs(current => {
      const index = current.findIndex(item => item.id === tab.id);
      if (index < 0) return [...current, tab];
      const next = current.slice();
      next[index] = { ...next[index], ...tab };
      return next;
    });
    setActiveTab(tab.id);
  }

  function closeTab(id) {
    if (id === "chat") return;
    const current = tabs();
    const wasActive = activeTab() === id;
    const index = current.findIndex(tab => tab.id === id);
    if (agentViews.get(id)) agentViews.close(id);
    if (workspaceViews.getDocument(id)) workspaceViews.closeDocument(id);
    const next = current.filter(tab => tab.id !== id);
    setTabs(next);
    tabTargets.delete(id);
    if (wasActive) {
      const replacement = next[Math.min(index, next.length - 1)] || next[0];
      setActiveTab(replacement?.id || "chat");
    }
    tabTargets.get(activeTab())?.focus();
  }

  function onTabKeyDown(event, id) {
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    const current = tabs();
    const index = current.findIndex(tab => tab.id === id);
    if (index < 0) return;
    let next;
    switch (event.key) {
      case "ArrowLeft": next = (index + current.length - 1) % current.length; break;
      case "ArrowRight": next = (index + 1) % current.length; break;
      case "Home": next = 0; break;
      case "End": next = current.length - 1; break;
      default: return;
    }
    event.preventDefault();
    setActiveTab(current[next].id);
    tabTargets.get(current[next].id)?.focus();
  }

  function startSidebarResize(event) {
    if (window.matchMedia("(max-width: 820px)").matches) return;
    resizingSidebar = true;
    event.currentTarget.setPointerCapture?.(event.pointerId);
    document.body.classList.add("resizing-sidebar");
  }

  function moveSidebarResize(event) {
    if (!resizingSidebar) return;
    const width = Math.min(520, Math.max(220, event.clientX));
    setSidebarWidth(width);
  }

  function stopSidebarResize(event) {
    if (!resizingSidebar) return;
    resizingSidebar = false;
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    document.body.classList.remove("resizing-sidebar");
    localStorage.setItem("aw.sidebar.width", String(sidebarWidth()));
  }

  async function openSession(sessionId, branch, shared = false, options = {}) {
    const current = snapshot();
    if (!current) return false;
    if (current.saved?.session_id === sessionId &&
        (current.saved.branch_id === branch || current.saved.branch_name === branch)) {
      return showChat();
    }
    const preserveDraft = options.preserveDraft !== false;
    const original = prompt();
    const origin = draftOrigin();
    const target = draftTarget(current);
    const lifetime = observationLifetime;
    const consent = !options.quiet && Boolean(original.trim());
    if (consent && !confirm("Open another branch and keep this unsent draft text?")) return false;
    try {
      if (sharedActive()) await leaveSharedDraft(preserveDraft);
      const path = shared ? "/api/team/session/open-shared" : "/api/session/open";
      const next = await api(path, {
        expected_revision: current.revision,
        session_id: sessionId,
        branch,
      });
      if (lifetime !== observationLifetime) return false;
      const keepConsent = consent && prompt() === original
        && sameDraftTarget(origin, draftOrigin())
        && (sameDraftTarget(target, draftTarget()) || sameDraftTarget(draftTarget(next), draftTarget()));
      const installed = setSnapshot(value => acceptSnapshot(value, next));
      if (keepConsent && sameDraftTarget(draftTarget(next), draftTarget(installed))) {
        setDraftOrigin(draftTarget(installed));
      }
      setActiveTab("chat");
      setMobileNav(false);
      await Promise.allSettled([loadSessions(), loadCollaboration()]);
      return true;
    } catch (e) {
      setError(e.message);
      return false;
    }
  }

  async function newChat() {
    const current = snapshot();
    if (!current || current.pending) return false;
    const original = prompt();
    const origin = draftOrigin();
    const lifetime = observationLifetime;
    if (original.trim() && !confirm("Start a new chat? Your unsent draft will be cleared.")) return false;
    try {
      const next = await api("/api/reset", { expected_revision: current.revision });
      if (lifetime !== observationLifetime) return false;
      if (prompt() === original && draftOrigin() === origin) setPrompt("");
      setSnapshot(next);
      showChat();
      await loadSessions();
      return true;
    } catch (e) {
      setError(e.message);
      return false;
    }
  }

  async function send(event) {
    event?.preventDefault();
    const current = snapshot();
    if (!current || draftRetargetRequired()) return;
    if (currentSession()?.shared && !sharedActive()) {
      await beginCollaboration();
      return;
    }
    if (!canSend(current, posting(), prompt())) return;
    setPosting(true);
    setError("");
    try {
      if (sharedActive()) {
        const confirmed = await sharedClient.flush();
        if (confirmed.text !== prompt()) {
          throw new Error("Shared draft changed while sending; review the merged text and send again.");
        }
        const selected = account();
        const body = {
          expected_revision: current.revision,
          draft_id: confirmed.draft_id,
          generation: confirmed.generation,
          document_epoch: confirmed.document_epoch,
          accepted_revision: confirmed.accepted_revision,
          metadata_revision: confirmed.metadata_revision,
          digest_sha256: confirmed.candidate_sha256,
        };
        if (selected) {
          body.account_id = selected.account_id;
          body.model = selected.model;
        }
        await api("/api/team/draft/submit", body, 120000);
        await leaveSharedDraft(false);
        setPrompt("");
      } else {
        const body = submission(current, prompt(), crypto.randomUUID(), account());
        const original = prompt();
        const origin = draftOrigin();
        await api("/api/prompts", body, 120000);
        if (prompt() === original && draftOrigin() === origin) setPrompt("");
      }
      await refresh(false);
    } catch (e) {
      setError(e.message);
    } finally {
      setPosting(false);
    }
  }

  async function cancel() {
    if (!snapshot()?.pending) return;
    try {
      await api("/api/cancel", { run_id: snapshot().pending.run_id });
    } catch (e) {
      setError(e.message);
    }
  }

  function openReadonly(session, branch) {
    const source = viewSource();
    if (!source || session.shared || !branch.id) return false;
    try {
      workspaceViews.assertCanOpenDocument();
      const id = agentViews.open({ ...source, session_id: session.id, branch_id: branch.id });
      upsertTab({ id, kind: "agent-view", title: "Read-only view" });
      setMobileNav(false);
      void agentViews.refresh(id);
      return true;
    } catch (error) {
      setError(error.message);
      return false;
    }
  }

  function openFile(containerId, path) {
    try {
      const id = workspaceViews.openFile(containerId, path);
      setActiveTab(id);
      setMobileNav(false);
      return true;
    } catch (e) {
      setError(e.message);
      return false;
    }
  }

  function openReview(containerId, origin) {
    try {
      const id = workspaceViews.openReview(containerId, origin);
      setActiveTab(id);
      setMobileNav(false);
      return true;
    } catch (error) { setError(error.message); return false; }
  }

  const title = createMemo(() => snapshot()?.saved?.title || "New Agent");

  return (
    <Show
      when={paired()}
      fallback={
        <main class="pair-screen">
          <form class="pair-card" onSubmit={pair}>
            <Mark />
            <h1>Connect to aw</h1>
            <p>Use the one-time pairing code from your local aw owner. Provider credentials never enter this browser.</p>
            <label class="field">
              <span>Pairing code</span>
              <input type="password" value={pairCode()} onInput={event => setPairCode(event.currentTarget.value)} autocomplete="off" />
            </label>
            <button class="primary" type="submit">Connect</button>
            <Show when={error()}><div class="error">{error()}</div></Show>
          </form>
        </main>
      }
    >
      <div class="shell" style={{ "--sidebar-width": String(sidebarWidth()) + "px" }}>
        <aside class={"sidebar " + (mobileNav() ? "open" : "")}>
          <div class="brand">
            <Mark />
            <span>aw</span>
            <small>by abird</small>
          </div>
          <div class="new-row">
            <button class="new-button" onClick={newChat}>
              <Icon name="plus" size={14} />
              <span>New</span>
            </button>
          </div>
          <nav class="nav-tabs" aria-label="Explorer">
            <For each={[
              ["sessions", "Sessions", "chat"],
              ["files", "Files", "file"],
              ["git", "Git", "git"],
            ]}>
              {item => (
                <button
                  class={"nav-tab " + (nav() === item[0] ? "active" : "")}
                  onClick={() => showNav(item[0])}
                  disabled={item[0] !== "sessions" && !workspaceEnabled()}
                >
                  <Icon name={item[2]} size={13} />
                  <span>{item[1]}</span>
                </button>
              )}
            </For>
          </nav>
          <div class="sidebar-body">
            <div class="scoped-navigator" hidden={nav() !== "sessions"}>
              <SessionNavigator sessions={sessions()} snapshot={snapshot()} openSession={openSession} openReadonly={openReadonly} canInspect={Boolean(viewSource())} />
            </div>
            <Show when={nav() !== "sessions"}>
              <div class="navigator-views">
                <select aria-label={nav() === "files" ? "Files views" : "Git views"} value={selectedContainers()[nav()] || ""}
                  disabled={!workspaceContainers().some(container => container.kind === nav())}
                  onChange={event => {
                    const id = event.currentTarget.value;
                    if (workspaceViews.getContainer(id)?.kind === nav()) setSelectedContainers(value => ({ ...value, [nav()]: id }));
                  }}>
                  <Show when={!workspaceContainers().some(container => container.kind === nav())}><option value="">No open views</option></Show>
                  <For each={workspaceContainers().filter(container => container.kind === nav())}>
                    {(container, index) => <option value={container.id}>{index() + 1}. {container.label}</option>}
                  </For>
                </select>
                <button class="mini-icon" type="button" aria-label={nav() === "files" ? "New Files view" : "New Git view"}
                  title="Open another independent view" onClick={() => void newWorkspaceContainer(nav())}><Icon name="plus" size={13} /></button>
                <button class="mini-icon" type="button" aria-label={nav() === "files" ? "Close Files view and its documents" : "Close Git view and its documents"}
                  title="Close this view and its documents" disabled={!selectedContainers()[nav()]} onClick={() => closeWorkspaceContainer(nav())}><Icon name="close" size={13} /></button>
              </div>
              <Show when={!selectedContainers()[nav()]}>
                <div class="navigator-empty">Open a {nav() === "files" ? "Files" : "Git"} view to browse independently.</div>
              </Show>
            </Show>
            <For each={workspaceContainers().map(container => container.id)}>
              {id => {
                const kind = workspaceViews.getContainer(id).kind;
                const state = createMemo(() => workspaceContainer(id));
                return <div class="scoped-navigator" hidden={nav() !== kind || selectedContainers()[kind] !== id}>
                  <Show when={kind === "files"} fallback={<GitNavigator id={id} state={state()} controller={workspaceViews} fail={setError} openReview={openReview} />}>
                    <FilesNavigator id={id} state={state()} controller={workspaceViews} fail={setError} openFile={openFile} activePath={activeFilePath(id)} />
                  </Show>
                </div>;
              }}
            </For>
          </div>
          <div class="sidebar-footer">
            <button class="quiet settings-link" onClick={() => setSettings(true)}>
              <Icon name="settings" size={14} />
              <span>Settings</span>
            </button>
          </div>
        </aside>

        <div
          class="sidebar-resizer"
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize Explorer"
          onPointerDown={startSidebarResize}
          onPointerMove={moveSidebarResize}
          onPointerUp={stopSidebarResize}
          onPointerCancel={stopSidebarResize}
        />

        <Show when={mobileNav()}>
          <button class="mobile-scrim" aria-label="Close Explorer" onClick={() => setMobileNav(false)} />
        </Show>

        <main class="main">
          <header class="workspace-chrome">
            <div class="command-row">
              <button class="icon-button mobile-nav" onClick={() => setMobileNav(true)} aria-label="Open Explorer">
                ☰
              </button>
              <CommandBar
                workspaceEnabled={workspaceEnabled()}
                sessions={sessions()}
                tabs={tabs().map(tab => ({ ...tab, title: tabTitle(tab), path: tabDescription(tab) }))}
                agent={{ title: agentTabTitle() }}
                newChat={newChat}
                chat={showChat}
                nav={showNav}
                settings={() => setSettings(true)}
                activateTab={setActiveTab}
                openSession={openSession}
                openFile={openFile}
                searchFiles={searchWorkspaceFiles}
                fileSearchContainer={() => ensureWorkspaceContainer("files")}
                workspaceToken={workspaceToken()}
                focusSurface={focusPaletteSurface}
              />
              <span class="connection" title={connection()}>
                <i class={"connection-dot " + (online() ? "online" : "")} />
                <span>{connection()}</span>
              </span>
              <button class="icon-button chrome-settings" onClick={() => setSettings(true)} aria-label="Settings">
                <Icon name="settings" size={14} />
              </button>
            </div>
            <div class="document-tabs">
              <div class="tab-strip" role="tablist" aria-label="Workspace documents" aria-orientation="horizontal">
                <For each={tabs()}>
                  {tab => (
                    <div class={"doc-tab " + (activeTab() === tab.id ? "active" : "")} role="presentation">
                      <button
                        ref={element => tabTargets.set(tab.id, element)}
                        id={tabElementId(tab.id)}
                        class="doc-tab-target"
                        role="tab"
                        aria-selected={activeTab() === tab.id}
                        aria-label={tabDescription(tab)}
                        aria-controls="workspace-document-panel"
                        tabIndex={activeTab() === tab.id ? 0 : -1}
                        onClick={() => setActiveTab(tab.id)}
                        onKeyDown={event => onTabKeyDown(event, tab.id)}
                        title={tabDescription(tab)}
                      >
                        <Icon name={tab.kind === "chat" || tab.kind === "agent-view" ? "chat" : tab.kind === "review" ? "git" : "file"} size={13} />
                        <span>{tabTitle(tab)}</span>
                      </button>
                      <Show when={tab.id !== "chat"}>
                        <button class="doc-tab-close" tabIndex={activeTab() === tab.id ? 0 : -1} onClick={() => closeTab(tab.id)} aria-label={"Close " + tabTitle(tab)}>
                          <Icon name="close" size={11} />
                        </button>
                      </Show>
                    </div>
                  )}
                </For>
              </div>
              <div class="model-state" title="Current provider profile and model">
                <span>{snapshot()?.profile || "not configured"}</span>
                <span class="model-state-dot">·</span>
                <span>{snapshot()?.model || "choose a model"}</span>
              </div>
            </div>
          </header>

          <Show when={error()}>
            <div class="notice">
              <span>{error()}</span>
              <button class="notice-close" onClick={() => setError("")} aria-label="Dismiss">
                <Icon name="close" size={11} />
              </button>
            </div>
          </Show>

          <div class="workspace-document-panel" id="workspace-document-panel" role="tabpanel" aria-labelledby={tabElementId(activeTab())}>
            <div class="current-agent-panel" hidden={surface().kind !== "chat"}>
              <div class="chat-scroll">
                <div class="chat">
                  <ResourceHeader snapshot={snapshot()} />
                  <Show
                    when={(snapshot()?.turns?.length || 0) + (snapshot()?.context_blocks?.length || 0) > 0 || snapshot()?.pending}
                    fallback={
                      <div class="welcome">
                        <div>
                          <Mark />
                          <h1>What are we working on?</h1>
                          <p>Work with your Agent naturally, inspect the workspace when you need it, and bring files or review feedback into context only when you explicitly choose to.</p>
                        </div>
                      </div>
                    }
                  >
                    <For each={snapshot()?.turns || []}>
                      {(turn, index) => (
                        <>
                          <For each={(snapshot()?.context_blocks || []).filter(block => block.before_turn === index())}>
                            {block => <ContextCard block={block} />}
                          </For>
                          <Message role="user" text={turn.prompt} />
                          <Message role="assistant" text={turn.answer} outcome={turn.outcome} tools={turn.tool_records} showTools={presentationUi().show_tool_calls} showThinking={presentationUi().show_thinking} />
                        </>
                      )}
                    </For>
                    <For each={(snapshot()?.context_blocks || []).filter(block => block.before_turn === (snapshot()?.turns?.length || 0))}>
                      {block => <ContextCard block={block} />}
                    </For>
                    <Show when={snapshot()?.pending}>
                      <Message role="user" text={snapshot().pending.prompt} />
                      <Message role="assistant" text={snapshot().pending.answer} outcome="streaming" tools={snapshot().pending.tool_records} showTools={presentationUi().show_tool_calls} showThinking={presentationUi().show_thinking} />
                    </Show>
                  </Show>
                </div>
              </div>

              <div class="composer-dock">
                <Show when={collabPanel() && teamSharing()}>
                  <CollaborationPanel
                    collaboration={collaboration()}
                    collaborators={collaborators()}
                    draft={sharedDraft()}
                    presence={sharedPresence()}
                    active={sharedActive()}
                    sharedSession={Boolean(currentSession()?.shared)}
                    publishChoice={publishChoice()}
                    setMode={setCollaborationMode}
                    share={setParticipantShare}
                    join={() => beginCollaboration()}
                    publishPrivate={() => beginCollaboration({ publishPrivate: true })}
                    keepPrivate={() => {
                      setPublishChoice(false);
                      setCollabPanel(false);
                    }}
                    leave={() => leaveSharedDraft(true)}
                    close={() => setCollabPanel(false)}
                  />
                </Show>
                <Show when={draftRetargetRequired()}>
                  <div class="notice" role="status">
                    <span>The active Agent changed to {title()} · {snapshot()?.saved?.branch_name || "new chat"}. Your unsent draft is still here.</span>
                    <button class="quiet" type="button" onClick={useDraftHere}>Use draft here</button>
                  </div>
                </Show>
                <form class="composer" onSubmit={send}>
                  <textarea
                    ref={composerTextarea}
                    value={prompt()}
                    onInput={event => {
                      if (sharedActive()) sharedClient.setText(event.currentTarget.value);
                      else setPrompt(event.currentTarget.value);
                      queuePresence();
                    }}
                    onSelect={queuePresence}
                    onFocus={queuePresence}
                    onBlur={() => {
                      clearTimeout(presencePending);
                      if (sharedActive()) void publishPresence(false);
                    }}
                    onKeyDown={event => {
                      if (isSubmitKey(event)) {
                        event.preventDefault();
                        void send();
                      }
                    }}
                    placeholder={snapshot()?.identity ? "Message aw…" : "Configure a provider in Settings…"}
                    disabled={posting() || (sharedActive() && sharedDraft()?.state !== "active")}
                  />
                  <AgentStatus snapshot={snapshot()} ui={presentationUi()} />
                  <div class="composer-foot">
                    <Show when={choices().length}>
                      <select
                        class="model-select"
                        value={account() ? account().account_id + "::" + account().model : ""}
                        onChange={event => {
                          const index = event.currentTarget.value.indexOf("::");
                          const accountId = event.currentTarget.value.slice(0, index);
                          const model = event.currentTarget.value.slice(index + 2);
                          setAccount(choices().find(choice => choice.account_id === accountId && choice.model === model) || null);
                        }}
                      >
                        <For each={choices()}>
                          {choice => (
                            <option value={choice.account_id + "::" + choice.model}>
                              {choice.model} · {choice.account_id}
                            </option>
                          )}
                        </For>
                      </select>
                    </Show>
                    <Show when={teamSharing()}>
                      <button
                        class={"quiet collab-toggle " + (sharedActive() ? "active" : "")}
                        type="button"
                        onClick={() => {
                          const opening = !collabPanel();
                          setCollabPanel(opening);
                          if (opening) void loadCollaboration();
                        }}
                      >
                        {sharedActive() ? "Shared" : currentSession()?.shared ? "Collaborate" : "Share"}
                      </button>
                    </Show>
                    <span class="composer-hint">Enter send · Shift+Enter newline</span>
                    <Show
                      when={snapshot()?.pending}
                      fallback={<button class="send" type="submit" disabled={draftRetargetRequired() || !canSend(snapshot(), posting(), prompt())}>Send ↑</button>}
                    >
                      <button class="quiet stop" type="button" onClick={cancel}>Stop</button>
                    </Show>
                  </div>
                </form>
              </div>
            </div>

            <For each={pinnedIds()}>
              {id => {
                const view = createMemo(() => pinnedView(id));
                const data = createMemo(() => view()?.snapshot);
                return (
                  <AgentViewPanel id={id} active={activeTab() === id} snapshot={data()} pending={view()?.pending} error={view()?.error}
                    find={view()?.find} viewport={view()?.viewport}
                    onFind={find => agentViews.updateView(id, { find })} onViewport={viewport => agentViews.updateView(id, { viewport })}
                    onRefresh={() => void agentViews.refresh(id)} onCheckAccess={() => Boolean(agentViews.get(id)?.snapshot)}>
                    <For each={data()?.turns || []}>
                      {(turn, index) => <>
                        <For each={(data()?.context_blocks || []).filter(block => block.before_turn === index())}>{block => <ContextCard block={block} />}</For>
                        <Message role="user" text={turn.prompt} />
                        <Message role="assistant" text={turn.answer} outcome={turn.outcome} tools={turn.tool_records} showTools={presentationUi().show_tool_calls} showThinking={false} />
                      </>}
                    </For>
                    <For each={(data()?.context_blocks || []).filter(block => block.before_turn === (data()?.turns?.length || 0))}>{block => <ContextCard block={block} />}</For>
                  </AgentViewPanel>
                );
              }}
            </For>

            <For each={workspaceDocumentIds()}>
              {id => {
                const document = createMemo(() => workspaceDocument(id));
                const data = createMemo(() => document()?.data);
                const checkAccess = () => Boolean(workspaceViews.getDocument(id)?.data);
                const destinationKind = () => document()?.kind === "file" ? "files" : "git";
                const destinations = createMemo(() => workspaceContainers().filter(
                  container => container.kind === destinationKind() && !container.unavailable
                ));
                const moveDocument = event => {
                  const origin = document()?.containerId;
                  const target = event.currentTarget.value;
                  try {
                    if (!workspaceViews.moveDocument(id, target)) return;
                    setSelectedContainers(current => ({ ...current, [destinationKind()]: target }));
                  } catch (error) {
                    setError(error.message);
                    event.currentTarget.value = origin;
                  }
                };
                return <div class="surface code-surface" hidden={activeTab() !== id}>
                  <div class="surface-bar" aria-label="Document source and access" title={data()?.path || ""}>
                    <span class="surface-path">{data() ? [document()?.kind === "review" ? data().root : "", dirname(data().path), data().basis].filter(Boolean).join(" · ") || "." : "Read-only document"}</span>
                    <span class="surface-badge">{document()?.kind === "review" ? "diff · " : ""}read only</span>
                    <Show when={destinations().length > 1}>
                      <select class="surface-move-view"
                        aria-label={destinationKind() === "files" ? "Move document to another Files view" : "Move document to another Git view"}
                        title="Move this read-only tab without changing its source"
                        value={document()?.containerId || ""}
                        onChange={moveDocument}>
                        <For each={destinations()}>
                          {(container, index) => <option value={container.id}>{index() + 1}. {container.label}</option>}
                        </For>
                      </select>
                    </Show>
                    <Show when={["file", "review"].includes(document()?.kind) && data()
                      && !document()?.pending && !document()?.unavailable && workspaceEnabled()}>
                      <button class="mini-icon" type="button"
                        aria-label={document()?.kind === "review" ? "Move Review to a new window" : "Move File to a new window"}
                        title={document()?.kind === "review"
                          ? "Move this immutable Git Review into another authenticated browser window"
                          : "Move this read-only File into another authenticated browser window"}
                        disabled={Boolean(dockingDocument())}
                        onPointerDown={() => documentDocking.stage(id)}
                        onPointerLeave={() => documentDocking.cancelStage(id)}
                        onPointerCancel={() => documentDocking.cancelStage(id)}
                        onClick={() => void documentDocking.send(id).catch(error => setError(error.message))}>
                        <Icon name="popout" size={13} />
                      </button>
                    </Show>
                    <button class="mini-icon" type="button" aria-label="Refresh document" title="Refresh this exact document" onClick={() => void workspaceViews.refreshDocument(id)}><Icon name="refresh" size={13} /></button>
                  </div>
                  <Show when={document()?.pending}><div class="document-state" role="status">Loading…</div></Show>
                  <Show when={document()?.error}><div class="document-state" role="status">{document().error}</div></Show>
                  <Show when={data()} fallback={<div class="navigator-empty">{document()?.pending ? "" : "This document is unavailable. Refresh to try again."}</div>}>
                    <Show when={document()?.kind === "file"} fallback={<CodeDiff review={data()} active={activeTab() === id} onCheckAccess={checkAccess}
                      onPresentationReady={controller => bindPresentation(id, controller)} />}>
                      <CodeFile text={data()?.text || ""} path={data()?.path || ""} active={activeTab() === id} onCheckAccess={checkAccess}
                        onPresentationReady={controller => bindPresentation(id, controller)} />
                    </Show>
                  </Show>
                </div>;
              }}
            </For>
          </div>
        </main>
      </div>

      <Show when={settings()}>
        <Settings close={() => setSettings(false)} changed={async ui => { if (ui) setPresentationUi(normalizedUi(ui)); await refresh(false); }} />
      </Show>
    </Show>
  );
}

render(() => <App />, document.getElementById("root"));
