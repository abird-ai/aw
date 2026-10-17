import { createEffect, createMemo, createSignal, on, onCleanup, onMount, Show } from "solid-js";
import { Icon } from "./icons.jsx";

// The DOM ranges are local presentation state. They never become API targets.
const MATCH_LIMIT = 4096;
let highlightedOwner = null;

export function AgentViewPanel(props) {
  let content;
  let scroller;
  let search;
  let ranges = [];
  let savedSelection = null;
  let pendingJump = false;
  let generation = {};
  const [count, setCount] = createSignal(0);
  const [limited, setLimited] = createSignal(false);
  const snapshot = createMemo(() => props.snapshot);
  const title = () => props.snapshot?.title || "Read-only view";
  const query = () => props.find?.query || "";
  const selected = () => Math.min(props.find?.selected || 0, Math.max(0, count() - 1));

  function clearHighlights() {
    if (highlightedOwner !== props.id) return;
    globalThis.CSS?.highlights?.delete("aw-agent-find");
    globalThis.CSS?.highlights?.delete("aw-agent-find-current");
    highlightedOwner = null;
  }
  function highlight() {
    clearHighlights();
    if (!props.active || !ranges.length || !globalThis.CSS?.highlights || typeof Highlight === "undefined") return;
    globalThis.CSS.highlights.set("aw-agent-find", new Highlight(...ranges));
    globalThis.CSS.highlights.set("aw-agent-find-current", new Highlight(ranges[selected()]));
    highlightedOwner = props.id;
  }
  function clearDerived() {
    generation = {};
    clearHighlights();
    ranges = [];
    savedSelection = null;
    setCount(0);
    setLimited(false);
    const selection = window.getSelection();
    if (selection?.rangeCount && (content?.contains(selection.anchorNode) || content?.contains(selection.focusNode))) selection.removeAllRanges();
  }
  function buildMatches() {
    const ticket = generation = {};
    const value = query();
    queueMicrotask(() => {
      if (ticket !== generation || !content || !props.snapshot) return;
      clearHighlights();
      ranges = [];
      setLimited(false);
      if (value) {
        const nodes = [];
        let text = "";
        const walker = document.createTreeWalker(content, NodeFilter.SHOW_TEXT);
        while (walker.nextNode()) {
          const node = walker.currentNode;
          const parent = node.parentElement;
          const details = parent?.closest("details:not([open])");
          const hidden = parent?.closest('[hidden], [aria-hidden="true"]');
          // An inactive panel is hidden as a whole, but still owns its Find index.
          if ((hidden && content.contains(hidden)) || (details && content.contains(details) && !details.querySelector("summary")?.contains(node))) continue;
          nodes.push({ node, start: text.length, end: text.length + node.length });
          text += node.data;
        }
        const pattern = new RegExp(value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "giu");
        let match;
        let first = 0;
        while ((match = pattern.exec(text)) && ranges.length < MATCH_LIMIT) {
          while (first < nodes.length && nodes[first].end <= match.index) first += 1;
          let last = first;
          const end = match.index + match[0].length;
          while (last < nodes.length && nodes[last].end < end) last += 1;
          if (!nodes[first] || !nodes[last]) break;
          const range = document.createRange();
          range.setStart(nodes[first].node, match.index - nodes[first].start);
          range.setEnd(nodes[last].node, end - nodes[last].start);
          ranges.push(range);
        }
        setLimited(Boolean(match && ranges.length === MATCH_LIMIT));
      }
      setCount(ranges.length);
      highlight();
      if (pendingJump) {
        pendingJump = false;
        ranges[selected()]?.startContainer.parentElement?.scrollIntoView({ block: "center", inline: "nearest" });
      }
    });
  }
  function changeQuery(value) {
    pendingJump = true;
    props.onFind({ query: value.slice(0, 1024), selected: 0 });
  }
  function jump(delta) {
    if (!ranges.length) return;
    const next = (selected() + delta + ranges.length) % ranges.length;
    props.onFind({ query: query(), selected: next });
    ranges[next].startContainer.parentElement?.scrollIntoView({ block: "center", inline: "nearest" });
  }
  function rememberSelection() {
    const selection = window.getSelection();
    if (props.active && props.snapshot && selection?.rangeCount && content?.contains(selection.anchorNode) && content.contains(selection.focusNode)) {
      savedSelection = selection.getRangeAt(0).cloneRange();
    }
  }
  function key(event) {
    if (event.defaultPrevented || event.target?.closest?.('dialog, [role="dialog"]') || !props.active || (!event.ctrlKey && !event.metaKey) || event.altKey || event.key.toLowerCase() !== "f") return;
    event.preventDefault();
    search?.focus();
    search?.select();
  }

  createEffect(on(snapshot, () => clearDerived()));
  createEffect(() => { if (snapshot()) { void query(); buildMatches(); } });
  createEffect(() => { void props.active; void selected(); highlight(); });
  createEffect(on(() => props.active, active => {
    if (!active) {
      const selection = window.getSelection();
      if (selection?.rangeCount && (content?.contains(selection.anchorNode) || content?.contains(selection.focusNode))) {
        if (content.contains(selection.anchorNode) && content.contains(selection.focusNode)) savedSelection = selection.getRangeAt(0).cloneRange();
        selection.removeAllRanges();
      }
      return;
    }
    queueMicrotask(() => {
      if (!props.active || !props.snapshot || !scroller) return;
      scroller.scrollTop = props.viewport?.top || 0;
      scroller.scrollLeft = props.viewport?.left || 0;
      if (savedSelection && content?.contains(savedSelection.startContainer) && document.activeElement !== search) {
        const selection = window.getSelection();
        selection?.removeAllRanges();
        selection?.addRange(savedSelection);
      }
    });
  }));
  const copy = event => {
    const selection = window.getSelection();
    if (!selection?.rangeCount || !(content?.contains(selection.anchorNode) || content?.contains(selection.focusNode))) return;
    if (!props.active || !props.onCheckAccess()) {
      event.preventDefault();
      selection.removeAllRanges();
    }
  };
  const toggled = event => { if (content?.contains(event.target)) buildMatches(); };
  onMount(() => {
    document.addEventListener("copy", copy, true);
    document.addEventListener("toggle", toggled, true);
    document.addEventListener("selectionchange", rememberSelection);
    document.addEventListener("keydown", key);
    buildMatches();
  });
  onCleanup(() => {
    clearDerived();
    document.removeEventListener("copy", copy, true);
    document.removeEventListener("toggle", toggled, true);
    document.removeEventListener("selectionchange", rememberSelection);
    document.removeEventListener("keydown", key);
  });

  return (
    <section class="pinned-agent-panel" data-agent-view-id={props.id} hidden={!props.active} aria-label={`Read-only Agent view ${title()}`}>
      <div class="surface-bar pinned-agent-bar">
        <span class="surface-path">{title()}{props.snapshot ? ` · ${props.snapshot.branch_name}` : ""}</span>
        <span class="surface-badge">read only</span>
        <button class="mini-icon" type="button" onClick={props.onRefresh} disabled={props.pending} aria-label={`Refresh ${title()}`} title="Refresh read-only view">
          <Icon name="refresh" size={13} />
        </button>
      </div>
      <div class="agent-view-find" role="search" aria-label={`Find in ${title()}`}>
        <input ref={search} type="search" value={query()} onInput={event => changeQuery(event.currentTarget.value)} aria-label={`Find in ${title()}`} placeholder="Find in this view" maxLength={1024}
          onKeyDown={event => { if (event.key === "Enter") { event.preventDefault(); jump(event.shiftKey ? -1 : 1); } }} />
        <span role="status" aria-live="polite">{query() ? count() ? `${selected() + 1} of ${count()}${limited() ? "+" : ""}` : "No matches" : ""}</span>
        <button class="mini-icon" type="button" onClick={() => jump(-1)} disabled={!count()} aria-label={`Previous match in ${title()}`}><Icon name="chevronDown" class="find-previous" size={13} /></button>
        <button class="mini-icon" type="button" onClick={() => jump(1)} disabled={!count()} aria-label={`Next match in ${title()}`}><Icon name="chevronDown" size={13} /></button>
      </div>
      <div class="agent-view-state" role="status">
        {props.error || (props.pending ? "Checking read access…" : props.snapshot?.needs_inspection ? "History needs inspection. Viewing takes no recovery action." : "Pinned branch · Agent unchanged")}
      </div>
      <div class="chat-scroll pinned-agent-scroll" ref={scroller} onScroll={() => { if (props.active && props.snapshot) props.onViewport({ top: scroller.scrollTop, left: scroller.scrollLeft, follow: false }); }}>
        <Show when={props.snapshot} fallback={<div class="navigator-empty">{props.error || "Loading authorized read-only view…"}</div>}>
          <div class="chat pinned-agent-transcript" ref={content}>{props.children}</div>
        </Show>
      </div>
    </section>
  );
}
