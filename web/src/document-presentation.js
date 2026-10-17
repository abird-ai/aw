// Ephemeral, read-only editor presentation. Neither text nor a new source
// capability is transported: just bounded positions/Find intent and SHA-256
// of the original authorized snapshot for exact-receipt comparison.
const encoder = new TextEncoder();
const digestSyntax = /^[a-f0-9]{64}$/u;
const focusKinds = new Set(["none", "find", "pane0", "pane1"]);
const maximumOffset = 4 * 1024 * 1024;
const maximumScroll = 32 * 1024 * 1024;
const maximumHorizontal = 4 * 1024 * 1024;
const maxFindBytes = 1024;
const maxFindCharacters = 512;
const keysExactly = (value, names) => value && typeof value === "object"
  && !Array.isArray(value) && Object.keys(value).length === names.length
  && names.every(name => Object.hasOwn(value, name));
const integer = (value, max) => Number.isSafeInteger(value) && value >= 0 && value <= max;

export function boundedPresentation(kind, raw) {
  if (!["file", "review"].includes(kind)
    || !keysExactly(raw, ["version", "fingerprint", "query", "selected", "focus", "panes"])
    || raw.version !== 1 || !digestSyntax.test(raw.fingerprint)
    || typeof raw.query !== "string" || raw.query.length > maxFindCharacters
    || encoder.encode(raw.query).length > maxFindBytes
    || [...raw.query].some(char => /[\p{Cc}\p{Cf}]/u.test(char))
    || !Number.isSafeInteger(raw.selected) || raw.selected < -1 || raw.selected > 4095
    || !focusKinds.has(raw.focus)
    || (kind === "file" && raw.focus === "pane1")
    || !Array.isArray(raw.panes) || raw.panes.length !== (kind === "file" ? 1 : 2)
    || !raw.panes.every(pane =>
      keysExactly(pane, ["anchor", "head", "from", "top", "left"])
      && integer(pane.anchor, maximumOffset) && integer(pane.head, maximumOffset)
      && integer(pane.from, maximumOffset) && integer(pane.top, maximumScroll)
      && integer(pane.left, maximumHorizontal))) {
    throw new Error("Invalid bounded read-only presentation.");
  }
  // Clone only plain numeric fields; no references to source views, DOM or text.
  return Object.freeze({
    version: 1, fingerprint: raw.fingerprint, query: raw.query,
    selected: raw.selected, focus: raw.focus,
    panes: Object.freeze(raw.panes.map(pane => Object.freeze({
      anchor: pane.anchor, head: pane.head, from: pane.from,
      top: pane.top, left: pane.left,
    }))),
  });
}

export async function documentFingerprint(kind, data, subtle = globalThis.crypto?.subtle) {
  if (!subtle || typeof subtle.digest !== "function" || !data || !["file", "review"].includes(kind)) {
    throw new Error("Snapshot fingerprint unavailable; docking has not been attempted.");
  }
  if (kind === "file" && typeof data.text !== "string") {
    throw new Error("File snapshot unavailable.");
  }
  if (kind === "review" && (data.kind !== "review" || typeof data.id !== "string")) {
    throw new Error("Immutable Review snapshot unavailable.");
  }
  // The immutable Review envelope includes the exact displayed lines. Never
  // pass this input through the handoff API or URL; only its SHA-256 checksum.
  const content = kind === "file"
    ? "file\0" + data.text
    : "review\0" + JSON.stringify(data);
  const digest = await subtle.digest("SHA-256", encoder.encode(content));
  return [...new Uint8Array(digest)].map(byte => byte.toString(16).padStart(2, "0")).join("");
}
