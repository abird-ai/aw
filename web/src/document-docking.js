// Read-only File/immutable Git Review cross-window handoff. The server ticket
// is correlation, not a capability. Receiver independently authenticates and
// reads the exact retained source before accepting. No plaintext or grants
// cross browser windows in a message or ticket.
import { boundedPresentation, documentFingerprint } from "./document-presentation.js";
const ticketSyntax = /^[a-f0-9]{64}$/u;
const opaqueIdSyntax = /^[A-Za-z0-9_-]{1,128}$/u;
const MAX_WAIT_MS = 90_000;

export function takeDocumentDockTicket(location, history) {
  if (!location?.hash?.startsWith("#aw-dock=")) return null;
  const ticket = location.hash.slice("#aw-dock=".length);
  history.replaceState(null, "", location.pathname + location.search);
  return ticketSyntax.test(ticket) ? ticket : null;
}

export function createDocumentDocking({
  api, views, currentOwner, select, close, popup,
  sleep = ms => new Promise(resolve => setTimeout(resolve, ms)),
  timeoutMs = MAX_WAIT_MS, onProgress = () => {}, isRendered = () => true,
  capture = () => null, restore = async () => true,
  navigate = (window, ticket) => {
    window.opener = null;
    window.location.replace(new URL("#aw-dock=" + ticket, location.href).href);
  },
}) {
  if ([api, currentOwner, select, close, popup, sleep, onProgress, isRendered, capture, restore, navigate]
    .some(callback => typeof callback !== "function")) throw new Error("Read-only document docking needs original-browser callbacks.");
  let disposed = false;
  let staged = null;
  // Called on the real button's pointerdown before browser focus moves away
  // from Find/the editor. Keyboard actions capture at send as a fallback.
  function stage(documentId) {
    staged = { documentId, snapshot: capture(documentId), createdAt: Date.now() };
  }
  function cancelStage(documentId) {
    if (staged?.documentId === documentId) staged = null;
  }
  const ownerMatches = expected => {
    const now = currentOwner();
    return !disposed && now?.source_id === expected.source_id
      && now?.owner_epoch === expected.owner_epoch
      && (expected.kind === "review" || now?.scope_id === expected.scope_id);
  };
  const envelope = (window, ticket) => ({
    window_id: window.window_id, window_proof: window.window_proof, ticket,
  });
  const safeClose = window => {
    try { window?.close?.(); } catch { /* already closed */ }
  };

  async function send(documentId) {
    const document = views.getDocument(documentId);
    const source = currentOwner();
    const kind = document?.kind;
    const target = document?.target;
    const exactResource = kind === "file" ? target?.path : target?.review_id;
    if (disposed || !["file", "review"].includes(kind) || !document.data || document.unavailable
      || document.pending || !target || typeof exactResource !== "string"
      || (kind === "review" && (!opaqueIdSyntax.test(exactResource)
        || document.data.kind !== "review" || document.data.id !== exactResource))
      || !ownerMatches({
        ...source, kind, source_id: target.source_id, scope_id: target.scope_id,
      })) throw new Error("Only a currently authorized, loaded File or immutable Review can move to another window.");
    const initial = document;
    // A pointer press can be cancelled before click. Never reuse a stale
    // captured Find/caret from a previous gesture or different document.
    const captured = staged?.documentId === documentId
      && Date.now() - staged.createdAt <= 2000
      ? staged.snapshot : capture(documentId);
    staged = null;
    const expected = { ...source };
    const originalContainer = initial.containerId;
    const originalResource = exactResource;
    const sameDocument = () => {
      const active = views.getDocument(documentId);
      return ownerMatches(expected) && active
        && active.containerId === originalContainer && active.target === initial.target
        && active.kind === kind && !active.unavailable && active.data === initial.data
        && (kind !== "review" || active.data.id === originalResource);
    };
    const nextWindow = popup(); // Must execute during the direct user click.
    if (!nextWindow) throw new Error("The browser blocked the new window. Allow pop-ups for the local owner.");
    let origin, ticket, released = false, navigated = false;
    onProgress(documentId, true);
    try {
      // The browser computes the digest from already-authorized local data.
      // No plaintext is included in server ticket metadata or popup URL.
      const presentation = captured ? boundedPresentation(kind, {
        ...captured, version: 1,
        fingerprint: await documentFingerprint(kind, initial.data),
      }) : null;
      if (!sameDocument()) throw new Error("The original document changed during handoff.");
      origin = await api("/api/dock/register", {});
      if (!sameDocument() || origin.source_id !== expected.source_id
        || origin.scope_id !== expected.scope_id || origin.owner_epoch !== expected.owner_epoch) {
        throw new Error("Source changed before handoff preparation.");
      }
      const prepared = await api(kind === "file" ? "/api/dock/prepare" : "/api/dock/prepare-review", {
        window_id: origin.window_id, window_proof: origin.window_proof,
        owner_epoch: expected.owner_epoch, source_id: expected.source_id,
        scope_id: target.scope_id, document_id: documentId,
        ...(kind === "file" ? { path: originalResource } : { review_id: originalResource }),
        ...(presentation ? { presentation } : {}),
      });
      ticket = prepared.ticket;
      if (!ticketSyntax.test(ticket)) throw new Error("Invalid owner handoff identity.");
      if (!sameDocument()) throw new Error("Original view changed during preparation.");
      // The fragment never reaches the HTTP owner and is removed from browser
      // history by the receiver before any inspection. Not an authority token.
      navigate(nextWindow, ticket);
      navigated = true;
      const until = Date.now() + Math.min(Math.max(timeoutMs, 1000), MAX_WAIT_MS);
      while (Date.now() < until && !disposed) {
        if (!sameDocument() || nextWindow.closed) break;
        const result = await api("/api/dock/status", envelope(origin, ticket));
        if (result.document_id !== documentId) throw new Error("Handoff target changed.");
        if (result.state === "accepted") {
          if (!sameDocument()) break;
          const outcome = await api("/api/dock/release", envelope(origin, ticket));
          if (outcome.state !== "released" || outcome.document_id !== documentId) {
            throw new Error("Handoff was not released by the owner.");
          }
          released = true;
          if (sameDocument()) close(documentId);
          return true;
        }
        if (result.state !== "prepared") break;
        await sleep(250);
      }
      throw new Error("Handoff not accepted; the original read-only document remains open.");
    } catch (error) {
      if (!released && origin && ticket) {
        // Cancellation only rolls back Prepared. Unknown Accept/Release stays
        // inspectable; never claim a transfer on a failed network reply.
        try { await api("/api/dock/cancel", envelope(origin, ticket)); } catch {}
      }
      if (!navigated) safeClose(nextWindow);
      throw error;
    } finally {
      onProgress(documentId, false);
    }
  }

  async function receive(ticket) {
    if (disposed || !ticketSyntax.test(ticket)) throw new Error("Invalid read-only document docking invitation.");
    const receiver = await api("/api/dock/register", {});
    const body = envelope(receiver, ticket);
    let containerId, documentId, acceptanceAttempted = false;
    try {
      const description = await api("/api/dock/inspect", body);
      const kind = description.kind;
      const resourceKey = kind === "file" ? "path" : "review_id";
      if (!["file", "review"].includes(kind)
        || !ownerMatches(description)
        || description.source_id !== receiver.source_id
        || description.owner_epoch !== receiver.owner_epoch
        || !opaqueIdSyntax.test(description.scope_id)
        || (kind === "file" && description.scope_id !== receiver.scope_id)
        || (kind === "review" && !opaqueIdSyntax.test(description.review_id))
        || typeof description[resourceKey] !== "string") {
        throw new Error("The original File or Review scope is unavailable in this window.");
      }
      // A Review's original retained Git scope can differ from the default
      // navigator. Do not install it as a general worktree or Files grant.
      containerId = views.openContainer(kind === "file" ? "files" : "git");
      documentId = kind === "file"
        ? views.openFile(containerId, description.path)
        : views.openRetainedReview(containerId, {
            source_id: description.source_id,
            scope_id: description.scope_id,
            review_id: description.review_id,
          });
      select(containerId, documentId, kind);
      const until = Date.now() + Math.min(Math.max(timeoutMs, 1000), MAX_WAIT_MS);
      while (Date.now() < until && ownerMatches(description)) {
        const item = views.getDocument(documentId);
        if (!item || item.target.source_id !== description.source_id
          || item.target.scope_id !== description.scope_id
          || item.target[resourceKey] !== description[resourceKey]
          || item.unavailable || item.error) break;
        // The new browser performs its own authorized read and renders the
        // result before asking the owner to accept the transfer.
        if (!item.pending && item.data && isRendered(documentId)) {
          if (description.presentation) {
            const presentation = boundedPresentation(kind, description.presentation);
            const fingerprint = await documentFingerprint(kind, item.data);
            if (fingerprint !== presentation.fingerprint) {
              throw new Error("The source content changed during handoff; refresh the original document and retry.");
            }
            if (!await restore(documentId, presentation)) {
              throw new Error("Receiving editor is no longer available to restore.");
            }
            // Restoration awaits receiver layout; deny a closed/replaced view
            // or a changed workspace before accepting the single-use ticket.
            const current = views.getDocument(documentId);
            if (!ownerMatches(description) || current !== item
              || !isRendered(documentId)) {
              throw new Error("Receiving document changed during presentation restoration.");
            }
          }
          acceptanceAttempted = true;
          const result = await api("/api/dock/accept", body);
          if (result.state !== "accepted") {
            const error = new Error("The owner did not accept this view.");
            error.status = 409;
            throw error;
          }
          return { containerId, documentId };
        }
        await sleep(120);
      }
      throw new Error("Receiving read-only document failed authorization or timed out; origin retains its view.");
    } catch (error) {
      // A *known* authorization/expiry/conflict denial scrubs plaintext
      // immediately, even after a successful earlier File read. Only a lost
      // response or 5xx after the acceptance request is truly uncertain.
      const knownFailure = Number.isInteger(error?.status)
        && error.status >= 400 && error.status < 500;
      if (!acceptanceAttempted || knownFailure) {
        try { await api("/api/dock/decline", body); } catch {}
        if (documentId) views.closeDocument(documentId);
        if (containerId) views.closeContainer(containerId);
      }
      // An unknown acceptance response may already have committed on the
      // owner. Keep its read-only, independently authorized view, never claim
      // success or retry the ticket as a new operation.
      throw error;
    }
  }

  return Object.freeze({ stage, cancelStage, send, receive, dispose() { disposed = true; staged = null; } });
}
