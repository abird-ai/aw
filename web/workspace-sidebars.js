import {changeRows} from '/workspace-model.js';

const el = (tag, cls, text) => {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
};
const button = (label, fn, cls = 'side-action') => {
  const node = el('button', cls, label);
  node.type = 'button';
  node.addEventListener('click', fn);
  return node;
};

function verticalStack(host, panels) {
  host.classList.add('side-stack');
  const records = [];
  for (const [index, panel] of panels.entries()) {
    const section = el('section', 'side-panel');
    const header = el('div', 'side-panel-header');
    const title = el('strong', '', panel.title);
    const collapse = button('−', () => {
      section.classList.toggle('collapsed');
      collapse.textContent = section.classList.contains('collapsed') ? '+' : '−';
    }, 'side-collapse');
    header.append(title, collapse);
    const body = el('div', 'side-panel-body');
    section.append(header, body);
    host.append(section);
    records.push({section, body, panel});

    if (index < panels.length - 1) {
      const handle = el('div', 'side-row-resizer');
      handle.setAttribute('role', 'separator');
      handle.setAttribute('aria-orientation', 'horizontal');
      handle.setAttribute('aria-label', 'Resize side panes');
      handle.tabIndex = 0;
      handle.addEventListener('pointerdown', event => {
        event.preventDefault();
        const after = records[index + 1]?.section || handle.nextElementSibling;
        if (!after) return;
        const startY = event.clientY;
        const beforeRect = section.getBoundingClientRect();
        const afterRect = after.getBoundingClientRect();
        const total = beforeRect.height + afterRect.height;
        const move = moveEvent => {
          const delta = moveEvent.clientY - startY;
          const first = Math.max(72, Math.min(total - 72, beforeRect.height + delta));
          section.style.flex = `0 0 ${first}px`;
          after.style.flex = `0 0 ${total - first}px`;
        };
        const done = () => {
          document.removeEventListener('pointermove', move);
          document.removeEventListener('pointerup', done);
        };
        document.addEventListener('pointermove', move);
        document.addEventListener('pointerup', done);
      });
      host.append(handle);
    }
  }
  for (const record of records) record.section.style.flex = '1 1 0';
  return records;
}

export function createWorkspaceSidebars({
  left,
  right,
  request,
  command,
  open,
  openSession,
  getSnapshot,
  notify,
  rootLabel,
  onGitContext,
}) {
  let alive = true;
  let fileRoot = '';
  let fileTicket = 0;
  let gitTicket = 0;
  let sessionsTicket = 0;
  let gitContext = 'root';
  let gitMode = 'changes';
  let gitDisplay = 'tree';
  const expanded = new Map();

  const leftPanels = verticalStack(left, [
    {key: 'files', title: 'Explorer'},
    {key: 'git', title: 'Source Control'},
    {key: 'sessions', title: 'Sessions'},
  ]);
  const rightPanels = verticalStack(right, [
    {key: 'session', title: 'Session'},
    {key: 'activity', title: 'Activity'},
    {key: 'context', title: 'Context'},
  ]);
  const leftBy = Object.fromEntries(leftPanels.map(record => [record.panel.key, record.body]));
  const rightBy = Object.fromEntries(rightPanels.map(record => [record.panel.key, record.body]));

  const safe = promise => Promise.resolve(promise).catch(error => {
    if (alive) notify(error.message);
  });

  function pathEntries(path) {
    return expanded.get(path) || [];
  }

  async function loadDirectory(path) {
    const ticket = ++fileTicket;
    const data = await command({op: 'files', path});
    if (!alive || ticket !== fileTicket) return;
    expanded.set(path, data.entries || []);
    renderFiles();
  }

  function renderFileRows(host, path, depth) {
    for (const item of pathEntries(path)) {
      const row = button('', () => {}, 'side-tree-row');
      row.style.paddingLeft = `${8 + depth * 14}px`;
      const key = item.path || '';
      const opened = item.kind === 'directory' && expanded.has(key);
      row.textContent = (item.kind === 'directory' ? (opened ? '▾ ' : '▸ ') : '  ') + item.name;
      row.disabled = item.kind === 'unsupported';
      row.onclick = () => {
        if (item.kind === 'directory') {
          if (opened) {
            expanded.delete(key);
            renderFiles();
          } else {
            safe(loadDirectory(key));
          }
        } else if (item.kind === 'file') {
          open({kind: 'file', title: item.name, resource: item.path});
        }
      };
      host.append(row);
      if (opened) renderFileRows(host, key, depth + 1);
    }
  }

  function renderFiles() {
    const host = leftBy.files;
    host.replaceChildren();
    const toolbar = el('div', 'side-mini-toolbar');
    toolbar.append(
      el('span', 'side-scope', fileRoot || rootLabel || '.'),
      button('↻', () => safe(loadDirectory(fileRoot)), 'side-icon-button'),
    );
    host.append(toolbar);
    const tree = el('div', 'side-tree');
    renderFileRows(tree, fileRoot, 0);
    if (!tree.childElementCount) tree.append(el('p', 'side-empty', 'No files in this bounded view.'));
    host.append(tree);
  }

  async function loadGit() {
    const ticket = ++gitTicket;
    const data = await command({op: 'git_list', context: gitContext, mode: gitMode});
    if (!alive || ticket !== gitTicket) return;
    renderGit(data);
  }

  function renderGit(data) {
    const host = leftBy.git;
    host.replaceChildren();
    const toolbar = el('div', 'side-mini-toolbar');
    for (const mode of ['changes', 'branches', 'worktrees']) {
      const item = button(mode[0].toUpperCase() + mode.slice(1), () => {
        gitMode = mode;
        safe(loadGit());
      }, 'side-chip');
      item.classList.toggle('selected', gitMode === mode);
      toolbar.append(item);
    }
    toolbar.append(button('↻', () => safe(loadGit()), 'side-icon-button'));
    host.append(toolbar);

    if (gitMode === 'changes') {
      const display = el('div', 'side-sub-toolbar');
      for (const mode of ['tree', 'list']) {
        const item = button(mode, () => {
          gitDisplay = mode;
          renderGit(data);
        }, 'side-chip');
        item.classList.toggle('selected', gitDisplay === mode);
        display.append(item);
      }
      host.append(display);
    }

    const list = el('div', 'side-list');
    const rows = gitMode === 'changes'
      ? changeRows(data.items || [], gitDisplay === 'tree', '', new Set())
      : (data.items || []).map(item => ({...item, kind: 'item'}));
    for (const row of rows) {
      if (row.kind === 'heading') {
        list.append(el('div', 'side-heading', row.label));
        continue;
      }
      if (row.kind === 'folder') {
        const folder = el('div', 'side-tree-label', `${'  '.repeat(row.depth || 0)}▾ ${row.label}`);
        list.append(folder);
        continue;
      }
      const item = button(
        `${row.status ? row.status + ' ' : ''}${'  '.repeat(row.depth || 0)}${row.label || row.path || row.id}`,
        async () => {
          try {
            const result = await command({op: 'git_open', item_id: row.id});
            if (result.kind === 'context') {
              gitContext = result.context.id;
              onGitContext?.(gitContext);
              notify('Git view scope changed; agent root and model context did not move.');
              await loadGit();
            } else {
              open({kind: 'diff', title: result.path || 'Comparison', resource: result.id});
            }
          } catch (error) {
            notify(error.message);
          }
        },
        'side-list-row',
      );
      item.title = row.path || row.label || '';
      list.append(item);
    }
    if (!list.childElementCount) list.append(el('p', 'side-empty', 'No entries.'));
    host.append(list);
  }

  async function loadSessions() {
    const ticket = ++sessionsTicket;
    const data = await request('/api/sessions');
    if (!alive || ticket !== sessionsTicket) return;
    const host = leftBy.sessions;
    host.replaceChildren();
    const toolbar = el('div', 'side-mini-toolbar');
    toolbar.append(el('span', 'side-scope', 'Conversation history'), button('↻', () => safe(loadSessions()), 'side-icon-button'));
    host.append(toolbar);
    const list = el('div', 'side-list');
    for (const item of data.sessions || []) {
      const row = button(item.title || item.id, () => {
        openSession(item.id);
        open({kind: 'conversation', title: 'Conversation'});
      }, 'side-list-row');
      row.title = `${item.id} · ${item.directory || ''}`;
      list.append(row);
    }
    if (!list.childElementCount) list.append(el('p', 'side-empty', 'No saved sessions yet.'));
    host.append(list);
  }

  function renderRight() {
    const snapshot = getSnapshot();
    const session = rightBy.session;
    session.replaceChildren();
    if (!snapshot) {
      session.append(el('p', 'side-empty', 'No session loaded.'));
      return;
    }
    const items = [
      ['Profile', snapshot.profile || 'not configured'],
      ['Model', snapshot.model || 'not configured'],
      ['Branch', snapshot.saved?.branch_name || 'memory'],
      ['Session', snapshot.saved?.title || 'not yet created'],
      ['State', snapshot.pending ? 'running' : snapshot.needs_reconciliation ? 'inspect' : 'ready'],
    ];
    for (const [label, value] of items) {
      const row = el('div', 'inspector-row');
      row.append(el('span', '', label), el('strong', '', value));
      session.append(row);
    }

    const activity = rightBy.activity;
    activity.replaceChildren();
    const log = [];
    for (const turn of (snapshot.turns || []).slice(-10)) {
      log.push(`Turn ${turn.id} · ${turn.outcome}`);
      for (const record of turn.tool_records || []) {
        log.push(`  ↳ ${record.call?.name || 'tool'} · ${record.status}`);
      }
    }
    if (snapshot.pending) log.push(`Run ${snapshot.pending.run_id} · streaming`);
    activity.append(el('pre', 'side-log', log.join('\n') || 'No activity yet.'));

    const context = rightBy.context;
    context.replaceChildren();
    if (snapshot.needs_reconciliation) context.append(el('p', 'side-warning', 'Inspection required before more model work.'));
    for (const block of snapshot.context_blocks || []) {
      const details = el('details', 'context-side-item');
      details.append(
        el('summary', '', `${block.kind} · ${(block.sources || []).length} sources`),
        el('pre', '', block.text || ''),
      );
      context.append(details);
    }
    if (!context.childElementCount) context.append(el('p', 'side-empty', 'No imported or compacted context.'));
  }

  function refresh() {
    if (!alive) return;
    renderRight();
    safe(loadSessions());
  }

  safe(loadDirectory(fileRoot));
  safe(loadGit());
  safe(loadSessions());
  renderRight();

  const panelsByKey = {
    files: leftPanels.find(record => record.panel.key === 'files'),
    git: leftPanels.find(record => record.panel.key === 'git'),
    sessions: leftPanels.find(record => record.panel.key === 'sessions'),
    session: rightPanels.find(record => record.panel.key === 'session'),
    activity: rightPanels.find(record => record.panel.key === 'activity'),
    context: rightPanels.find(record => record.panel.key === 'context'),
  };

  return {
    refresh,
    gitContext: () => gitContext,
    focus(key) {
      const record = panelsByKey[key];
      if (!record) return;
      record.section.classList.remove('collapsed');
      record.section.scrollIntoView({block:'nearest'});
      record.section.querySelector('button, [tabindex]')?.focus?.();
    },
    dispose() {
      alive = false;
      left.replaceChildren();
      right.replaceChildren();
    },
  };
}
