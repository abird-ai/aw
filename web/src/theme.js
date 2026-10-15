export const THEME_EVENT = "aw-theme-change";

export function normalizeTheme(value) {
  return ["system", "light", "dark"].includes(value) ? value : "system";
}

export function effectiveTheme() {
  const value = document.documentElement.dataset.effectiveTheme;
  return value === "light" ? "light" : "dark";
}

export function applyThemePreference(value, { persist = false } = {}) {
  const theme = normalizeTheme(value);
  if (typeof window.__awApplyTheme === "function") {
    return window.__awApplyTheme(theme, persist);
  }
  const dark = window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;
  const effective = theme === "system" ? (dark ? "dark" : "light") : theme;
  document.documentElement.dataset.theme = theme;
  document.documentElement.dataset.effectiveTheme = effective;
  if (persist) {
    try { localStorage.setItem("aw.theme.preference", theme); } catch {}
  }
  window.dispatchEvent(new CustomEvent(THEME_EVENT, { detail: { theme, effective } }));
  return effective;
}

export function finishBoot() {
  const done = () => document.documentElement.classList.add("aw-app-ready");
  Promise.resolve(window.__awThemeReady)
    .catch(() => undefined)
    .finally(() => requestAnimationFrame(done));
}
