import { createEffect, onCleanup, onMount } from "solid-js";
import { Compartment, EditorState, StateEffect, StateField } from "@codemirror/state";
import { Decoration, EditorView, lineNumbers } from "@codemirror/view";
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

async function applyShiki(view, path) {
  const generation = (highlightGeneration.get(view) || 0) + 1;
  highlightGeneration.set(view, generation);
  const code = view.state.doc.toString();
  const theme = effectiveTheme();

  // Plain text is the fail-safe presentation while async syntax work is pending.
  // Never leave decorations from another file/theme attached to the current doc.
  if (!view.destroyed) view.dispatch({ effects: setShiki.of(Decoration.none) });
  if (!code || code.length > 2 * 1024 * 1024) return;

  try {
    const instance = await highlighter;
    if (
      view.destroyed ||
      highlightGeneration.get(view) !== generation ||
      view.state.doc.toString() !== code
    ) return;
    if (effectiveTheme() !== theme) {
      void applyShiki(view, path);
      return;
    }
    const result = instance.codeToTokens(code, {
      lang: languageFor(path),
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

function labelAttributes(label) {
  return EditorView.contentAttributes.of({
    "aria-label": label,
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
  ];
}

export function CodeFile(props) {
  let host;
  let view;
  let mounted = false;
  let currentPath = "";
  const labelCompartment = new Compartment();

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
    void applyShiki(view, props.path);
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
  });

  onCleanup(() => {
    window.removeEventListener(THEME_EVENT, recolor);
    view?.destroy();
  });
  return <div class="code-editor" ref={host} />;
}

export function CodeDiff(props) {
  let host;
  let merge;
  let mounted = false;

  const recolor = () => {
    if (!merge) return;
    const path = props.review?.path || "";
    void applyShiki(merge.a, path);
    void applyShiki(merge.b, path);
  };

  const rerender = () => {
    if (!mounted || !host) return;
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
    void applyShiki(merge.a, path);
    void applyShiki(merge.b, path);
  };

  onMount(() => {
    mounted = true;
    rerender();
    window.addEventListener(THEME_EVENT, recolor);
  });
  createEffect(() => {
    void props.review;
    rerender();
  });
  onCleanup(() => {
    window.removeEventListener(THEME_EVENT, recolor);
    merge?.destroy();
  });

  return <div class="code-diff" ref={host} />;
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
