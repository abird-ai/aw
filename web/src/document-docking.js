// Read-only File/immutable Git Review cross-window handoff. The server ticket
// is correlation, not a capability. Receiver independently authenticates and
// reads the exact retained source before accepting. No plaintext or grants
// cross browser windows in a message or ticket.
import { boundedPresentation, documentFingerprint } from "./document-presentation.js";
const ticketSyntax = /^[a-f0-9]{64}$/u;
const opaqueIdSyntax = /^[A-Za-z0-9_-]{1,128}$/u;
const agentIdSyntax = /^[A-Za-z0-9_.:/@-]{1,128}$/u;
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
  retireWindow = () => {}, onLinkReady = () => {},
  makeLink = ticket => new URL("#aw-dock=" + ticket, location.href).href,
  capture = () => null, restore = async () => true,
  navigate = (window, ticket) => {
    window.opener = null;
    window.location.replace(new URL("#aw-dock=" + ticket, location.href).href);
  },
}) {
  if ([api, currentOwner, select, close, popup, sleep, onProgress, isRendered, capture, restore, retireWindow, onLinkReady, makeLink, navigate]
    .some(callback => typeof callback !== "function")) throw new Error("Read-only document docking needs original-browser callbacks.");
  let disposed = false;
  let staged = null;
  let receivingWindow = null;
  // A pagehide message is best effort. The origin's fresh ACK requirement
  // remains the real gate even when browsers omit unload networking.
  function retire() {
    if (receivingWindow) retireWindow(receivingWindow);
    receivingWindow = null;
  }
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

  async function send(documentId, { external = false } = {}) {
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
    const nextWindow = external ? null : popup(); // Only the popup path needs the direct click.
    if (!external && !nextWindow) throw new Error("The browser blocked the new window. Allow pop-ups for the local owner.");
    let origin, ticket, released = false, navigated = false, requestedConfirmation = false;
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
      if (external) onLinkReady(documentId, makeLink(ticket));
      else navigate(nextWindow, ticket);
      navigated = true;
      const until = Date.now() + Math.min(Math.max(timeoutMs, 1000), MAX_WAIT_MS);
      while (Date.now() < until && !disposed) {
        if (!sameDocument() || (nextWindow && nextWindow.closed)) break;
        const result = await api("/api/dock/status", envelope(origin, ticket));
        if (result.document_id !== documentId) throw new Error("Handoff target changed.");
        if (result.state === "confirming") {
          if (!requestedConfirmation) {
            const acknowledgement = await api("/api/dock/confirm", envelope(origin, ticket));
            if (acknowledgement.state !== "confirming") {
              throw new Error("The original owner rejected the confirmation request.");
            }
            requestedConfirmation = true;
          }
          await sleep(250);
          continue;
        }
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
      if (external) onLinkReady(documentId, null);
      onProgress(documentId, false);
    }
  }

  async function receive(ticket) {
    if (disposed || !ticketSyntax.test(ticket)) throw new Error("Invalid read-only document docking invitation.");
    const receiver = await api("/api/dock/register", {});
    const body = envelope(receiver, ticket);
    receivingWindow = body;
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
          // Accept alone is not sufficient. Source initiates confirmation
          // after seeing Accepted; receiver must respond while this document
          // and browser lifetime are still owned by this exact renderer.
          while (Date.now() < until && !disposed && ownerMatches(description)) {
            const current = views.getDocument(documentId);
            if (!current || current !== item || !isRendered(documentId)) break;
            const state = await api("/api/dock/heartbeat", body);
            if (state.state === "released") {
              receivingWindow = null;
              return { containerId, documentId };
            }
            if (!["confirming", "accepted"].includes(state.state)) break;
            await sleep(250);
          }
          throw new Error("Handoff release was not confirmed; read-only receiver remains inspectable.");
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

  return Object.freeze({
    stage, cancelStage, send, receive, retire,
    dispose() { retire(); disposed = true; staged = null; },
  });
}

const surfaceDockPrefix = "#aw-surface-dock=";

export function takeDelegatedSurfaceDockTicket(location, history) {
  if (!location?.hash?.startsWith(surfaceDockPrefix)) return null;
  const ticket = location.hash.slice(surfaceDockPrefix.length);
  history.replaceState(null, "", location.pathname + location.search);
  return ticketSyntax.test(ticket) ? ticket : null;
}

export function createDelegatedSurfaceDocking({
  api, views,
  sleep = ms => new Promise(resolve => setTimeout(resolve, ms)),
  timeoutMs = MAX_WAIT_MS, onProgress = () => {}, onLinkReady = () => {},
  onReceiveDocument = () => {}, isRendered = () => true,
  capture = () => null, restore = async () => true,
  makeLink = ticket => new URL(surfaceDockPrefix + ticket, location.href).href,
}) {
  if (typeof api !== "function" || !views
    || !["getDocument", "openDelegatedDocument", "refreshDocument", "closeDocument"]
      .every(name => typeof views[name] === "function")
    || [sleep, onProgress, onLinkReady, onReceiveDocument, isRendered, capture, restore, makeLink]
      .some(callback => typeof callback !== "function")) {
    throw new Error("Delegated File/Review docking needs the read-only document owner.");
  }
  let disposed = false;
  let staged = null;
  function stage(documentId, { preserveFocus = false } = {}) {
    let snapshot = capture(documentId);
    if (preserveFocus && snapshot?.focus === "none" && staged?.documentId === documentId
      && Date.now() - staged.createdAt <= 60_000
      && staged.snapshot?.focus && staged.snapshot.focus !== "none") {
      snapshot = { ...snapshot, focus: staged.snapshot.focus };
    }
    staged = { documentId, snapshot, createdAt: Date.now() };
  }
  function cancelStage(documentId) {
    if (staged?.documentId === documentId) staged = null;
  }
  const envelope = (window, ticket) => ({
    window_id: window.window_id, window_proof: window.window_proof, ticket,
  });
  const validPath = value => typeof value === "string" && value.length > 0 && value.length <= 4096
    && !value.startsWith("/") && !value.includes("\\") && !/[\u0000-\u001f\u007f-\u009f]/u.test(value)
    && value.split("/").every(part => part && part !== "." && part !== "..");
  const exactTarget = document => {
    if (!document || document.delegated || !["file", "review"].includes(document.kind)
      || !document.data || document.pending || document.unavailable || document.error
      || !document.target || !opaqueIdSyntax.test(document.target.source_id)
      || !opaqueIdSyntax.test(document.target.scope_id)) return null;
    if (document.kind === "file") {
      return validPath(document.target.path) && document.data.path === document.target.path
        ? { source_id: document.target.source_id, scope_id: document.target.scope_id, path: document.target.path }
        : null;
    }
    return opaqueIdSyntax.test(document.target.review_id)
      && document.data.kind === "review" && document.data.id === document.target.review_id
      ? { source_id: document.target.source_id, scope_id: document.target.scope_id, review_id: document.target.review_id }
      : null;
  };
  const sameTarget = (left, right, kind) => left && right
    && left.source_id === right.source_id && left.scope_id === right.scope_id
    && (kind === "file" ? left.path === right.path : left.review_id === right.review_id);

  async function send(documentId, recipientId) {
    const initial = views.getDocument(documentId);
    const target = exactTarget(initial);
    if (disposed || !target || !opaqueIdSyntax.test(recipientId)) {
      throw new Error("Only a loaded owner File or immutable Review can be delegated.");
    }
    const kind = initial.kind;
    const captured = staged?.documentId === documentId
      && Date.now() - staged.createdAt <= 2000
      ? staged.snapshot : capture(documentId);
    staged = null;
    const sameDocument = () => {
      const current = views.getDocument(documentId);
      return current && current === initial && sameTarget(current.target, target, kind)
        && current.data === initial.data && !current.pending && !current.unavailable && !current.error;
    };
    let origin, ticket, released = false, requestedConfirmation = false;
    onProgress(documentId, true);
    try {
      const presentation = captured ? boundedPresentation(kind, {
        ...captured, version: 1,
        fingerprint: await documentFingerprint(kind, initial.data),
      }) : null;
      if (!sameDocument()) throw new Error("The original document changed before delegation.");
      origin = await api("/api/dock/surface/register", {});
      if (!agentIdSyntax.test(origin.store_id) || !agentIdSyntax.test(origin.source_epoch)
        || !ticketSyntax.test(origin.window_id) || !ticketSyntax.test(origin.window_proof)) {
        throw new Error("Invalid delegated Surface owner identity.");
      }
      const prepared = await api("/api/dock/surface/prepare", {
        window_id: origin.window_id, window_proof: origin.window_proof,
        store_id: origin.store_id, source_epoch: origin.source_epoch,
        source_id: target.source_id, scope_id: target.scope_id, owner_epoch: origin.source_epoch,
        document_id: documentId, recipient_id: recipientId, kind,
        ...(kind === "file" ? { path: target.path } : { review_id: target.review_id }),
        ...(presentation ? { presentation } : {}),
      });
      ticket = prepared.ticket;
      if (!ticketSyntax.test(ticket) || !sameDocument()) {
        throw new Error("Invalid delegated File/Review handoff identity.");
      }
      onLinkReady(documentId, makeLink(ticket));
      const until = Date.now() + Math.min(Math.max(timeoutMs, 1000), MAX_WAIT_MS);
      while (Date.now() < until && !disposed) {
        if (!sameDocument()) break;
        const result = await api("/api/dock/surface/status", envelope(origin, ticket));
        if (result.document_id !== documentId) throw new Error("Delegated document target changed.");
        if (result.state === "confirming") {
          if (!requestedConfirmation) {
            const acknowledgement = await api("/api/dock/surface/confirm", envelope(origin, ticket));
            if (acknowledgement.state !== "confirming") {
              throw new Error("Delegated recipient confirmation was rejected.");
            }
            requestedConfirmation = true;
          }
          await sleep(250);
          continue;
        }
        if (result.state === "accepted") {
          if (!sameDocument()) break;
          const outcome = await api("/api/dock/surface/release", envelope(origin, ticket));
          if (outcome.state !== "released" || outcome.document_id !== documentId) {
            throw new Error("Delegated File/Review handoff was not released by the owner.");
          }
          released = true;
          if (sameDocument()) views.closeDocument(documentId);
          return true;
        }
        if (result.state === "revoked") {
          throw Object.assign(new Error("Delegated File/Review grant was revoked."), { status: 403 });
        }
        if (result.state !== "prepared") break;
        await sleep(250);
      }
      throw new Error("Delegated File/Review handoff not accepted; the original view remains open.");
    } catch (error) {
      if (!released && origin && ticket) {
        try { await api("/api/dock/surface/cancel", envelope(origin, ticket)); } catch {}
      }
      throw error;
    } finally {
      onLinkReady(documentId, null);
      onProgress(documentId, false);
    }
  }

  async function receive(ticket) {
    if (disposed || !ticketSyntax.test(ticket)) {
      throw new Error("Invalid delegated File/Review docking invitation.");
    }
    const receiver = await api("/api/dock/surface/receive", { ticket });
    const resource = receiver?.resource;
    const kind = ["file", "review"].includes(resource?.kind) ? resource.kind : null;
    const target = kind === "file" ? {
      source_id: resource?.source_id, scope_id: resource?.scope_id, path: resource?.path,
    } : kind === "review" ? {
      source_id: resource?.source_id, scope_id: resource?.scope_id, review_id: resource?.review_id,
    } : null;
    if (!kind || !target || !opaqueIdSyntax.test(target.source_id) || !opaqueIdSyntax.test(target.scope_id)
      || (kind === "file" ? !validPath(target.path) : !opaqueIdSyntax.test(target.review_id))
      || !ticketSyntax.test(receiver.window_id) || !ticketSyntax.test(receiver.window_proof)
      || typeof receiver.grant_generation !== "string"
      || !/^[1-9][0-9]{0,19}$/u.test(receiver.grant_generation)) {
      throw new Error("Delegated File/Review source is unavailable in this browser.");
    }
    const body = envelope(receiver, ticket);
    const authorityKey = `delegated-surface:${ticket}:${receiver.grant_generation}`;
    const reader = async (requested, options = {}) => {
      const command = requested?.request;
      const exact = requested?.op === "view"
        && requested.source_id === target.source_id && requested.scope_id === target.scope_id
        && (kind === "file"
          ? command?.op === "file" && command.path === target.path && Object.keys(command).length === 2
          : command?.op === "review" && command.review_id === target.review_id && Object.keys(command).length === 2);
      if (!exact) throw Object.assign(new Error("Delegated document target changed."), { status: 403 });
      return api("/api/dock/surface/read", body, 15000, options);
    };
    const documentId = views.openDelegatedDocument(kind, target, reader, authorityKey);
    onReceiveDocument(documentId, kind, target);
    let acceptanceAttempted = false;
    try {
      if (!await views.refreshDocument(documentId) || !views.getDocument(documentId)?.data) {
        const error = new Error("Delegated File/Review read failed authorization.");
        error.status = 403;
        throw error;
      }
      const until = Date.now() + Math.min(Math.max(timeoutMs, 1000), MAX_WAIT_MS);
      while (Date.now() < until && !disposed) {
        const item = views.getDocument(documentId);
        if (!item?.data || !sameTarget(item.target, target, kind) || item.unavailable || item.error) break;
        if (!isRendered(documentId)) {
          await sleep(120);
          continue;
        }
        if (receiver.presentation) {
          const presentation = boundedPresentation(kind, receiver.presentation);
          const fingerprint = await documentFingerprint(kind, item.data);
          if (fingerprint !== presentation.fingerprint) {
            throw new Error("The delegated source content changed during handoff; retry from the source document.");
          }
          if (!await restore(documentId, presentation)) {
            throw new Error("Receiving delegated editor is no longer available to restore.");
          }
          const current = views.getDocument(documentId);
          if (current !== item || !sameTarget(current?.target, target, kind) || !isRendered(documentId)) {
            throw new Error("Receiving delegated document changed during presentation restoration.");
          }
        }
        acceptanceAttempted = true;
        const accepted = await api("/api/dock/surface/accept", body);
        if (accepted.state !== "accepted") {
          throw Object.assign(new Error("Owner rejected delegated File/Review acceptance."), { status: 409 });
        }
        while (Date.now() < until && !disposed) {
          const document = views.getDocument(documentId);
          if (!document?.data || !sameTarget(document.target, target, kind)) break;
          const state = await api("/api/dock/surface/heartbeat", body);
          if (state.state === "released") return { documentId, kind };
          if (!["confirming", "accepted"].includes(state.state)) break;
          await sleep(250);
        }
        break;
      }
      throw new Error("Delegated File/Review release was not confirmed; receiver remains inspectable until authority expiry.");
    } catch (error) {
      const knownFailure = Number.isInteger(error?.status) && error.status >= 400 && error.status < 500;
      if (!acceptanceAttempted || knownFailure) views.closeDocument(documentId);
      throw error;
    }
  }

  return Object.freeze({
    stage, cancelStage, send, receive,
    dispose() { disposed = true; staged = null; },
  });
}

const agentDockPrefix = "#aw-agent-dock=";

export function takeAgentDockTicket(location, history) {
  if (!location?.hash?.startsWith(agentDockPrefix)) return null;
  const ticket = location.hash.slice(agentDockPrefix.length);
  history.replaceState(null, "", location.pathname + location.search);
  return ticketSyntax.test(ticket) ? ticket : null;
}

// Distinct-Actor Agent-view docking uses the same owner-confirmed lifecycle as
// File/Review docking, but the receiver's only reader is the delegated ticket
// endpoint. The ticket remains correlation; every refresh is server-authorized.
export function createAgentDocking({
  api, agentViews, popup,
  sleep = ms => new Promise(resolve => setTimeout(resolve, ms)),
  timeoutMs = MAX_WAIT_MS, onProgress = () => {}, onLinkReady = () => {},
  onReceiveView = () => {},
  makeLink = ticket => new URL(agentDockPrefix + ticket, location.href).href,
  navigate = (window, ticket) => {
    window.opener = null;
    window.location.replace(new URL(agentDockPrefix + ticket, location.href).href);
  },
}) {
  if (typeof api !== "function" || !agentViews
    || !["get", "openDelegated", "refresh", "close"].every(name => typeof agentViews[name] === "function")
    || [popup, sleep, onProgress, onLinkReady, onReceiveView, makeLink, navigate]
      .some(callback => typeof callback !== "function")) {
    throw new Error("Delegated Agent docking needs the read-only Agent view owner.");
  }
  let disposed = false;
  const envelope = (window, ticket) => ({
    window_id: window.window_id, window_proof: window.window_proof, ticket,
  });
  const sameTarget = (left, right) => left && right
    && ["store_id", "source_epoch", "session_id", "branch_id"].every(key => left[key] === right[key]);
  const validTarget = target => target && Object.keys(target).length === 4
    && ["store_id", "source_epoch", "session_id", "branch_id"].every(key => agentIdSyntax.test(target[key]));
  const safeClose = window => {
    try { window?.close?.(); } catch { /* already closed */ }
  };

  async function send(viewId, recipientId, { external = false } = {}) {
    const initial = agentViews.get(viewId);
    if (disposed || !initial?.snapshot || initial.pending || initial.status !== "ready"
      || !validTarget(initial.target) || !opaqueIdSyntax.test(recipientId)) {
      throw new Error("Only a loaded read-only Agent view can be delegated.");
    }
    const sameView = () => {
      const current = agentViews.get(viewId);
      return current && sameTarget(current.target, initial.target) && current.snapshot === initial.snapshot
        && current.status === "ready" && !current.pending;
    };
    const nextWindow = external ? null : popup();
    if (!external && !nextWindow) throw new Error("The browser blocked the new window. Allow pop-ups for the local owner.");
    let origin, ticket, released = false, navigated = false, requestedConfirmation = false;
    onProgress(viewId, true);
    try {
      if (!sameView()) throw new Error("The original Agent view changed during handoff.");
      origin = await api("/api/dock/agent/register", {});
      if (origin.store_id !== initial.target.store_id || origin.source_epoch !== initial.target.source_epoch) {
        throw new Error("Agent source changed before delegated handoff preparation.");
      }
      const prepared = await api("/api/dock/agent/prepare", {
        window_id: origin.window_id, window_proof: origin.window_proof,
        store_id: initial.target.store_id, source_epoch: initial.target.source_epoch,
        session_id: initial.target.session_id, branch_id: initial.target.branch_id,
        document_id: viewId, recipient_id: recipientId,
      });
      ticket = prepared.ticket;
      if (!ticketSyntax.test(ticket) || !sameView()) throw new Error("Invalid delegated Agent handoff identity.");
      if (external) onLinkReady(viewId, makeLink(ticket));
      else navigate(nextWindow, ticket);
      navigated = true;
      const until = Date.now() + Math.min(Math.max(timeoutMs, 1000), MAX_WAIT_MS);
      while (Date.now() < until && !disposed) {
        if (!sameView() || (nextWindow && nextWindow.closed)) break;
        const result = await api("/api/dock/agent/status", envelope(origin, ticket));
        if (result.document_id !== viewId) throw new Error("Delegated Agent handoff target changed.");
        if (result.state === "confirming") {
          if (!requestedConfirmation) {
            const acknowledgement = await api("/api/dock/agent/confirm", envelope(origin, ticket));
            if (acknowledgement.state !== "confirming") throw new Error("Delegated recipient confirmation was rejected.");
            requestedConfirmation = true;
          }
          await sleep(250);
          continue;
        }
        if (result.state === "accepted") {
          if (!sameView()) break;
          const outcome = await api("/api/dock/agent/release", envelope(origin, ticket));
          if (outcome.state !== "released" || outcome.document_id !== viewId) {
            throw new Error("Delegated Agent handoff was not released by the owner.");
          }
          released = true;
          if (sameView()) agentViews.close(viewId);
          return true;
        }
        if (result.state === "revoked") throw Object.assign(new Error("Delegated Agent grant was revoked."), { status: 403 });
        if (result.state !== "prepared") break;
        await sleep(250);
      }
      throw new Error("Delegated Agent handoff not accepted; the original view remains open.");
    } catch (error) {
      if (!released && origin && ticket) {
        try { await api("/api/dock/agent/cancel", envelope(origin, ticket)); } catch {}
      }
      if (!navigated) safeClose(nextWindow);
      throw error;
    } finally {
      if (external) onLinkReady(viewId, null);
      onProgress(viewId, false);
    }
  }

  async function receive(ticket) {
    if (disposed || !ticketSyntax.test(ticket)) throw new Error("Invalid delegated Agent docking invitation.");
    const receiver = await api("/api/dock/agent/receive", { ticket });
    const target = receiver?.resource?.kind === "agent_view" ? {
      store_id: receiver.resource.store_id,
      source_epoch: receiver.resource.source_epoch,
      session_id: receiver.resource.session_id,
      branch_id: receiver.resource.branch_id,
    } : null;
    if (!validTarget(target) || !ticketSyntax.test(receiver.window_id)
      || !ticketSyntax.test(receiver.window_proof)
      || typeof receiver.grant_generation !== "string" || !/^[1-9][0-9]{0,19}$/.test(receiver.grant_generation)) {
      throw new Error("Delegated Agent source is unavailable in this browser.");
    }
    const body = envelope(receiver, ticket);
    const authorityKey = `delegated:${ticket}:${receiver.grant_generation}`;
    const reader = async (requested, options = {}) => {
      if (!sameTarget(requested, target)) throw Object.assign(new Error("Delegated Agent target changed."), { status: 403 });
      return api("/api/dock/agent/read", body, 15000, options);
    };
    const viewId = agentViews.openDelegated(target, reader, authorityKey);
    onReceiveView(viewId, target);
    let acceptanceAttempted = false;
    try {
      if (!await agentViews.refresh(viewId) || !agentViews.get(viewId)?.snapshot) {
        const error = new Error("Delegated Agent read failed authorization.");
        error.status = 403;
        throw error;
      }
      acceptanceAttempted = true;
      const accepted = await api("/api/dock/agent/accept", body);
      if (accepted.state !== "accepted") throw Object.assign(new Error("Owner rejected delegated Agent acceptance."), { status: 409 });
      const until = Date.now() + Math.min(Math.max(timeoutMs, 1000), MAX_WAIT_MS);
      while (Date.now() < until && !disposed) {
        const view = agentViews.get(viewId);
        if (!view?.snapshot || !sameTarget(view.target, target)) break;
        const state = await api("/api/dock/agent/heartbeat", body);
        if (state.state === "released") return { viewId };
        if (!["confirming", "accepted"].includes(state.state)) break;
        await sleep(250);
      }
      throw new Error("Delegated Agent release was not confirmed; receiver remains inspectable until authority expiry.");
    } catch (error) {
      const knownFailure = Number.isInteger(error?.status) && error.status >= 400 && error.status < 500;
      if (!acceptanceAttempted || knownFailure) agentViews.close(viewId);
      throw error;
    }
  }

  return Object.freeze({ send, receive, dispose() { disposed = true; } });
}
