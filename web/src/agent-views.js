// Memory-only read-only views. Transport/authentication belong to the existing
// HTTP adapter; these IDs and snapshots never grant Store or Agent authority.
export const AGENT_VIEW_LIMITS = Object.freeze({
  views: 8,
  snapshotBytes: 4 * 1024 * 1024,
  retainedBytes: 8 * 1024 * 1024,
  pending: 4,
  refreshMs: 5000,
  expiryMs: 15000,
});

const encoder = new TextEncoder();
const targetFields = ["store_id", "source_epoch", "session_id", "branch_id"];
const responseFields = [...targetFields, "title", "branch_name", "head", "revision",
  "turns", "context_blocks", "needs_inspection", "retained_bytes", "expires_in_ms"];
const controls = /[\u0000-\u001f\u007f-\u009f]/u;
const emptyFind = () => Object.freeze({ query: "", selected: 0 });

export function checkedViewSequence(previous) {
  if (!Number.isSafeInteger(previous) || previous < 0 || previous >= Number.MAX_SAFE_INTEGER) {
    throw new Error("Read-only view IDs exhausted; reload the browser.");
  }
  return previous + 1;
}

function targetCopy(value) {
  if (!value || Object.keys(value).length !== targetFields.length || targetFields.some(key =>
    typeof value[key] !== "string" || !value[key] || encoder.encode(value[key]).length > 128
    || controls.test(value[key]))) throw new Error("Invalid exact read-only Agent target.");
  return Object.freeze(Object.fromEntries(targetFields.map(key => [key, value[key]])));
}

function sameTarget(left, right) {
  return targetFields.every(key => left[key] === right[key]);
}

function freezeJson(value) {
  const stack = [[value, 0]];
  let objects = 0;
  while (stack.length) {
    const [item, depth] = stack.pop();
    if (!item || typeof item !== "object") continue;
    if (depth > 64 || ++objects > 65536) throw new Error("Read-only transcript structure exceeds its display limit.");
    Object.freeze(item);
    for (const child of Object.values(item)) stack.push([child, depth + 1]);
  }
  return value;
}

function snapshotCopy(reply, target) {
  if (!reply || Object.keys(reply).length !== responseFields.length
    || responseFields.some(key => !Object.hasOwn(reply, key)) || !sameTarget(reply, target)
    || typeof reply.title !== "string" || typeof reply.branch_name !== "string"
    || typeof reply.head !== "string" || encoder.encode(reply.head).length > 128
    || typeof reply.revision !== "string" || !/^(0|[1-9][0-9]{0,19})$/.test(reply.revision)
    || BigInt(reply.revision) > 18446744073709551615n
    || typeof reply.needs_inspection !== "boolean"
    || !Number.isSafeInteger(reply.retained_bytes) || reply.retained_bytes < 0
    || reply.retained_bytes > AGENT_VIEW_LIMITS.snapshotBytes
    || !Number.isSafeInteger(reply.expires_in_ms) || reply.expires_in_ms <= 0
    || reply.expires_in_ms > AGENT_VIEW_LIMITS.expiryMs
    || !Array.isArray(reply.turns) || reply.turns.length > 4096
    || !Array.isArray(reply.context_blocks) || reply.context_blocks.length > 4096
    || reply.turns.some(turn => !turn || typeof turn.prompt !== "string" || typeof turn.answer !== "string")
    || reply.context_blocks.some(block => !block || typeof block.text !== "string"
      || !Number.isSafeInteger(block.before_turn) || block.before_turn < 0
      || block.before_turn > reply.turns.length)) {
    throw new Error("Invalid exact read-only Agent response.");
  }
  // Fixed public field order also makes equivalent envelope key order harmless.
  const encoded = JSON.stringify(Object.fromEntries(responseFields.map(key => [key, reply[key]])));
  const bytes = encoder.encode(encoded).length;
  if (bytes > AGENT_VIEW_LIMITS.snapshotBytes) {
    throw new Error("Read-only transcript exceeds the 4 MiB display limit.");
  }
  return { snapshot: freezeJson(JSON.parse(encoded)), encoded, bytes };
}

function viewPatch(patch, current) {
  if (!patch || Object.keys(patch).some(key => !["find", "viewport", "selection"].includes(key))) {
    throw new Error("Invalid read-only view state.");
  }
  const next = {};
  if (Object.hasOwn(patch, "find")) {
    const find = { ...current.find, ...patch.find };
    if (!patch.find || Object.keys(patch.find).some(key => !["query", "selected"].includes(key))
      || typeof find.query !== "string" || encoder.encode(find.query).length > 4096
      || !Number.isSafeInteger(find.selected) || find.selected < 0 || find.selected > 4096) {
      throw new Error("Invalid bounded read-only Find state.");
    }
    next.find = Object.freeze(find);
  }
  if (Object.hasOwn(patch, "viewport")) {
    const viewport = { ...current.viewport, ...patch.viewport };
    if (!patch.viewport || Object.keys(patch.viewport).some(key => !["top", "left", "follow"].includes(key))
      || ![viewport.top, viewport.left].every(value => Number.isFinite(value) && value >= 0 && value <= 1e9)
      || typeof viewport.follow !== "boolean") throw new Error("Invalid read-only viewport.");
    next.viewport = Object.freeze(viewport);
  }
  if (Object.hasOwn(patch, "selection")) {
    const selection = patch.selection;
    if (selection !== null && (!selection || Object.keys(selection).length !== 2
      || ![selection.anchor, selection.head].every(value => Number.isSafeInteger(value)
        && value >= 0 && value <= AGENT_VIEW_LIMITS.snapshotBytes))) {
      throw new Error("Invalid bounded read-only selection.");
    }
    next.selection = selection === null ? null : Object.freeze({ anchor: selection.anchor, head: selection.head });
  }
  return next;
}

export function createAgentViews({ read, onChange = () => {}, now = () => performance.now() }) {
  if (typeof read !== "function" || typeof onChange !== "function" || typeof now !== "function") {
    throw new Error("Read-only views require a reader, clock and change callback.");
  }
  const entries = new Map();
  const outstanding = new Set();
  let sequence = 0;
  let retained = 0;
  let disposed = false;
  let lastTime = 0;

  function clock() {
    const time = now();
    if (!Number.isFinite(time) || time < 0 || time > Number.MAX_SAFE_INTEGER - AGENT_VIEW_LIMITS.expiryMs) {
      throw new Error("Read-only view clock is unavailable.");
    }
    lastTime = Math.max(lastTime, time);
    return lastTime;
  }
  function allocate() {
    try { sequence = checkedViewSequence(sequence); }
    catch (error) { clear(); throw error; }
    return sequence;
  }
  function publish(entry, patch) {
    entry.view = Object.freeze({ ...entry.view, ...patch });
  }
  function discard(entry, status, error = "") {
    retained -= entry.bytes;
    entry.bytes = 0;
    publish(entry, { snapshot: null, status, error, expiresAt: null,
      find: emptyFind(), selection: null, pending: Boolean(entry.token) });
  }
  function invalidate(entry, status, error = "") {
    const token = entry.token;
    entry.token = null;
    // A pending async frame still owns this entry even after Map deletion.
    // Drop its protected snapshot/derived state before any abort listener runs.
    discard(entry, status, error);
    return token;
  }
  function close(id) {
    const entry = entries.get(id);
    if (!entry) return false;
    const token = invalidate(entry, "closed");
    entries.delete(id);
    token?.control.abort();
    onChange();
    return true;
  }
  function clear() {
    const tokens = Array.from(entries.values(), entry => invalidate(entry, "closed"));
    entries.clear();
    retained = 0;
    for (const token of tokens) token?.control.abort();
    onChange();
  }
  function expire() {
    const time = clock();
    let changed = false;
    const tokens = [];
    for (const entry of entries.values()) {
      if (entry.token && time >= entry.token.deadline) {
        tokens.push(invalidate(entry, "expired", "Read-only view expired; refresh to revalidate access."));
        changed = true;
      } else if (entry.view.snapshot && time >= entry.view.expiresAt) {
        discard(entry, entry.token ? "loading" : "expired", "Read-only view expired; revalidating access.");
        changed = true;
      }
    }
    for (const token of tokens) token?.control.abort();
    if (changed) onChange();
    return changed;
  }

  async function refresh(id) {
    if (disposed) return false;
    expire();
    const entry = entries.get(id);
    if (!entry || entry.token || outstanding.size >= AGENT_VIEW_LIMITS.pending) return false;
    const started = clock();
    const token = { sequence: allocate(), control: new AbortController(),
      deadline: started + AGENT_VIEW_LIMITS.expiryMs };
    entry.token = token;
    outstanding.add(token);
    publish(entry, { pending: true, status: "loading", error: "",
      refreshAt: started + AGENT_VIEW_LIMITS.refreshMs });
    onChange();
    let accepted = false;
    try {
      const reply = await read(entry.view.target, { signal: token.control.signal });
      if (disposed || entries.get(id) !== entry || entry.token !== token) return false;
      if (clock() >= token.deadline) {
        discard(entry, "expired", "Read-only reply expired; refresh to revalidate access.");
        return false;
      }
      const data = snapshotCopy(reply, entry.view.target);
      const expiresAt = started + data.snapshot.expires_in_ms;
      // Validation/copying is bounded work, but it can still consume the last
      // moments of a grant. Recheck at publication, not only at network return.
      const time = clock();
      if (time >= expiresAt) {
        discard(entry, "expired", "Read-only reply expired; refresh to revalidate access.");
        return false;
      }
      if (entry.view.snapshot && time >= entry.view.expiresAt) {
        discard(entry, "loading", "Read-only view expired; revalidating access.");
      }
      if (retained - entry.bytes + data.bytes > AGENT_VIEW_LIMITS.retainedBytes) {
        throw new Error("Read-only views exceed the 8 MiB cache limit; close another view and refresh.");
      }
      // A fresh successful read proves current authorization. Head/revision alone
      // never admits data or preserves derived state after denial/expiry.
      const unchanged = entry.view.snapshot && JSON.stringify(entry.view.snapshot) === data.encoded;
      retained += data.bytes - entry.bytes;
      entry.bytes = data.bytes;
      publish(entry, { snapshot: unchanged ? entry.view.snapshot : data.snapshot,
        status: "ready", error: "", expiresAt,
        ...(unchanged ? {} : { find: Object.freeze({ ...entry.view.find, selected: 0 }), selection: null }) });
      accepted = true;
    } catch (error) {
      if (disposed || entries.get(id) !== entry || entry.token !== token) return false;
      if (error?.status === 401) {
        clear();
      } else if (error?.status === 403) {
        const tokens = [];
        for (const other of entries.values()) {
          if (sameTarget(other.view.target, entry.view.target)) {
            tokens.push(invalidate(other, "error", "Read-only Agent access denied; refresh after permission is restored."));
          }
        }
        for (const token of tokens) token?.control.abort();
      } else {
        discard(entry, "error", String(error?.message || "Read-only Agent view unavailable.").slice(0, 1024));
      }
    } finally {
      outstanding.delete(token);
      if (entries.get(id) === entry && entry.token === token) {
        entry.token = null;
        publish(entry, { pending: false });
      }
      onChange();
    }
    return accepted;
  }

  return Object.freeze({
    open(target) {
      if (disposed) throw new Error("Read-only views have been disposed.");
      const exact = targetCopy(target);
      if (entries.size >= AGENT_VIEW_LIMITS.views) throw new Error("At most 8 read-only Agent views may be open.");
      const id = `agent-view-${allocate()}`;
      const view = Object.freeze({ id, target: exact, snapshot: null, status: "idle", error: "",
        pending: false, refreshAt: 0, expiresAt: null, find: emptyFind(),
        viewport: Object.freeze({ top: 0, left: 0, follow: false }), selection: null });
      entries.set(id, { view, bytes: 0, token: null });
      onChange();
      return id;
    },
    get(id) { expire(); return entries.get(id)?.view || null; },
    list() { expire(); return Object.freeze(Array.from(entries.values(), entry => entry.view)); },
    updateView(id, patch) {
      expire();
      const entry = entries.get(id);
      if (!entry) return false;
      publish(entry, viewPatch(patch, entry.view));
      onChange();
      return true;
    },
    refresh, close, clear, expire,
    dispose() { disposed = true; clear(); },
  });
}
