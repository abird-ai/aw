// Memory-only human views over the existing SurfaceHost. Descriptors request
// retained capabilities; labels and browser paths never create a grant.
export const WORKSPACE_VIEW_LIMITS = Object.freeze({
  containers: 8, documents: 32, pending: 4,
  replyBytes: 4 * 1024 * 1024, retainedBytes: 8 * 1024 * 1024,
  navigatorBytes: 64 * 1024, directories: 128,
});
const encoder = new TextEncoder();
const controls = /[\u0000-\u001f\u007f-\u009f]/u;
const bytes = value => encoder.encode(value).length;
const own = (value, key) => Object.hasOwn(value, key);
const defaultNavigator = () => Object.freeze({ query: "", view: "tree", expanded: Object.freeze([""]),
  collapsed: Object.freeze([]), mode: "changes", selected: null });

export function checkedWorkspaceSequence(previous) {
  if (!Number.isSafeInteger(previous) || previous < 0 || previous >= Number.MAX_SAFE_INTEGER) {
    throw new Error("Workspace view IDs exhausted; reload the browser.");
  }
  return previous + 1;
}
function text(value, limit, empty = false) {
  return typeof value === "string" && (empty || value.length > 0) && bytes(value) <= limit && !controls.test(value);
}
function id(value) { return text(value, 128) && /^[A-Za-z0-9_-]{1,128}$/u.test(value); }
function gitOrigin(value) {
  if (!value || Object.keys(value).length !== 2 || !own(value, "inventory_id") || !own(value, "item_id")
    || !id(value.inventory_id) || !id(value.item_id)) throw new Error("Invalid Git inventory target.");
  return Object.freeze({ inventory_id: value.inventory_id, item_id: value.item_id });
}
function path(value, empty = false) {
  return text(value, 4096, empty) && !value.includes("\\") && (value === "" || (!value.startsWith("/")
    && value.split("/").every(part => part && part !== "." && part !== "..")));
}
function freeze(value) {
  const pending = [[value, 0]];
  let count = 0;
  while (pending.length) {
    const [item, depth] = pending.pop();
    if (!item || typeof item !== "object") continue;
    if (depth > 64 || ++count > 65536) throw new Error("Workspace response structure exceeds its limit.");
    Object.freeze(item);
    for (const child of Object.values(item)) pending.push([child, depth + 1]);
  }
  return value;
}
function requestCopy(value) {
  if (!value || typeof value !== "object") throw new Error("Invalid workspace request.");
  const allowed = { files: ["path"], file: ["path"], file_search: ["query", "limit"],
    git_list: ["mode"], git_open: ["inventory_id", "item_id"], review: ["review_id"] }[value.op];
  if (!allowed || Object.keys(value).some(key => key !== "op" && !allowed.includes(key))) throw new Error("Invalid workspace request.");
  const result = { ...value };
  if (["files", "file"].includes(value.op) && !path(value.path, value.op === "files")) throw new Error("Invalid relative workspace path.");
  if (value.op === "file_search") {
    if (!text(value.query, 256) || !value.query.trim()) throw new Error("File search needs a bounded query.");
    result.limit ??= 120;
    if (!Number.isSafeInteger(result.limit) || result.limit < 1 || result.limit > 200) throw new Error("Invalid file search limit.");
  }
  if (value.op === "git_list" && !["changes", "worktrees", "branches"].includes(value.mode)) throw new Error("Invalid Git mode.");
  if (value.op === "git_open" && (!id(value.inventory_id) || !id(value.item_id))) throw new Error("Invalid Git inventory target.");
  if (value.op === "review" && !id(value.review_id)) throw new Error("Invalid immutable review target.");
  return Object.freeze(result);
}
function navigatorCopy(patch, current) {
  const allowed = ["query", "view", "expanded", "collapsed", "mode", "selected"];
  if (!patch || Object.keys(patch).some(key => !allowed.includes(key))) throw new Error("Invalid navigator state.");
  const next = { ...current, ...patch };
  if (!text(next.query, 256, true) || !["tree", "list"].includes(next.view)
    || !["changes", "worktrees", "branches"].includes(next.mode)
    || (next.selected !== null && !text(next.selected, 8192))
    || [next.expanded, next.collapsed].some(list => !Array.isArray(list) || list.length > 256
      || list.some(item => !text(item, 8192, true)) || new Set(list).size !== list.length)
    || bytes(JSON.stringify(next)) > WORKSPACE_VIEW_LIMITS.navigatorBytes) throw new Error("Navigator state exceeds its limit.");
  return freeze({ ...next, expanded: [...next.expanded], collapsed: [...next.collapsed] });
}
function replyCopy(value, target, request) {
  const encoded = JSON.stringify(value);
  if (typeof encoded !== "string" || bytes(encoded) > WORKSPACE_VIEW_LIMITS.replyBytes) throw new Error("Workspace reply exceeds the 4 MiB display limit.");
  const reply = JSON.parse(encoded);
  if (!reply || Object.keys(reply).length !== 3 || !own(reply, "result")
    || reply.source_id !== target.source_id || reply.scope_id !== target.scope_id) throw new Error("Workspace reply does not match its exact source and scope.");
  const r = reply.result;
  if (!r || typeof r !== "object" || Array.isArray(r)) throw new Error("Invalid workspace result.");
  const entriesValid = () => Array.isArray(r.entries) && r.entries.length <= 4096 && r.entries.every(entry =>
    entry && text(entry.name, 8192) && path(entry.path) && ["directory", "file", "unsupported"].includes(entry.kind));
  let valid = false;
  switch (request.op) {
    case "files": valid = r.context === target.scope_id && r.path === request.path && entriesValid() && typeof r.incomplete === "boolean"; break;
    case "file_search": valid = r.context === target.scope_id && r.query === request.query && entriesValid(); break;
    case "file": valid = r.context === target.scope_id && r.path === request.path && typeof r.text === "string" && r.read_only === true; break;
    case "git_list": valid = r.context === target.scope_id && r.mode === request.mode && id(r.inventory_id)
      && Array.isArray(r.items) && r.items.length <= 4096 && r.items.every(item => item && id(item.id)
        && typeof item.label === "string" && typeof item.key === "string" && ["change", "branch", "worktree"].includes(item.kind)); break;
    case "git_open": valid = r.kind === "context" && r.agent_unchanged === true && id(r.context?.id) && text(r.context?.label, 1024); // falls through for reviews
    case "review":
      valid ||= r.kind === "review" && id(r.id) && (request.op !== "review" || r.id === request.review_id)
        && typeof r.path === "string" && typeof r.root === "string" && typeof r.basis === "string"
        && r.read_only === true && typeof r.binary === "boolean" && typeof r.truncated === "boolean"
        && typeof r.same_agent_root === "boolean" && Array.isArray(r.lines) && r.lines.length <= 4096
        && r.lines.every(line => line && typeof line.text === "string" && typeof line.kind === "string");
      break;
  }
  if (!valid) throw new Error("Invalid exact workspace response.");
  return { data: freeze(r), bytes: bytes(encoded) };
}

export function createWorkspaceViews({ request, onChange = () => {}, externalDocumentCount = () => 1 }) {
  if (typeof request !== "function" || typeof onChange !== "function" || typeof externalDocumentCount !== "function") throw new Error("Workspace views need a reader and document count.");
  const containers = new Map(), documents = new Map(), scopes = new Map(), outstanding = new Set();
  let source = null, sequence = 0, retained = 0, disposed = false;
  const emit = (type = "change", scope_id) => onChange(Object.freeze({ type, scope_id }));
  const publish = (entry, patch) => { entry.value = Object.freeze({ ...entry.value, ...patch }); };
  function allocate() {
    try { return sequence = checkedWorkspaceSequence(sequence); }
    catch (error) { clear(); throw error; }
  }
  function ready() { if (disposed || !source) throw new Error("Workspace source unavailable; reconnect explicitly."); }
  function container(id) { const entry = containers.get(id); if (!entry) throw new Error("Workspace container is closed."); return entry; }
  function assertCanOpenDocument() {
    const count = externalDocumentCount();
    if (!Number.isSafeInteger(count) || count < 1 || count > WORKSPACE_VIEW_LIMITS.documents) throw new Error("Invalid external document count.");
    if (documents.size + count >= WORKSPACE_VIEW_LIMITS.documents) throw new Error("At most 32 document tabs may be open, including Agent tabs.");
    return true;
  }
  function refreshValue(entry) {
    if (entry.document) publish(entry, { data: entry.cache.get("document")?.data || null, pending: entry.tokens.size > 0 });
    else publish(entry, { listings: Object.freeze(Object.fromEntries([...entry.cache].filter(([key]) => key.startsWith("dir:")).map(([key, value]) => [key.slice(4), value.data]))),
      search: entry.cache.get("search")?.data || null, inventory: entry.cache.get("inventory")?.data || null, pending: entry.tokens.size > 0 });
  }
  function removeCache(entry, slot) {
    const old = entry.cache.get(slot);
    if (old) { retained -= old.bytes; entry.cache.delete(slot); }
  }
  function scrub(entry, error = "", unavailable = false) {
    for (const item of entry.cache.values()) retained -= item.bytes;
    entry.cache.clear();
    publish(entry, { error, unavailable, ...(entry.document ? {} : { navigator: defaultNavigator(), label: unavailable ? "Unavailable scope" : entry.value.label }) });
    refreshValue(entry);
  }
  function cancel(entry, slot) {
    const token = entry.tokens.get(slot);
    if (!token) return null;
    entry.tokens.delete(slot);
    refreshValue(entry);
    return token;
  }
  function cancelAll(entry) { const tokens = [...entry.tokens.values()]; entry.tokens.clear(); refreshValue(entry); return tokens; }
  function clear() {
    const tokens = [];
    // Scrub awaiting entry frames and remove live identities before abort callbacks.
    for (const entry of [...containers.values(), ...documents.values()]) {
      tokens.push(...cancelAll(entry));
      scrub(entry);
    }
    source = null; containers.clear(); documents.clear(); scopes.clear(); retained = 0;
    emit("clear");
    for (const token of tokens) token.control.abort();
  }
  function denyScope(scope_id, error) {
    const tokens = [];
    const scope = scopes.get(scope_id);
    if (scope) scopes.set(scope_id, Object.freeze({ ...scope, unavailable: true }));
    for (const entry of [...containers.values(), ...documents.values()]) {
      if ((entry.document ? entry.value.target.scope_id : entry.value.scope_id) !== scope_id) continue;
      tokens.push(...cancelAll(entry)); scrub(entry, error, true);
      if (!entry.document) publish(entry, { generation: allocate() });
    }
    emit("scope-denied", scope_id);
    for (const token of tokens) token.control.abort();
  }
  function cache(entry, slot, result) {
    const previous = entry.cache.get(slot);
    if (retained - (previous?.bytes || 0) + result.bytes > WORKSPACE_VIEW_LIMITS.retainedBytes) throw new Error("Workspace snapshots exceed the 8 MiB cache limit; close a view and refresh.");
    if (!entry.document && slot.startsWith("dir:") && !previous
      && [...entry.cache.keys()].filter(key => key.startsWith("dir:")).length >= WORKSPACE_VIEW_LIMITS.directories) throw new Error("Directory cache limit reached; refresh this navigator.");
    const same = previous && JSON.stringify(previous.data) === JSON.stringify(result.data);
    retained += result.bytes - (previous?.bytes || 0);
    entry.cache.set(slot, { bytes: result.bytes, data: same ? previous.data : result.data });
    const scope_id = entry.document ? entry.value.target.scope_id : entry.value.scope_id;
    const scope = scopes.get(scope_id);
    if (scope?.unavailable) scopes.set(scope_id, Object.freeze({ ...scope, unavailable: false }));
    publish(entry, { error: "", unavailable: false });
    if (!entry.document) publish(entry, { label: scopes.get(entry.value.scope_id)?.label || entry.value.label });
    refreshValue(entry);
  }
  async function run(entry, slot, raw, { signal, accept, transient = false } = {}) {
    const command = requestCopy(raw);
    if (!source || disposed || signal?.aborted) return null;
    cancel(entry, slot)?.control.abort();
    if (outstanding.size >= WORKSPACE_VIEW_LIMITS.pending) { publish(entry, { error: "Workspace readers busy; refresh when a pending request finishes." }); emit(); return null; }
    const target = Object.freeze({ source_id: source.source_id, scope_id: entry.document ? entry.value.target.scope_id : entry.value.scope_id });
    const token = { sequence: allocate(), control: new AbortController() };
    entry.tokens.set(slot, token); outstanding.add(token);
    const abort = () => { if (entry.tokens.get(slot) === token) { cancel(entry, slot); emit(); } token.control.abort(); };
    signal?.addEventListener("abort", abort, { once: true });
    const current = () => source?.source_id === target.source_id && entry.tokens.get(slot) === token
      && (entry.document ? documents : containers).get(entry.value.id) === entry;
    publish(entry, { pending: true, error: "" }); emit();
    try {
      if (!current()) return null;
      const reply = await request({ op: "view", ...target, request: command }, { signal: token.control.signal });
      if (!current() || signal?.aborted) return null;
      const result = replyCopy(reply, target, command);
      if (!current() || signal?.aborted) return null;
      if (accept) accept(result.data);
      if (!transient) cache(entry, slot, result);
      emit(); return result.data;
    } catch (error) {
      if (!current()) return null;
      const message = String(error?.message || "Workspace read unavailable.").slice(0, 1024);
      if (error?.status === 401 || (error?.status === 403 && error.code === "source_changed"
        && error.source_id === target.source_id && error.scope_id === target.scope_id)) clear();
      else if (error?.status === 403 && error.code === "scope_denied" && error.source_id === target.source_id && error.scope_id === target.scope_id) denyScope(target.scope_id, message);
      else {
        if (error?.status === 403) removeCache(entry, slot);
        publish(entry, { error: message }); refreshValue(entry); emit();
      }
      return null;
    } finally {
      signal?.removeEventListener("abort", abort); outstanding.delete(token);
      if (entry.tokens.get(slot) === token) { entry.tokens.delete(slot); refreshValue(entry); }
      emit();
    }
  }
  function openDocument(owner, kind, target, origin) {
    ready(); assertCanOpenDocument();
    const documentId = `workspace-document-${allocate()}`;
    const entry = { document: true, cache: new Map(), tokens: new Map(), origin,
      value: Object.freeze({ id: documentId, containerId: owner.value.id, kind, target: Object.freeze(target), data: null, pending: false, error: "", unavailable: false }) };
    documents.set(documentId, entry); emit(); void refreshDocument(documentId);
    return documentId;
  }
  async function refreshDocument(documentId) {
    const entry = documents.get(documentId);
    if (!entry || !source) return false;
    const target = entry.value.target;
    const command = entry.value.kind === "file" ? { op: "file", path: target.path }
      : target.review_id ? { op: "review", review_id: target.review_id } : { op: "git_open", ...entry.origin };
    return Boolean(await run(entry, "document", command, { accept(data) {
      if (entry.value.kind === "review") {
        if (data.kind !== "review") throw new Error("Expected an immutable review, not another scope.");
        publish(entry, { target: Object.freeze({ source_id: target.source_id, scope_id: target.scope_id, review_id: data.id }) });
      }
    } }));
  }
  function closeDocument(documentId) {
    const entry = documents.get(documentId); if (!entry) return false;
    const tokens = cancelAll(entry); scrub(entry); documents.delete(documentId);
    emit(); for (const token of tokens) token.control.abort(); return true;
  }
  function closeContainer(containerId) {
    const entry = containers.get(containerId); if (!entry) return false;
    const owned = [...documents.values()].filter(document => document.value.containerId === containerId);
    const tokens = [];
    for (const item of [entry, ...owned]) { tokens.push(...cancelAll(item)); scrub(item); }
    containers.delete(containerId);
    for (const document of owned) documents.delete(document.value.id);
    emit("close-container"); for (const token of tokens) token.control.abort(); return true;
  }
  function loadNavigator(containerId, raw) {
    const entry = container(containerId), command = requestCopy(raw);
    if (!(entry.value.kind === "files" ? ["files", "file_search"] : ["git_list"]).includes(command.op)) throw new Error("Request does not belong to this navigator.");
    const slot = command.op === "files" ? `dir:${command.path}` : command.op === "file_search" ? "search" : "inventory";
    return run(entry, slot, command).then(Boolean);
  }
  return Object.freeze({
    setSource(value) {
      if (value === null) { clear(); return true; }
      if (disposed || !value || !id(value.source_id) || !id(value.view_root?.id) || !text(value.view_root?.label, 1024)) throw new Error("Invalid workspace source capability.");
      if (source?.source_id === value.source_id && source.view_root.id === value.view_root.id) return false;
      clear(); source = freeze({ source_id: value.source_id, view_root: { id: value.view_root.id, label: value.view_root.label } });
      scopes.set(source.view_root.id, source.view_root); emit("source"); return true;
    },
    openContainer(kind, scope_id = source?.view_root.id) {
      ready();
      if (!["files", "git"].includes(kind) || !scopes.has(scope_id) || (kind === "files" && scope_id !== source.view_root.id)) throw new Error("Scope is not an authorized navigator binding.");
      if (scopes.get(scope_id).unavailable) throw new Error("Workspace scope unavailable; refresh an existing view explicitly.");
      if (containers.size >= WORKSPACE_VIEW_LIMITS.containers) throw new Error("At most 8 Files/Git containers may be open.");
      const generation = allocate(), containerId = `workspace-container-${generation}`;
      containers.set(containerId, { document: false, tokens: new Map(), cache: new Map(), value: Object.freeze({ id: containerId,
        kind, scope_id, label: scopes.get(scope_id).label, generation, navigator: defaultNavigator(), listings: Object.freeze({}), search: null, inventory: null, pending: false, error: "", unavailable: false }) });
      emit(); return containerId;
    },
    listContainers: () => Object.freeze([...containers.values()].map(entry => entry.value)),
    getContainer: containerId => containers.get(containerId)?.value || null,
    listDocuments: () => Object.freeze([...documents.values()].map(entry => entry.value)),
    getDocument: documentId => documents.get(documentId)?.value || null,
    patchNavigator(containerId, patch) {
      const entry = container(containerId), next = navigatorCopy(patch, entry.value.navigator);
      const tokens = [];
      if (next.query !== entry.value.navigator.query) { const token = cancel(entry, "search"); if (token) tokens.push(token); removeCache(entry, "search"); }
      if (next.mode !== entry.value.navigator.mode) { const token = cancel(entry, "inventory"); if (token) tokens.push(token); removeCache(entry, "inventory"); }
      publish(entry, { navigator: next }); refreshValue(entry);
      emit(); for (const token of tokens) token.control.abort(); return true;
    },
    loadNavigator,
    async refreshNavigator(containerId) {
      const entry = container(containerId), navigator = entry.value.navigator;
      const tokens = cancelAll(entry); scrub(entry); publish(entry, { generation: allocate() });
      const generation = entry.value.generation;
      publish(entry, { navigator }); emit("refresh");
      for (const token of tokens) token.control.abort();
      if (entry.value.kind === "git") return loadNavigator(containerId, { op: "git_list", mode: navigator.mode });
      const listed = await loadNavigator(containerId, { op: "files", path: "" });
      if (containers.get(containerId) !== entry || entry.value.generation !== generation
        || entry.value.navigator.query !== navigator.query) return false;
      const searched = !navigator.query.trim() || await loadNavigator(containerId, { op: "file_search", query: navigator.query.trim(), limit: 120 });
      return listed && searched;
    },
    async searchFiles(containerId, query, limit = 120, { signal } = {}) {
      const entry = container(containerId);
      if (entry.value.kind !== "files") throw new Error("File search requires a Files container.");
      return run(entry, "palette", { op: "file_search", query, limit }, { signal, transient: true });
    },
    openFile(containerId, relativePath) {
      const entry = container(containerId); ready();
      if (entry.value.kind !== "files" || !path(relativePath)) throw new Error("File needs an exact Files scope and relative path.");
      const existing = [...documents.values()].find(document => document.value.containerId === containerId && document.value.kind === "file"
        && document.value.target.source_id === source.source_id && document.value.target.scope_id === entry.value.scope_id && document.value.target.path === relativePath);
      return existing?.value.id || openDocument(entry, "file", { source_id: source.source_id, scope_id: entry.value.scope_id, path: relativePath });
    },
    openReview(containerId, origin) {
      const entry = container(containerId); ready(); origin = gitOrigin(origin);
      if (entry.value.kind !== "git") throw new Error("Review needs a Git navigator.");
      const existing = [...documents.values()].find(document => document.value.containerId === containerId && document.value.kind === "review"
        && document.value.target.scope_id === entry.value.scope_id && document.origin?.inventory_id === origin.inventory_id && document.origin?.item_id === origin.item_id);
      return existing?.value.id || openDocument(entry, "review", { source_id: source.source_id, scope_id: entry.value.scope_id, ...origin }, Object.freeze({ ...origin }));
    },
    async selectWorktree(containerId, origin) {
      const entry = container(containerId); origin = gitOrigin(origin);
      if (entry.value.kind !== "git") throw new Error("Worktree selection needs a Git navigator.");
      const tokens = cancelAll(entry); for (const token of tokens) token.control.abort();
      const result = await run(entry, "worktree", { op: "git_open", ...origin }, { transient: true, accept(data) {
        if (data.kind !== "context") throw new Error("Expected an authorized worktree scope.");
        if (!scopes.has(data.context.id) && scopes.size >= 16) throw new Error("Retained worktree scope limit reached; reconnect explicitly.");
        scopes.set(data.context.id, data.context); scrub(entry);
        publish(entry, { scope_id: data.context.id, label: data.context.label, navigator: defaultNavigator(), generation: allocate(), unavailable: false });
      } });
      return Boolean(result);
    },
    assertCanOpenDocument, refreshDocument, closeDocument, closeContainer, clear,
    dispose() { disposed = true; clear(); },
  });
}
