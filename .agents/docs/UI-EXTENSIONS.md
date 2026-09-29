# UI extension architecture

Status: current post-Phase-37 product architecture. This document is authoritative for UI-extension ownership and supersedes any older generated artifact that implies status/performance rows are hardwired into a renderer.

## Intent

aw borrows the useful part of Pi's extension model: presentation features should be contributed through a stable extension boundary instead of growing special cases in the core chat loop.

The implementation is deliberately Rust-idiomatic:

- typed traits for compiled-in extensions today;
- typed, serde-stable request/response messages at the host boundary;
- bounded semantic contributions rather than arbitrary terminal drawing;
- one registry that validates manifests, slots, row/span bounds and extension identity;
- no Rust dynamic-library ABI promise;
- no extension gains execution authority merely because it can render UI.

Today every built-in extension is linked into the single `aw` binary. The protocol is intentionally shaped so a future external extension-host process can proxy the exact same requests without changing the registry, slots, or TUI/workspace renderers.

## Crate boundary

`crates/aw-ui-extension` owns the transport-neutral contract.

### Extension implementation

`UiExtension` is the in-process implementation trait:

- `manifest() -> Manifest`
- `project(slot, context) -> Vec<Contribution>`

Built-in extensions implement this trait.

### Host boundary

`ExtensionHost` is the registry-facing request/response boundary:

- `request(Request) -> Result<Response, RegistryError>`

`InProcessExtensionHost` wraps a compiled `UiExtension` today.

The registry also exposes `Registry::from_hosts(...)`. A future process-backed proxy can implement `ExtensionHost` by framing the existing serde `Request` / `Response` types over an IPC channel. A unit test intentionally registers a protocol-only host that does not implement `UiExtension`, proving the registry is not coupled to compiled extension objects.

No external extension process is implemented yet. When one is added, process lifecycle, framing, resource limits and sandbox policy belong to that host adapter, not to the TUI renderer.

## Slots

Current presentation slots are:

- `above-composer`
- `below-composer`
- `after-turn`
- `fullscreen-status`

The inline renderer asks the registry for above/below rows and allocates bounded terminal height. Fullscreen asks for fullscreen status rows. Finalized turn rendering asks for after-turn contributions.

The renderer does not know which extension produced a line.

## Contribution model

Extensions return semantic `Line` / `Span` data with tones, bold/italic hints and ordering. The registry enforces:

- at most 64 registered UI extensions;
- versioned/portable extension IDs;
- declared slot membership;
- at most 32 lines per contribution;
- at most 32 spans per line;
- bounded span text;
- no control characters other than tab;
- contribution extension ID must match the validated manifest;
- contribution slot must match the requested slot.

These are presentation contributions only. They are never model context, tool authority, provider credentials, filesystem/process/network authority, or agent hooks.

## Built-in status-line extension

`StatusLineExtension` owns status rows completely.

The canonical user config is `ui.status_lines`, an ordered list of:

- `region = "above-composer"`
- `region = "below-composer"`
- `region = "fullscreen-status"`

plus independent ordered `items` (left) and optional `right_items` groups.

Example:

```toml
[ui]
status_lines = [
  { region = "below-composer", items = ["directory", "agent"], right_items = ["tools", "state"] },
  { region = "below-composer", items = ["session-tps"], right_items = ["model", "provider", "effort"] },
  { region = "fullscreen-status", items = ["directory", "agent"], right_items = ["tools", "state"] },
  { region = "fullscreen-status", items = ["session-tps"], right_items = ["model", "provider", "effort"] },
]
```

The current built-in default is deliberately Pi-like: **two compact rows below the two-rule composer**, mirrored as two `fullscreen-status` rows in Workspace. The first line is directory + Agent identity on the left and tools/state on the right; the second is trustworthy current-process average output TPS when available on the left and model/provider/reasoning effort on the right. **The same left/right renderer is used for defaults and arbitrary custom rows**; there is no default-only layout branch.

Users may add/reorder rows without changing core TUI code. Validation is bounded: at most 32 rows, at most 16 combined left/right status items per row, and no duplicate item across either side of one row. Inline rendering currently displays at most eight rows above and eight below so a status extension cannot consume the terminal indefinitely.

The old singular `status_line = ["model", ...]` form is accepted on read for compatibility and normalized to one `above-composer` row. Both the historical three-below/one-fullscreen default and the immediately previous Phase-57/58 default migrate exactly to the current default; arbitrary custom left/right layouts are preserved. New saves emit `status_lines`.

Native Settings item toggles operate over this row model. The active Solid Settings UI exposes left/right membership, row placement, all four Agent/Workspace padding settings and performance/TPS/TTFT/waterfall visibility. The historical `web/settings.js` path remains compatibility/reference code rather than the active browser settings architecture.

## Built-in performance extension

`PerformanceExtension` owns the human-facing TPS/performance presentation completely.

Core code records only raw semantic turn measurements in `PerformanceStats`; `SessionPerformance` additionally keeps a non-durable in-process aggregate used only for truthful current-runtime average output TPS:

- elapsed duration;
- first-token duration when measured;
- input token count when available;
- output token count when available.

`PerformanceStats` also carries a typed throughput-eligibility bit. A sample contributes to TPS only when its elapsed/first-token timing belongs to **one model invocation with no tool execution interleaved**. Multi-round Agent loops, tool latency and later invocation TTFT are therefore not mislabeled as model throughput. `SessionPerformance` sums only eligible measured output-token counts and generation duration (elapsed minus first-token wait). It resets when the active Agent/Session changes and is intentionally not reconstructed from historical turns after restart because those turns do not persist timing.

The extension/presentation layer owns:

- tokens-per-second calculation;
- token-count formatting;
- TTFT text;
- elapsed-time text;
- optional waterfall rendering;
- tones/styles;
- placement in `after-turn`.

Per-turn transcript performance is still projected through the same extension registry `after-turn` slot in Inline and Workspace. The exact built-in two-row status layout additionally uses the shared `project_status(...)` presentation seam for runtime-session average TPS; ordinary core renderers do not calculate or format TPS themselves.

This keeps measurement/aggregation in the runtime and human formatting in the UI-extension presentation layer.

## Pi-like two-rule composer colors

The inline composer itself remains core interaction chrome rather than an extension.

Ordinary composer top/bottom rules use a stable per-Session accent selected from a small palette. Embedded views that replace the composer interior (Settings/keymap/help/provider-related views) use one reserved auxiliary orange that is not in the composer palette. This prevents an embedded view from visually masquerading as the session composer.

Status-extension rows are outside the two rules and keep their semantic per-span tones.

## Future process extension host

A future external host should preserve this layering:

1. aw starts/connects to a bounded extension-host process.
2. The proxy obtains a `Manifest` through `Request::Manifest`.
3. The normal `Registry` validates the manifest.
4. Projection uses `Request::Project { slot, context }`.
5. The process returns `Response::Project { contributions }`.
6. The same registry validates every contribution before the TUI sees it.

Do not expose raw terminal handles, SQLite handles, provider clients, tool executors or model-context mutation through this UI-extension protocol.

If future extensions need execution hooks, model hooks or tools, define separate capability-scoped protocols rather than silently widening the UI-extension host.

## Non-goals / not yet implemented

- external extension-host process lifecycle;
- installable third-party extension packages;
- stable Rust dylib ABI;
- arbitrary terminal drawing callbacks;
- JavaScript/TypeScript Pi-extension binary compatibility;
- implicit model/tool hooks.

The current boundary intentionally keeps those decisions open while making the compiled-in architecture compatible with a later host process.


## Phase 52 Markdown renderer boundary

The Phase 52 Markdown renderer is internal Agent transcript presentation, not a new extension execution hook or arbitrary widget host. It consumes settled assistant text and returns bounded Ratatui text spans only. External/installable Markdown widgets, HTML, JavaScript, OSC-8 links and extension-controlled terminal drawing remain outside the current compiled-in UI extension protocol.
