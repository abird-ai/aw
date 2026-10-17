import { createEffect, createSignal, onCleanup, onMount } from "solid-js";
import { Compartment, EditorState, StateEffect, StateField } from "@codemirror/state";
import { Decoration, EditorView, drawSelection, lineNumbers } from "@codemirror/view";
import { MergeView } from "@codemirror/merge";
import { createHighlighterCore } from "@shikijs/core";
import { createJavaScriptRegexEngine } from "@shikijs/engine-javascript";
import rust from "@shikijs/langs/rust";
import javascript from "@shikijs/langs/javascript";
import typescript from "@shikijs/langs/typescript";
import jsx from "@shikijs/langs/jsx";
import tsx from "@shikijs/langs/tsx";
import jsonLang from "@shikijs/langs/json";
import jsonc from "@shikijs/langs/jsonc";
import markdown from "@shikijs/langs/markdown";
import toml from "@shikijs/langs/toml";
import yaml from "@shikijs/langs/yaml";
import bash from "@shikijs/langs/bash";
import python from "@shikijs/langs/python";
import css from "@shikijs/langs/css";
import html from "@shikijs/langs/html";
import nix from "@shikijs/langs/nix";
import githubDark from "@shikijs/themes/github-dark-default";
import githubLight from "@shikijs/themes/github-light-default";
import { effectiveTheme, THEME_EVENT } from "./theme.js";

const highlighter = createHighlighterCore({
  themes: [githubDark, githubLight],
  langs: [rust, javascript, typescript, jsx, tsx, jsonLang, jsonc, markdown, toml, yaml, bash, python, css, html, nix],
  engine: createJavaScriptRegexEngine(),
});

const setShiki = StateEffect.define();
const shikiField = StateField.define({
  create: () => Decoration.none,
  update(value, transaction) {
    value = value.map(transaction.changes);
    for (const effect of transaction.effects) {
      if (effect.is(setShiki)) value = effect.value;
    }
    return value;
  },
  provide: field => EditorView.decorations.from(field),
});

const setFind = StateEffect.define();
const findField = StateField.define({
  create: () => Decoration.none,
  update(value, transaction) {
    value = value.map(transaction.changes);
    for (const effect of transaction.effects) if (effect.is(setFind)) value = effect.value;
    return value;
  },
  provide: field => EditorView.decorations.from(field),
});

const editorTheme = EditorView.theme({
  "&": {
    height: "100%",
    background: "transparent",
    color: "var(--fg)",
    fontSize: "12.5px",
  },
  ".cm-scroller": {
    overflow: "auto",
    fontFamily: "Geist Mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
    lineHeight: "1.58",
  },
  ".cm-content": {
    padding: "12px 0 24px",
    color: "var(--fg)",
    caretColor: "var(--blue)",
  },
  ".cm-line": {
    padding: "0 16px 0 10px",
  },
  ".cm-gutters": {
    background: "var(--page)",
    color: "var(--faint)",
    border: "0",
    borderRight: "1px solid var(--border)",
    minWidth: "48px",
  },
  ".cm-lineNumbers .cm-gutterElement": {
    padding: "0 10px 0 8px",
    minWidth: "42px",
  },
  ".cm-activeLine, .cm-activeLineGutter": {
    background: "var(--editor-active-line)",
  },
  ".cm-selectionBackground, ::selection": {
    background: "var(--editor-selection) !important",
  },
  ".cm-cursor": {
    borderLeftColor: "var(--blue)",
  },
});

const highlightGeneration = new WeakMap();
const styleNonce = document.querySelector('meta[name="aw-style-nonce"]')?.content || "";
const syntaxClasses = new Map();
let syntaxStyles;

const languageByExtension = {
  rs: "rust",
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "jsx",
  ts: "typescript",
  mts: "typescript",
  cts: "typescript",
  tsx: "tsx",
  json: "json",
  jsonc: "jsonc",
  md: "markdown",
  markdown: "markdown",
  toml: "toml",
  yaml: "yaml",
  yml: "yaml",
  sh: "bash",
  bash: "bash",
  zsh: "bash",
  py: "python",
  css: "css",
  html: "html",
  htm: "html",
  nix: "nix",
};

function languageFor(path = "") {
  const name = path.split("/").at(-1)?.toLowerCase() || "";
  if (name === "cargo.lock" || name === "cargo.toml") return "toml";
  if (name === "flake.lock") return "json";
  const extension = name.includes(".") ? name.split(".").at(-1) : "";
  return languageByExtension[extension] || "text";
}

function fontStyle(bits = 0) {
  let style = "";
  if (bits & 1) style += "font-style:italic;";
  if (bits & 2) style += "font-weight:700;";
  if (bits & 4) style += "text-decoration:underline;";
  return style;
}

function syntaxClass(token) {
  // Colors come from bundled Shiki themes, never arbitrary source-text CSS.
  // Keep marks class-based: CSP nonces authorize stylesheets, not style attrs.
  if (!/^#(?:[0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/i.test(token.color || "")) return null;
  const declarations = `color:${token.color};${fontStyle(token.fontStyle)}`;
  let name = syntaxClasses.get(declarations);
  if (name) return name;
  if (!syntaxStyles) {
    syntaxStyles = document.createElement("style");
    if (styleNonce) syntaxStyles.nonce = styleNonce;
    document.head.append(syntaxStyles);
  }
  if (!syntaxStyles.sheet) return null;
  name = "aw-syntax-" + syntaxClasses.size;
  syntaxStyles.sheet.insertRule(`.${name}{${declarations}}`, syntaxStyles.sheet.cssRules.length);
  syntaxClasses.set(declarations, name);
  return name;
}

function applyShiki(view, path) {
  const previous = highlightGeneration.get(view);
  if (previous) previous.path = "";
  const generation = { path: path || "" };
  highlightGeneration.set(view, generation);
  return highlightView(view, generation);
}

async function highlightView(view, generation) {
  const theme = effectiveTheme();

  // Plain text is the fail-safe presentation while async syntax work is pending.
  // Never leave decorations from another file/theme attached to the current doc.
  if (!view.destroyed) view.dispatch({ effects: setShiki.of(Decoration.none) });
  if (!view.state.doc.length || view.state.doc.length > 2 * 1024 * 1024) return;

  try {
    const instance = await highlighter;
    if (
      view.destroyed ||
      highlightGeneration.get(view) !== generation
    ) return;
    if (effectiveTheme() !== theme) {
      void applyShiki(view, generation.path);
      return;
    }
    const code = view.state.doc.toString();
    const result = instance.codeToTokens(code, {
      lang: languageFor(generation.path),
      theme: theme === "dark" ? "github-dark-default" : "github-light-default",
    });
    const ranges = [];
    for (const line of result.tokens) {
      for (const token of line) {
        const from = token.offset;
        const to = Math.min(code.length, from + token.content.length);
        const className = syntaxClass(token);
        if (to <= from || !className) continue;
        ranges.push(
          Decoration.mark({ class: className }).range(from, to),
        );
      }
    }
    if (
      !view.destroyed &&
      highlightGeneration.get(view) === generation &&
      view.state.doc.toString() === code &&
      effectiveTheme() === theme
    ) {
      view.dispatch({ effects: setShiki.of(Decoration.set(ranges, true)) });
    }
  } catch {
    // Decoration.none was installed before the async work. The editor remains
    // readable with its CSS foreground even when a grammar/highlighter fails.
  }
}

function scrubEditor(view) {
  if (!view || view.destroyed) return;
  const generation = highlightGeneration.get(view);
  if (generation) generation.path = "";
  highlightGeneration.delete(view);
  // A pending highlighter may still hold this view after destroy. Replace its
  // immutable state first so retired editors retain no source or selection.
  view.setState(EditorState.create());
}

function labelAttributes(label) {
  return EditorView.contentAttributes.of({
    "aria-label": label,
    // A read-only CodeMirror document must still admit keyboard focus,
    // navigation and selection. EditorState.readOnly and editable:false
    // remain enforced; this does not grant write permission.
    tabindex: "0",
    spellcheck: "false",
  });
}

function extensions(label, labelCompartment) {
  return [
    lineNumbers(),
    EditorView.cspNonce.of(styleNonce),
    EditorState.readOnly.of(true),
    EditorView.editable.of(false),
    labelCompartment ? labelCompartment.of(labelAttributes(label)) : labelAttributes(label),
    editorTheme,
    shikiField,
    findField,
    drawSelection(),
  ];
}

function clearOwnedSelection(host) {
  const selection = window.getSelection();
  if (selection && (host?.contains(selection.anchorNode) || host?.contains(selection.focusNode))) {
    selection.removeAllRanges();
  }
}

function createCodeFind(views) {
  const [query, setQuery] = createSignal("");
  const [matches, setMatches] = createSignal([]);
  const [selected, setSelected] = createSignal(-1);
  const [limited, setLimited] = createSignal(false);
  let input;

  function paint(reveal = false) {
    const all = matches();
    for (const view of views()) {
      if (view.destroyed) continue;
      const ranges = all.flatMap((match, index) => match.view === view
        ? [Decoration.mark({ class: index === selected() ? "code-find-current" : "code-find-match" }).range(match.from, match.to)] : []);
      view.dispatch({ effects: setFind.of(Decoration.set(ranges, true)) });
    }
    const match = all[selected()];
    if (reveal && match && !match.view.destroyed) match.view.dispatch({
      selection: { anchor: match.from, head: match.to },
      effects: EditorView.scrollIntoView(match.from, { y: "center" }),
    });
  }

  function rebuild(reveal = false) {
    const found = [];
    const value = query();
    let capped = false;
    if (value) {
      const expression = new RegExp(value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "giu");
      search: for (const view of views()) {
        if (view.destroyed) continue;
        const text = view.state.doc.toString();
        expression.lastIndex = 0;
        for (let match; (match = expression.exec(text));) {
          if (found.length === 4096) { capped = true; break search; }
          found.push({ view, from: match.index, to: match.index + match[0].length });
        }
      }
    }
    setMatches(found);
    setLimited(capped);
    setSelected(found.length ? Math.min(Math.max(0, selected()), found.length - 1) : -1);
    paint(reveal);
  }

  function change(value) {
    setQuery(value.slice(0, 512));
    setSelected(0);
    rebuild(true);
  }
  function step(direction) {
    const length = matches().length;
    if (!length) return;
    setSelected((selected() + direction + length) % length);
    paint(true);
  }
  function clear() {
    setQuery("");
    setMatches([]);
    setSelected(-1);
    setLimited(false);
    paint();
  }
  function focus() { input?.focus(); input?.select(); }
  function focusRestored() { input?.focus({ preventScroll: true }); }
  function capture() {
    return { query: query(), selected: selected(), focused: document.activeElement === input };
  }
  function restore(queryValue, selectedIndex) {
    // Never implicitly reveal the first match: the independently restored
    // viewport, caret and pane focus are the explicit presentation authority.
    setQuery(queryValue);
    setSelected(selectedIndex);
    rebuild(false);
  }
  function Toolbar() {
    return <div class="code-find" role="search" aria-label="Find in this document">
      <input ref={input} aria-label="Find in this document" placeholder="Find in this document" maxlength="512" value={query()}
        onInput={event => change(event.currentTarget.value)} onKeyDown={event => {
          if (event.isComposing) return;
          if (event.key === "Enter") { event.preventDefault(); step(event.shiftKey ? -1 : 1); }
          else if (event.key === "Escape") { event.preventDefault(); clear(); }
        }} />
      <span class="code-find-count" role="status">{query() ? (matches().length ? `${selected() + 1} of ${matches().length}${limited() ? "+" : ""}` : "No matches") : ""}</span>
      <button class="mini-icon" type="button" aria-label="Previous match" disabled={!matches().length} onClick={() => step(-1)}>↑</button>
      <button class="mini-icon" type="button" aria-label="Next match" disabled={!matches().length} onClick={() => step(1)}>↓</button>
      <button class="mini-icon" type="button" aria-label="Clear Find" disabled={!query()} onClick={clear}>×</button>
    </div>;
  }
  return { Toolbar, rebuild, clear, focus, focusRestored, capture, restore };
}

function captureCodePresentation(views, find) {
  const panes = views();
  if (!panes.length || panes.some(view => view.destroyed)) return null;
  const search = find.capture();
  const focused = panes.findIndex(view => view.hasFocus);
  return {
    query: search.query, selected: search.selected,
    focus: search.focused ? "find" : focused < 0 ? "none" : `pane${focused}`,
    panes: panes.map(view => ({
      anchor: view.state.selection.main.anchor,
      head: view.state.selection.main.head,
      from: view.viewport.from,
      top: Math.round(view.scrollDOM.scrollTop),
      left: Math.round(view.scrollDOM.scrollLeft),
    })),
  };
}
async function restoreCodePresentation(views, find, snapshot) {
  const panes = views();
  if (!snapshot || panes.length !== snapshot.panes.length
    || panes.some(view => view.destroyed)
    || snapshot.panes.some((pane, index) => pane.anchor > panes[index].state.doc.length
      || pane.head > panes[index].state.doc.length
      || pane.from > panes[index].state.doc.length)) {
    throw new Error("The original editor positions no longer match this document.");
  }
  find.restore(snapshot.query, snapshot.selected);
  for (let index = 0; index < panes.length; index++) {
    const pane = snapshot.panes[index];
    panes[index].dispatch({ selection: { anchor: pane.anchor, head: pane.head } });
  }
  // Wait until the *receiver* CodeMirror owns/layouts its independently read
  // document. A different viewport may clamp pixel offsets, but the selected
  // text positions are always exact to the verified document fingerprint.
  await new Promise(resolve => requestAnimationFrame(resolve));
  for (let index = 0; index < panes.length; index++) {
    const view = panes[index];
    if (view.destroyed) throw new Error("Receiving editor was closed.");
    const pane = snapshot.panes[index];
    view.scrollDOM.scrollTop = pane.top;
    view.scrollDOM.scrollLeft = pane.left;
    view.requestMeasure();
  }
  if (snapshot.focus === "find") find.focusRestored();
  else if (snapshot.focus === "pane0") panes[0]?.focus();
  else if (snapshot.focus === "pane1") panes[1]?.focus();
  return true;
}

function ownEditorLifecycle(props, host, views, find) {
  const scroll = new Map();
  function attach() {
    const current = new Set(views());
    for (const [view, saved] of scroll) if (!current.has(view)) {
      view.scrollDOM.removeEventListener("scroll", saved.listener);
      scroll.delete(view);
    }
    for (const view of current) if (!scroll.has(view)) {
      const saved = { top: view.scrollDOM.scrollTop, left: view.scrollDOM.scrollLeft };
      saved.listener = () => {
        if (props.active !== false) { saved.top = view.scrollDOM.scrollTop; saved.left = view.scrollDOM.scrollLeft; }
      };
      scroll.set(view, saved);
      view.scrollDOM.addEventListener("scroll", saved.listener, { passive: true });
    }
  }
  const keydown = event => {
    if (event.defaultPrevented || event.isComposing || event.altKey || props.active === false
      || !((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f")
      || [...document.querySelectorAll("dialog[open], [role=dialog]")]
        .some(dialog => !dialog.closest("[hidden]") && dialog.getClientRects().length)) return;
    event.preventDefault();
    find.focus();
  };
  onMount(() => window.addEventListener("keydown", keydown));
  createEffect(() => {
    if (props.active === false) clearOwnedSelection(host());
    else queueMicrotask(() => {
      if (props.active === false) return;
      attach();
      for (const [view, saved] of scroll) if (!view.destroyed) {
        view.scrollDOM.scrollTop = saved.top;
        view.scrollDOM.scrollLeft = saved.left;
        view.requestMeasure();
      }
    });
  });
  onCleanup(() => {
    window.removeEventListener("keydown", keydown);
    clearOwnedSelection(host());
    find.clear();
    for (const [view, saved] of scroll) view.scrollDOM.removeEventListener("scroll", saved.listener);
    scroll.clear();
  });
  return { attach };
}

export function CodeFile(props) {
  let host;
  let view;
  let mounted = false;
  let currentPath = "";
  const labelCompartment = new Compartment();
  const views = () => view ? [view] : [];
  const find = createCodeFind(views);
  const lifecycle = ownEditorLifecycle(props, () => host, views, find);

  const recolor = () => {
    if (view) void applyShiki(view, props.path || "");
  };

  onMount(() => {
    mounted = true;
    currentPath = props.path || "";
    view = new EditorView({
      state: EditorState.create({
        doc: props.text || "",
        extensions: extensions(`Read-only file ${props.path || ""}`, labelCompartment),
      }),
      parent: host,
    });
    lifecycle.attach();
    void applyShiki(view, props.path);
    find.rebuild();
    props.onPresentationReady?.({
      capture: () => captureCodePresentation(views, find),
      restore: value => restoreCodePresentation(views, find, value),
    });
    window.addEventListener(THEME_EVENT, recolor);
  });

  createEffect(() => {
    const text = props.text || "";
    const path = props.path || "";
    if (!mounted || !view) return;
    const fileChanged = currentPath !== path;
    const textChanged = view.state.doc.toString() !== text;
    currentPath = path;
    if (textChanged) {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
    }
    if (fileChanged) {
      view.dispatch({ effects: [
        labelCompartment.reconfigure(labelAttributes(`Read-only file ${path}`)),
        EditorView.scrollIntoView(0, { x: "start", y: "start" }),
      ] });
      view.scrollDOM.scrollLeft = 0;
    }
    void applyShiki(view, path);
    if (textChanged || fileChanged) find.rebuild();
  });

  onCleanup(() => {
    props.onPresentationReady?.(null);
    window.removeEventListener(THEME_EVENT, recolor);
    const retired = view;
    view = null;
    scrubEditor(retired);
    retired?.destroy();
  });
  return <div class="code-document">
    <find.Toolbar />
    <div class="code-editor" ref={host} onCopy={event => {
      if (props.active === false || props.onCheckAccess?.() === false) {
        event.preventDefault();
        clearOwnedSelection(host);
      }
    }} />
  </div>;
}

export function CodeDiff(props) {
  let host;
  let merge;
  let mounted = false;
  let currentReview;
  const views = () => merge ? [merge.a, merge.b] : [];
  const find = createCodeFind(views);
  const lifecycle = ownEditorLifecycle(props, () => host, views, find);

  const recolor = () => {
    if (!merge) return;
    const path = props.review?.path || "";
    void applyShiki(merge.a, path);
    void applyShiki(merge.b, path);
  };

  const rerender = () => {
    if (!mounted || !host) return;
    if (merge && currentReview === props.review) return;
    currentReview = props.review;
    clearOwnedSelection(host);
    if (merge) { scrubEditor(merge.a); scrubEditor(merge.b); }
    merge?.destroy();
    host.replaceChildren();
    const pair = reviewToDocuments(props.review);
    const path = props.review?.path || "";
    merge = new MergeView({
      parent: host,
      a: {
        doc: pair.before,
        extensions: extensions(`Before ${path}`),
      },
      b: {
        doc: pair.after,
        extensions: extensions(`After ${path}`),
      },
      orientation: "a-b",
      highlightChanges: true,
      gutter: true,
      collapseUnchanged: { margin: 3, minSize: 7 },
    });
    lifecycle.attach();
    void applyShiki(merge.a, path);
    void applyShiki(merge.b, path);
    find.rebuild();
  };

  onMount(() => {
    mounted = true;
    rerender();
    props.onPresentationReady?.({
      capture: () => captureCodePresentation(views, find),
      restore: value => restoreCodePresentation(views, find, value),
    });
    window.addEventListener(THEME_EVENT, recolor);
  });
  createEffect(() => {
    void props.review;
    rerender();
  });
  onCleanup(() => {
    props.onPresentationReady?.(null);
    window.removeEventListener(THEME_EVENT, recolor);
    currentReview = null;
    const retired = merge;
    merge = null;
    if (retired) { scrubEditor(retired.a); scrubEditor(retired.b); retired.destroy(); }
  });

  return <div class="code-document">
    <find.Toolbar />
    <div class="code-diff" ref={host} onCopy={event => {
      if (props.active === false || props.onCheckAccess?.() === false) {
        event.preventDefault();
        clearOwnedSelection(host);
      }
    }} />
  </div>;
}

function reviewToDocuments(review = {}) {
  const before = [];
  const after = [];
  let sawHunk = false;
  for (const line of review.lines || []) {
    const kind = String(line.kind || "").toLowerCase();
    let text = String(line.text || "");
    if (kind === "header" || kind === "notice") continue;
    if (kind === "hunk") {
      if (sawHunk) {
        before.push("⋯");
        after.push("⋯");
      }
      sawHunk = true;
      continue;
    }
    if (text.startsWith("+") || text.startsWith("-") || text.startsWith(" ")) {
      text = text.slice(1);
    }
    if (kind === "added") after.push(text);
    else if (kind === "removed") before.push(text);
    else {
      before.push(text);
      after.push(text);
    }
  }
  if (!before.length && !after.length) {
    const raw = (review.lines || []).map(line => String(line.text || "")).join("\n");
    return { before: raw, after: raw };
  }
  return { before: before.join("\n"), after: after.join("\n") };
}
