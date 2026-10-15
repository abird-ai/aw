(() => {
  const root = document.documentElement;
  const key = "aw.theme.preference";
  const media = window.matchMedia?.("(prefers-color-scheme: dark)");
  const normalize = value => ["system", "light", "dark"].includes(value) ? value : "system";
  const effective = theme => theme === "system" ? (media?.matches ? "dark" : "light") : theme;

  const apply = (value, persist = false) => {
    const theme = normalize(value);
    const resolved = effective(theme);
    root.dataset.theme = theme;
    root.dataset.effectiveTheme = resolved;
    document.querySelector('meta[name="theme-color"]')?.setAttribute(
      "content",
      resolved === "dark" ? "#181c1a" : "#fcfcfb",
    );
    if (persist) {
      try { localStorage.setItem(key, theme); } catch {}
    }
    window.dispatchEvent(new CustomEvent("aw-theme-change", {
      detail: { theme, effective: resolved },
    }));
    return resolved;
  };

  window.__awApplyTheme = apply;
  let cached = "system";
  try { cached = normalize(localStorage.getItem(key)); } catch {}
  apply(cached, false);

  media?.addEventListener?.("change", () => {
    if (root.dataset.theme === "system") apply("system", false);
  });

  window.__awThemeReady = (async () => {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 800);
    try {
      const response = await fetch("/api/settings", {
        credentials: "same-origin",
        cache: "no-store",
        signal: controller.signal,
      });
      if (response.ok) {
        const value = await response.json();
        apply(value?.ui?.theme || "system", true);
      }
    } catch {}
    finally {
      clearTimeout(timer);
      root.classList.add("aw-theme-resolved");
    }
  })();
})();
