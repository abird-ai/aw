export async function api(path, body, timeout = 15000, { signal } = {}) {
  const control = new AbortController();
  const abort = () => control.abort();
  if (signal?.aborted) abort();
  else signal?.addEventListener("abort", abort, { once: true });
  const timer = setTimeout(abort, timeout);
  try {
    const response = await fetch(path, {
      method: body === undefined ? "GET" : "POST",
      credentials: "same-origin",
      cache: "no-store",
      headers: body === undefined ? {} : { "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: control.signal,
    });
    let data = {};
    try { data = await response.json(); } catch (error) {
      if (control.signal.aborted) throw control.signal.reason || error;
    }
    if (control.signal.aborted) throw control.signal.reason;
    if (!response.ok) {
      const error = new Error(typeof data.error === "string" ? data.error : "Request failed.");
      error.status = response.status;
      throw error;
    }
    return data;
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener("abort", abort);
  }
}

export const workspaceAction = body => api("/api/workspace/action", body);
