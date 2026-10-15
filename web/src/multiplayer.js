import * as Y from "yjs";

const LOCAL = Symbol("aw-local-draft");
const REMOTE = Symbol("aw-remote-draft");

function fromBase64(value) {
  const binary = atob(value || "");
  const out = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) out[index] = binary.charCodeAt(index);
  return out;
}

function toBase64(value) {
  let binary = "";
  const step = 0x8000;
  for (let index = 0; index < value.length; index += step) {
    binary += String.fromCharCode(...value.subarray(index, index + step));
  }
  return btoa(binary);
}

function safePrefix(left, right) {
  let index = 0;
  const max = Math.min(left.length, right.length);
  while (index < max && left.charCodeAt(index) === right.charCodeAt(index)) index += 1;
  if (
    index > 0
    && index < left.length
    && index < right.length
    && left.charCodeAt(index - 1) >= 0xd800
    && left.charCodeAt(index - 1) <= 0xdbff
  ) index -= 1;
  return index;
}

function safeSuffix(left, right, prefix) {
  let count = 0;
  const max = Math.min(left.length, right.length) - prefix;
  while (
    count < max
    && left.charCodeAt(left.length - 1 - count) === right.charCodeAt(right.length - 1 - count)
  ) count += 1;
  const leftBoundary = left.length - count;
  const rightBoundary = right.length - count;
  if (
    count > 0
    && leftBoundary > prefix
    && rightBoundary > prefix
    && left.charCodeAt(leftBoundary - 1) >= 0xd800
    && left.charCodeAt(leftBoundary - 1) <= 0xdbff
  ) count -= 1;
  return count;
}

function replaceText(text, next) {
  const current = text.toString();
  if (current === next) return;
  const prefix = safePrefix(current, next);
  const suffix = safeSuffix(current, next, prefix);
  const remove = current.length - prefix - suffix;
  const insert = next.slice(prefix, next.length - suffix);
  if (remove > 0) text.delete(prefix, remove);
  if (insert) text.insert(prefix, insert);
}

function validSnapshot(value) {
  return Boolean(
    value
      && typeof value.draft_id === "string"
      && Number.isSafeInteger(value.generation)
      && Number.isSafeInteger(value.document_epoch)
      && Number.isSafeInteger(value.accepted_revision)
      && Number.isSafeInteger(value.metadata_revision)
      && typeof value.text === "string"
      && typeof value.update_v1_b64 === "string"
      && typeof value.candidate_sha256 === "string",
  );
}

export function createSharedDraftClient(options) {
  let doc = null;
  let text = null;
  let snapshot = null;
  let queue = Promise.resolve();
  let queueError = null;
  let localSequence = 0;
  let textObserver = null;
  let updateObserver = null;

  function state() {
    return snapshot;
  }

  function notifyState(next) {
    if (!validSnapshot(next)) throw new Error("Invalid shared-draft snapshot.");
    snapshot = next;
    options.onState?.(next);
  }

  function destroyDocument() {
    if (text && textObserver) text.unobserve(textObserver);
    if (doc && updateObserver) doc.off("update", updateObserver);
    doc?.destroy();
    doc = null;
    text = null;
    textObserver = null;
    updateObserver = null;
  }

  function install(next) {
    destroyDocument();
    if (!validSnapshot(next)) throw new Error("Invalid shared-draft snapshot.");
    doc = new Y.Doc();
    text = doc.getText("prompt");
    Y.applyUpdate(doc, fromBase64(next.update_v1_b64), REMOTE);
    snapshot = next;

    textObserver = event => {
      options.onText?.(text.toString(), event.transaction.origin === LOCAL ? "local" : "remote");
    };
    text.observe(textObserver);

    updateObserver = (update, origin) => {
      if (origin !== LOCAL) return;
      const updateId = "web-" + (++localSequence) + "-" + crypto.randomUUID();
      const body = {
        draft_id: snapshot.draft_id,
        generation: snapshot.generation,
        document_epoch: snapshot.document_epoch,
        update_id: updateId,
        update_v1_b64: toBase64(update),
      };
      queue = queue
        .catch(() => undefined)
        .then(async () => {
          const acknowledged = await options.api("/api/team/draft/update", body, 30000);
          queueError = null;
          notifyState(acknowledged);
          return acknowledged;
        })
        .catch(error => {
          queueError = error;
          options.onError?.(error);
          throw error;
        });
    };
    doc.on("update", updateObserver);
    options.onText?.(text.toString(), "remote");
    options.onState?.(next);
  }

  function setText(value) {
    if (!doc || !text) throw new Error("Shared draft is not active.");
    doc.transact(() => replaceText(text, value), LOCAL);
  }

  function applyRemote(payload) {
    if (
      !doc
      || !snapshot
      || payload?.draft_id !== snapshot.draft_id
      || payload?.generation !== snapshot.generation
      || typeof payload?.update_v1_b64 !== "string"
    ) return false;
    Y.applyUpdate(doc, fromBase64(payload.update_v1_b64), REMOTE);
    if (
      Number.isSafeInteger(payload.accepted_revision)
      && payload.accepted_revision > snapshot.accepted_revision
    ) {
      snapshot = { ...snapshot, accepted_revision: payload.accepted_revision, text: text.toString() };
      options.onState?.(snapshot);
    }
    return true;
  }

  async function flush() {
    try {
      await queue;
    } catch {}
    if (queueError) throw queueError;
    if (!snapshot) throw new Error("Shared draft is not active.");
    const confirmed = await options.api("/api/team/draft/status", {
      draft_id: snapshot.draft_id,
      generation: snapshot.generation,
    });
    notifyState(confirmed);
    if (text?.toString() !== confirmed.text) install(confirmed);
    return snapshot;
  }

  async function refresh() {
    if (!snapshot) return null;
    const next = await options.api("/api/team/draft/status", {
      draft_id: snapshot.draft_id,
      generation: snapshot.generation,
    });
    install(next);
    return next;
  }

  function active() {
    return Boolean(doc && snapshot);
  }

  function currentText() {
    return text?.toString() || "";
  }

  function destroy() {
    destroyDocument();
    snapshot = null;
    queue = Promise.resolve();
    queueError = null;
  }

  return {
    active,
    applyRemote,
    currentText,
    destroy,
    flush,
    install,
    refresh,
    setText,
    state,
  };
}

export const __test = { fromBase64, replaceText, safePrefix, safeSuffix, toBase64 };
