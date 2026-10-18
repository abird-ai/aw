// Factual tool outcomes for every browser surface. A color or symbol is
// secondary: the explicit status text always survives light/dark/mono and
// unknown or interrupted receipts never become "succeeded".
const knownStatuses = new Set([
  "running", "pending", "succeeded", "failed", "denied",
  "interrupted", "cancelled", "unknown",
]);
const failedStatuses = new Set(["failed", "denied", "interrupted"]);

export function toolPresentation(tool) {
  const raw = String(tool?.status ?? "unknown").toLowerCase();
  const status = knownStatuses.has(raw) ? raw : "unknown";
  if (failedStatuses.has(status) || tool?.output?.is_error === true) {
    return { tone: "failure", symbol: "×",
      status: failedStatuses.has(status) ? status : "failed" };
  }
  if (status === "succeeded") {
    return { tone: "success", symbol: "✓", status };
  }
  return { tone: "neutral",
    symbol: status === "unknown" ? "?" : status === "cancelled" ? "–" : "·",
    status };
}
