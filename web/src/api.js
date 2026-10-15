export async function api(path, body, timeout = 15000) {
  const control = new AbortController();
  const timer = setTimeout(() => control.abort(), timeout);
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
    try { data = await response.json(); } catch {}
    if (!response.ok) {
      const error = new Error(typeof data.error === "string" ? data.error : "Request failed.");
      error.status = response.status;
      throw error;
    }
    return data;
  } finally {
    clearTimeout(timer);
  }
}

export const workspaceAction = body => api("/api/workspace/action", body);
