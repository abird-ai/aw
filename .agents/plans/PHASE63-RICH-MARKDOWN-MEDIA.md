# Phase 63 — Rich Markdown and inert media references

**Status:** complete; implementation committed/tagged and validated
**Implementation checkpoint:** `dc7114dd49a28f19338eea664fae66b3948a7472` (`phase-63`)
**Version:** 0.15.0
**SQLite schema:** v32 unchanged
**Authority expansion:** none

## Goal

Expand native and Solid Agent answer rendering beyond the Phase-52 core subset while preserving the same safety boundary: rendering is presentation only and never performs I/O or grants authority.

## Shared semantic target

Phase 63 adds or normalizes:

- Markdown tables;
- nested ordered/unordered lists with bounded indentation;
- task-list markers;
- strikethrough;
- image/media syntax as an inert **media reference**.

### Native

The terminal renderer remains bounded and cell-aware.

Tables are projected as textual rows using visible separators and remain width bounded.

Task markers render as:

```text
☐ pending
☑ complete
```

Nested list indentation is bounded to four levels.

Strikethrough uses the terminal crossed-out style when available while preserving visible text.

Image syntax:

```markdown
![diagram](https://example.test/a.png)
```

renders as text such as:

```text
[image: diagram] (https://example.test/a.png)
```

Native never fetches the resource and never emits OSC-8 terminal hyperlinks.

## Solid/browser

MarkdownIt remains the parser.

Its default table and strikethrough support remain enabled.

Task-list source markers are converted only after MarkdownIt's safe HTML generation.

The image renderer is overridden so valid image syntax produces an inert media-reference span with an explicit clickable link instead of an `<img>` element. Rendering therefore never initiates an image fetch.

Unsafe media destinations rejected by MarkdownIt's link policy remain literal Markdown text and never gain an href.

## HTML and URL safety

- raw Markdown HTML remains disabled in Solid;
- native HTML remains literal visible text;
- data URLs remain rejected;
- browser media links use `target="_blank"` and `rel="noopener noreferrer"`;
- terminal output remains sanitized and escape-free;
- no renderer performs filesystem/network I/O.

## Non-goals

Phase 63 does not add:

- automatic image fetching;
- inline terminal graphics protocols;
- OSC-8 terminal links;
- artifact upload/storage;
- arbitrary HTML;
- remote media proxying;
- filesystem indexing or LSP;
- Phase 64/65 work.

## Validation target

1. native tables/tasks/nesting/strike/media project to bounded rows;
2. native media rendering emits no terminal control sequence or I/O;
3. Solid renders tables and strikethrough;
4. Solid task markers are explicit and bounded;
5. Solid image syntax never emits `<img>`;
6. valid media references are explicit user-click links;
7. unsafe destinations remain literal/non-clickable;
8. existing Phase-52 Markdown and Phase-53 syntax-highlighting tests remain green;
9. Vite production build remains green;
10. schema remains v32.

## Next gate

skip 64/65 as explicitly requested and continue to Phase 66.
