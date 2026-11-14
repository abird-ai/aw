import { createEffect, createMemo, createSignal, For, on, onCleanup, onMount, Show } from "solid-js";

function valueOf(value) {
  return typeof value === "function" ? value() : value;
}

function terminalRunStyle(style = {}) {
  const safe = value => /^#[0-9a-f]{6}$/iu.test(String(value || "")) ? value : undefined;
  return {
    color: safe(style.fg),
    "background-color": safe(style.bg),
    "font-weight": style.bold ? "700" : undefined,
    opacity: style.dim ? ".68" : undefined,
    "font-style": style.italic ? "italic" : undefined,
    "text-decoration": style.underline ? "underline" : undefined,
  };
}

function shortTerminalLabel(terminal) {
  const root = String(terminal?.root || "").replace(/\/+$/u, "");
  const name = root.split("/").filter(Boolean).at(-1);
  const suffix = String(terminal?.id || "").slice(-6);
  return name ? `${name} · ${suffix}` : suffix || "terminal";
}

export function TerminalTile(props) {
  const terminal = createMemo(() => valueOf(props.terminal) || null);
  const terminalId = createMemo(() => terminal()?.id || "");
  const active = () => Boolean(valueOf(props.active));
  const [screen, setScreen] = createSignal(null);
  const [lease, setLease] = createSignal(null);
  const [nextSequence, setNextSequence] = createSignal(1);
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [stopArmed, setStopArmed] = createSignal(false);
  const clientId = "webterm_" + crypto.randomUUID().replaceAll("-", "");
  let polling;
  let inputQueue = Promise.resolve();

  const controlled = () => Boolean(lease());
  const maximized = () => Boolean(valueOf(props.maximized));

  async function request(body) {
    return props.request(body);
  }

  async function releaseExact(id, token) {
    if (!id || !token) return;
    try {
      await request({ op: "terminal_release", terminal_id: id, client_id: clientId, lease: token });
    } catch {}
  }

  async function release() {
    const id = terminalId(), token = lease();
    setLease(null);
    await releaseExact(id, token);
  }

  async function refreshScreen() {
    const id = terminalId();
    if (!id || (!active() && !controlled())) return;
    const control = lease();
    try {
      const value = await request({
        op: "terminal_screen",
        terminal_id: id,
        scrollback: 0,
        ...(control ? { client_id: clientId, lease: control } : {}),
      });
      if (id === terminalId()) {
        setScreen(value);
        setError("");
      }
    } catch (cause) {
      if (id !== terminalId()) return;
      if (control) {
        setLease(null);
        setError("Terminal control expired; the PTY remains available read-only.");
        try {
          const value = await request({ op: "terminal_screen", terminal_id: id, scrollback: 0 });
          if (id === terminalId()) setScreen(value);
        } catch {}
      } else {
        setError(cause.message || "Terminal screen unavailable.");
      }
    }
  }

  async function claim() {
    const id = terminalId();
    if (!id || busy()) return;
    setBusy(true);
    try {
      const result = await request({ op: "terminal_claim", terminal_id: id, client_id: clientId });
      if (id !== terminalId()) {
        await releaseExact(id, result.lease);
        return;
      }
      setLease(result.lease);
      setNextSequence(result.next_sequence);
      setError("");
      await refreshScreen();
    } catch (cause) {
      setError(cause.message || "Terminal input is owned elsewhere.");
    } finally {
      setBusy(false);
    }
  }

  function queueInput(input) {
    const id = terminalId(), token = lease();
    if (!id || !token) return;
    inputQueue = inputQueue.then(async () => {
      const sequence = nextSequence();
      const result = await request({
        op: "terminal_input",
        terminal_id: id,
        client_id: clientId,
        lease: token,
        sequence,
        input,
      });
      if (id === terminalId() && lease() === token && result.accepted_sequence === sequence) {
        setNextSequence(sequence + 1);
        await refreshScreen();
      }
    }).catch(cause => {
      if (id === terminalId()) {
        setLease(null);
        setError(cause?.message || "Terminal input became uncertain; claim control again before typing.");
      }
    });
  }

  function onKeyDown(event) {
    if (!controlled() || event.isComposing) return;
    let input;
    if (event.key.length === 1 && !event.ctrlKey && !event.altKey) {
      input = { kind: "text", text: event.key };
    } else if (["Enter","Tab","Backspace","Escape","ArrowUp","ArrowDown","ArrowLeft","ArrowRight",
      "Home","End","Delete","Insert","PageUp","PageDown"].includes(event.key)
      || /^F\d{1,2}$/u.test(event.key)
      || (event.key.length === 1 && (event.ctrlKey || event.altKey))) {
      input = { kind: "key", key: event.key, ctrl: event.ctrlKey, alt: event.altKey, shift: event.shiftKey };
    }
    if (!input) return;
    event.preventDefault();
    event.stopPropagation();
    queueInput(input);
  }

  function onPaste(event) {
    if (!controlled()) return;
    const text = event.clipboardData?.getData("text") || "";
    if (!text) return;
    event.preventDefault();
    event.stopPropagation();
    queueInput({ kind: "paste", text });
  }

  async function stopTerminal() {
    const id = terminalId();
    if (!id || busy()) return;
    if (!stopArmed()) {
      setStopArmed(true);
      setTimeout(() => setStopArmed(false), 3000);
      return;
    }
    setStopArmed(false);
    setBusy(true);
    try {
      await release();
      await request({ op: "terminal_stop", terminal_id: id, confirmed: true });
      props.onStopped?.(id);
    } catch (cause) {
      setError(cause.message || "Terminal stop failed.");
    } finally {
      setBusy(false);
    }
  }

  createEffect(on(terminalId, (id, previous) => {
    if (previous && previous !== id) {
      const token = lease();
      setLease(null);
      void releaseExact(previous, token);
    }
    setScreen(null);
    setError("");
    setStopArmed(false);
    if (id && active()) queueMicrotask(() => void refreshScreen());
  }));

  createEffect(on(controlled, value => props.onControlChange?.(value)));

  createEffect(() => {
    if ((active() || controlled()) && terminalId()) queueMicrotask(() => void refreshScreen());
  });

  onMount(() => {
    polling = setInterval(() => {
      if ((active() || controlled()) && terminalId()) void refreshScreen();
    }, 750);
  });
  onCleanup(() => {
    clearInterval(polling);
    void release();
  });

  return (
    <div class={"terminal-tile " + (controlled() ? "controlled" : "viewer")}>
      <Show when={props.pane}>
        <div class="pane-bar terminal-pane-bar">
          <span><span class="terminal-status-dot" />{props.title?.() || shortTerminalLabel(terminal())}</span>
          <span class="terminal-pane-meta">{terminal()?.site || "local"} · {controlled() ? "owner" : "viewer"}</span>
          <button class="pane-icon" type="button" onClick={() => props.toggleMax?.()}
            aria-label={maximized() ? "Restore terminal pane" : "Maximize terminal pane"}
            title={maximized() ? "Restore" : "Maximize"}>
            <span aria-hidden="true">{maximized() ? "❐" : "⛶"}</span>
          </button>
        </div>
      </Show>
      <div class="terminal-toolbar">
        <span class="terminal-runtime-status">{terminal()?.status || "terminal"}</span>
        <Show when={controlled()} fallback={
          <button class="quiet" type="button" disabled={busy()} onClick={() => void claim()}>Take input</button>
        }>
          <button class="quiet" type="button" onClick={() => void release()}>Release input</button>
        </Show>
        <button class={"quiet terminal-stop " + (stopArmed() ? "armed" : "")} type="button"
          disabled={!terminalId() || busy()} onClick={() => void stopTerminal()}>
          {stopArmed() ? "Confirm stop" : "Stop"}
        </button>
        <Show when={!props.pane}>
          <span class="terminal-owner-state">{controlled() ? "input owner · browser" : "viewer"}</span>
        </Show>
      </div>
      <Show when={error()}><div class="terminal-notice">{error()}</div></Show>
      <div class={"terminal-screen " + (controlled() ? "controlled" : "viewer")}
        tabIndex={terminalId() ? 0 : -1}
        aria-label={controlled() ? "Terminal input" : "Read-only terminal screen"}
        onKeyDown={onKeyDown} onPaste={onPaste}>
        <Show when={screen()} fallback={<div class="terminal-placeholder">No terminal screen yet.</div>}>
          <For each={screen()?.lines || []}>{runs =>
            <div class="terminal-line"><For each={runs}>{run =>
              <span style={terminalRunStyle(run.style)}>{run.text}</span>
            }</For></div>
          }</For>
        </Show>
      </div>
    </div>
  );
}

function useTerminalInventory(props) {
  const [terminals, setTerminals] = createSignal([]);
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const active = () => Boolean(valueOf(props.active));
  const enabled = () => Boolean(valueOf(props.enabled));
  let polling;

  async function request(body) {
    return props.request(body);
  }

  async function load() {
    if (!enabled()) return [];
    try {
      const result = await request({ op: "terminals" });
      const list = Array.isArray(result) ? result : [];
      setTerminals(list);
      setError("");
      return list;
    } catch (cause) {
      setError(cause.message || "Terminal list unavailable.");
      return terminals();
    }
  }

  async function start() {
    if (!enabled() || busy()) return null;
    setBusy(true);
    setError("");
    try {
      const result = await request({
        op: "terminal_start",
        request_id: "webterm_" + crypto.randomUUID().replaceAll("-", ""),
        context: "root",
        confirmed: true,
        rows: 18,
        cols: 100,
      });
      await load();
      return result.terminal_id || null;
    } catch (cause) {
      setError(cause.message || "Terminal launch failed.");
      return null;
    } finally {
      setBusy(false);
    }
  }

  createEffect(() => {
    if (!enabled()) {
      setTerminals([]);
      setError("");
      return;
    }
    if (active()) queueMicrotask(() => void load());
  });

  onMount(() => {
    polling = setInterval(() => {
      if (enabled() && active()) void load();
    }, 1000);
  });
  onCleanup(() => clearInterval(polling));

  return { terminals, error, busy, load, start, active, enabled };
}

export function TerminalPane(props) {
  const inventory = useTerminalInventory(props);
  const [terminalId, setTerminalId] = createSignal("");
  const [controlled, setControlled] = createSignal(false);
  const selected = createMemo(() => inventory.terminals().find(item => item.id === terminalId()) || null);

  createEffect(() => {
    const list = inventory.terminals();
    const current = terminalId();
    if (!current || !list.some(item => item.id === current)) setTerminalId(list[0]?.id || "");
  });

  async function start() {
    const id = await inventory.start();
    if (id) setTerminalId(id);
  }

  return (
    <div class="review-terminal-content">
      <Show when={inventory.enabled()} fallback={
        <div class="terminal-unavailable">
          <strong>PTY unavailable in this browser</strong>
          <span>Start the local owner with <code>--web-allow-terminals</code> to expose real browser terminal custody. No fake shell is rendered.</span>
        </div>
      }>
        <div class="terminal-selector-row">
          <Show when={inventory.terminals().length} fallback={
            <button class="quiet terminal-start" type="button" disabled={inventory.busy()} onClick={() => void start()}>
              {inventory.busy() ? "Starting…" : "Start terminal"}
            </button>
          }>
            <select aria-label="Terminal" value={terminalId()} disabled={controlled()}
              title={controlled() ? "Release input before switching terminals" : "Choose retained terminal"}
              onChange={event => setTerminalId(event.currentTarget.value)}>
              <For each={inventory.terminals()}>{item =>
                <option value={item.id}>{item.status} · {shortTerminalLabel(item)}</option>
              }</For>
            </select>
          </Show>
        </div>
        <Show when={inventory.error()}><div class="terminal-notice">{inventory.error()}</div></Show>
        <Show when={selected()} fallback={<div class="terminal-placeholder">No retained terminal.</div>}>
          <TerminalTile terminal={selected} active={inventory.active} request={props.request}
            onControlChange={setControlled}
            onStopped={() => { setControlled(false); void inventory.load(); }} />
        </Show>
      </Show>
    </div>
  );
}

export function BuildLayout(props) {
  const inventory = useTerminalInventory(props);
  const [maximizedId, setMaximizedId] = createSignal(null);
  const active = () => inventory.active();

  createEffect(() => {
    const live = new Set(inventory.terminals().map(item => item.id));
    if (maximizedId() && !live.has(maximizedId())) setMaximizedId(null);
  });

  async function start() {
    await inventory.start();
  }

  return (
    <div class="build-layout" hidden={!active()}>
      <Show when={inventory.enabled()} fallback={
        <div class="terminal-unavailable">
          <strong>PTY unavailable in this browser</strong>
          <span>Build is a view over retained real terminals. Start the owner with <code>--web-allow-terminals</code>; no fake terminal is rendered.</span>
        </div>
      }>
        <Show when={inventory.error()}><div class="terminal-notice">{inventory.error()}</div></Show>
        <div class="build-grid" classList={{
          "one-terminal": inventory.terminals().length === 1,
          "two-terminals": inventory.terminals().length === 2,
          "max-terminal": Boolean(maximizedId()),
        }}>
          <For each={inventory.terminals().map(item => item.id)}>
            {id => {
              const terminal = createMemo(() => inventory.terminals().find(item => item.id === id) || null);
              const maximized = () => maximizedId() === id;
              return (
                <section class="build-terminal-pane" hidden={Boolean(maximizedId()) && !maximized()}>
                  <TerminalTile pane terminal={terminal}
                    title={() => shortTerminalLabel(terminal())}
                    active={() => active() && (!maximizedId() || maximized())}
                    request={props.request}
                    maximized={maximized}
                    toggleMax={() => setMaximizedId(maximized() ? null : id)}
                    onStopped={() => void inventory.load()} />
                </section>
              );
            }}
          </For>
          <Show when={!inventory.terminals().length}>
            <div class="build-empty">
              <strong>No retained terminals</strong>
              <span>Build never starts a PTY implicitly. Start one explicitly to add it to this saved layout.</span>
              <button class="quiet build-start-terminal" type="button" disabled={inventory.busy()} onClick={() => void start()}>
                {inventory.busy() ? "Starting…" : "Start terminal"}
              </button>
            </div>
          </Show>
        </div>
        <Show when={inventory.terminals().length > 0 && inventory.terminals().length < 4 && !maximizedId()}>
          <button class="build-add-terminal" type="button" disabled={inventory.busy()} onClick={() => void start()}>
            <span aria-hidden="true">＋</span> Terminal
          </button>
        </Show>
      </Show>
    </div>
  );
}
