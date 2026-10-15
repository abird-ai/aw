// Pure UI projection helpers. No provider calls, cookies or DOM access here.
export const MAX_PROMPT_BYTES = 128 * 1024;
export const utf8Bytes = (text) => new TextEncoder().encode(text).length;
export const canSend = (snapshot, posting, text) => Boolean(snapshot) && !snapshot.pending && !snapshot.needs_reconciliation && !posting && text.trim().length > 0 && utf8Bytes(text) <= MAX_PROMPT_BYTES;
export const isSubmitKey = (event) => event.key === 'Enter' && !event.shiftKey && !event.isComposing && event.keyCode !== 229;
export const needsDraftPublicationConsent = (text, active) => !active && String(text || '').trim().length > 0;
export function acceptSnapshot(current, next) {
  if (!next || typeof next.instance !== 'string' || !Number.isSafeInteger(next.sequence) || !Number.isSafeInteger(next.revision) || !Array.isArray(next.turns)) throw new Error('Invalid session snapshot.');
  if (next.saved && (typeof next.saved.session_id !== 'string' || typeof next.saved.branch_id !== 'string' || typeof next.saved.head !== 'string')) throw new Error('Invalid saved-session target.');
  if (current && current.instance === next.instance && next.sequence < current.sequence) return current;
  return next;
}
export function acceptDelta(current, sequence, delta) {
  if (!current || !Number.isSafeInteger(sequence) || sequence <= current.sequence) return { snapshot: current, resync: false, applied: false };
  if (sequence !== current.sequence + 1 || !current.pending || current.pending.run_id !== delta.run_id || typeof delta.text !== 'string') return { snapshot: current, resync: true, applied: false };
  return { snapshot: { ...current, sequence, pending: { ...current.pending, answer: current.pending.answer + delta.text } }, resync: false, applied: true };
}
// Preserve the original body/id on an ambiguous POST. Never automatically resend.
export function submission(snapshot, prompt, requestId, choice = null) {
  if (!canSend(snapshot, false, prompt)) throw new Error('Session is busy or prompt is empty/too large.');
  const body = { request_id: requestId, expected_revision: snapshot.revision, prompt };
  if (snapshot.saved) { body.session_id = snapshot.saved.session_id; body.branch_id = snapshot.saved.branch_id; }
  if (choice) {
    if (!Array.isArray(snapshot.accounts) || !snapshot.accounts.some(a => a.id === choice.account_id && a.allowed_models?.includes(choice.model))) throw new Error('Account/model not in this caller’s permitted choices.');
    body.account_id = choice.account_id; body.model = choice.model;
  }
  return body;
}

// Rendering/typing has no credential acquisition or model-discovery side effect.
export function accountChoices(snapshot) {
  return (snapshot?.accounts || []).flatMap(a => (a.allowed_models || []).map(model => ({account_id: a.id, model, account_kind: a.account_kind || 'configured', inference_executor: a.inference_executor || 'unknown', payer: a.payer && ['user','shared'].includes(a.payer.kind) && typeof a.payer.id === 'string' ? {kind:a.payer.kind,id:a.payer.id} : null})));
}

// UI-only projection; switching a server-owned view cannot silently retarget an unsent draft.
export function draftTargetChanged(previous, next, text) {
  if (!previous || !text.trim()) return false;
  return previous.instance !== next.instance ||
    (previous.saved?.session_id || null) !== (next.saved?.session_id || null) ||
    (previous.saved?.branch_id || null) !== (next.saved?.branch_id || null);
}
export function submissionSettled(receipt) {
  return receipt?.state === 'settled' || receipt?.state === 'interrupted';
}

// Display identity only from the authenticated snapshot. Never copy it into a
// prompt/request body: the server derives authority from its session token.
export function principalLabel(snapshot) {
  const actor = snapshot?.identity?.actor;
  return typeof actor === 'string' && /^[A-Za-z0-9:._@-]{1,128}$/.test(actor) ? actor : 'Authenticated participant';
}
