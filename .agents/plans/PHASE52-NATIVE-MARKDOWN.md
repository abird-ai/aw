# Phase 52 — safe native Markdown answer rendering

Status: authorized 2026-10-06 by the user after Phase 51 review.

## Boundary

Phase 52 is **native presentation only**. SQLite remains **v32**.

It adds no provider/account/tool/routing/subagent/planner/guardian authority, no model-context mutation, no browser redesign, no filesystem/network lookup, no terminal hyperlink activation, no arbitrary ANSI, and no execution semantics.

## Goal

Settled Agent answers should read like the supplied Pi/Codex references rather than raw Markdown source while preserving the low-chrome Phase-51 transcript.

The same bounded renderer is used by inline finalized scrollback and fullscreen Agent history.

Live partial streaming remains sanitized plain text until the answer settles. This avoids reparsing incomplete Markdown on every token and prevents unstable partial formatting.

## Supported native Markdown subset

Render safely:

- ATX headings `#` through `######`;
- bold `**text**` / `__text__`;
- emphasis `*text*` / `_text_`;
- inline code with backticks;
- unordered lists using `-`, `*`, or `+`;
- ordered lists using `N.`;
- block quotes using `>`;
- fenced code blocks using triple backticks or tildes, with an optional language label;
- horizontal rules;
- ordinary paragraphs and blank lines;
- Markdown links as visible label plus a muted textual destination, never as an OSC-8/terminal hyperlink.

Unsupported or malformed markup remains visible literal text. Raw HTML is never interpreted.

## Safety

Before Markdown parsing, model answer text continues through the existing display sanitizer.

The renderer emits only Ratatui text/style objects.

It must never:

- forward ESC/OSC/DCS/APC/PM bytes;
- interpret HTML;
- emit OSC-8 links;
- open URLs;
- read files;
- run syntax highlighters that inspect the filesystem;
- execute commands;
- turn code/link text into an aw command;
- expose private provider reasoning.

Link destinations are display-only sanitized text.

## Styling

Use the existing Phase-51 semantic palette:

- headings: accent + bold;
- ordinary text: foreground;
- strong: foreground + bold;
- emphasis: foreground + italic;
- inline code: neutral code chip;
- fenced code: full-width muted neutral code strip;
- quote marker: accent/muted quote text;
- list marker: accent;
- link label: normal/underlined; destination: faint/muted;
- horizontal rule: faint.

No large cards, heavy borders, HTML-like boxes, or bright syntax rainbow.

## Geometry

The renderer is width-bounded and Unicode-cell aware.

Fullscreen search/copy/find-row geometry derives from the exact rendered Markdown lines, not the raw source line count.

Private internal markers used by fullscreen transcript assembly are decoded before selection/copy and never appear in clipboard text.

## Transcript rules

Only assistant answer blocks receive Markdown rendering.

Do not Markdown-render:

- user prompts;
- tool commands/results;
- Thinking labels;
- errors;
- status/performance rows;
- file/diff/terminal content.

Authoritative `Turn.answer` fallback uses the same Markdown renderer.

## Validation

At minimum:

- inline and fullscreen call one shared Markdown renderer;
- headings/bold/emphasis/inline-code/list/quote/fenced-code/rule/link fixtures render deterministically;
- malformed markup remains visible;
- HTML is literal;
- ESC/OSC controls are escaped before rendering;
- link destinations are visible text only and no OSC-8 sequence exists;
- fenced code preserves spaces and uses neutral background;
- code/list/quote wrapping is width bounded;
- fullscreen rendered-row plain text and styled row counts match exactly;
- selection/copy contains visible rendered text and no private markers;
- live partial stream remains plain/sanitized;
- tools remain semantic Phase-51 strips, not Markdown;
- Phase-51 PTY switching/menu regressions remain green;
- Phase-50 authority checker remains green;
- schema remains v32;
- whole workspace/all targets, Clippy `-D warnings`, doctests/rustdoc, feature cuts, fullscreen PTY and inline PTY remain green.

## Deferred

- full CommonMark compliance;
- tables;
- footnotes;
- images;
- OSC-8 hyperlinks;
- syntax-highlighted assistant code fences;
- HTML rendering;
- browser Markdown convergence;
- incremental partial-token Markdown parsing;
- custom extension-provided Markdown widgets.
