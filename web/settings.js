const $ = id => document.getElementById(id);
const STATUS_ITEMS = ['model','directory','branch','profile','tools','session','state'];

function lines(value) {
  return value.split(/\r?\n/).map(v => v.trim()).filter(Boolean);
}

export function createSettings({request, changed}) {
  let state = null;
  let statusOrder = [...STATUS_ITEMS];

  function fail(text='') {
    const box = $('settings-error');
    box.textContent = text;
    box.hidden = !text;
  }

  function selectedProfile() {
    const name = $('settings-profile-choice').value;
    return state?.profiles?.find(profile => profile.name === name) || null;
  }

  function fill(profile) {
    const value = profile?.profile || {};
    $('settings-profile-name').value = profile?.name || '';
    $('settings-provider-kind').value = value.kind || 'ollama';
    $('settings-base-url').value = value.base_url || 'http://127.0.0.1:11434';
    $('settings-default-model').value = value.model || '';
    $('settings-models').value = (value.models || []).join('\n');
    $('settings-api-key-env').value = value.api_key_env || '';
    $('settings-insecure').checked = Boolean(value.allow_insecure_http);
    $('settings-default').checked = Boolean(profile?.is_default);
    $('settings-remove-profile').disabled = !profile || profile.builtin;
    $('settings-make-default').disabled = !profile;
  }

  function moveStatus(name, direction) {
    const index = statusOrder.indexOf(name);
    const next = index + direction;
    if (index < 0 || next < 0 || next >= statusOrder.length) return;
    [statusOrder[index], statusOrder[next]] = [statusOrder[next], statusOrder[index]];
    renderStatus();
  }

  function statusLines() {
    const canonical = state?.ui?.status_lines;
    if (Array.isArray(canonical)) return canonical.map(line => ({
      region: line.region,
      items: [...(line.items || [])],
    }));
    const legacy = state?.ui?.status_line || [];
    return legacy.length ? [{region:'below-composer',items:[...legacy]}] : [];
  }

  function selectedStatusItems() {
    return new Set(statusLines().flatMap(line => line.items || []));
  }

  function renderStatus() {
    const selected = selectedStatusItems();
    const host = $('settings-status-items');
    host.replaceChildren();
    for (const name of statusOrder) {
      const row = document.createElement('div');
      row.className = 'status-item-row';
      const label = document.createElement('label');
      const input = document.createElement('input');
      input.type = 'checkbox';
      input.value = name;
      input.checked = selected.has(name);
      label.append(input, document.createTextNode(name.replace('-', ' ')));
      const up = document.createElement('button');
      up.type='button'; up.className='quiet'; up.textContent='↑';
      up.onclick=()=>moveStatus(name,-1);
      const down = document.createElement('button');
      down.type='button'; down.className='quiet'; down.textContent='↓';
      down.onclick=()=>moveStatus(name,1);
      row.append(label,up,down);
      host.append(row);
    }
  }

  function render() {
    if (!state) return;
    const choice = $('settings-profile-choice');
    const previous = choice.value;
    choice.replaceChildren();
    for (const profile of state.profiles || []) {
      const option = document.createElement('option');
      option.value = profile.name;
      option.textContent = `${profile.name}${profile.is_default ? ' · default' : ''}${profile.builtin ? ' · built-in' : ''}`;
      choice.append(option);
    }
    const wanted = (state.profiles || []).some(p => p.name === previous)
      ? previous
      : state.default_profile || state.profiles?.[0]?.name || '';
    choice.value = wanted;
    $('settings-config-path').textContent = state.config_path
      ? `Config: ${state.config_path}`
      : 'No writable config path is available.';
    const ui = state.ui || {};
    $('settings-tui-mode').value = ui.tui_mode || 'inline';
    $('settings-editor-padding').value = ui.editor_padding_x ?? 0;
    $('settings-output-padding').value = ui.output_padding ?? 0;
    $('settings-autocomplete-max').value = ui.autocomplete_max_visible ?? 5;
    $('settings-performance').checked = ui.show_performance ?? true;
    $('settings-ttft').checked = ui.show_ttft ?? false;
    $('settings-waterfall').checked = ui.show_waterfall ?? false;
    $('settings-thinking').checked = ui.show_thinking ?? true;
    $('settings-tools').checked = ui.show_tool_calls ?? true;
    const stored = [...selectedStatusItems()];
    statusOrder = [...stored, ...STATUS_ITEMS.filter(item => !stored.includes(item))];
    fill(selectedProfile());
    renderStatus();
  }

  async function load(show = false) {
    state = await request('/api/settings');
    render();
    if (show && !$('settings-dialog').open) $('settings-dialog').showModal();
    return state;
  }

  $('settings-profile-choice').addEventListener('change',()=>fill(selectedProfile()));
  $('settings-new-profile').addEventListener('click',()=>{
    $('settings-profile-choice').value='';
    fill(null);
    $('settings-profile-name').focus();
  });
  $('settings-profile-form').addEventListener('submit',async event=>{
    event.preventDefault();
    fail();
    const body = {
      name: $('settings-profile-name').value.trim(),
      kind: $('settings-provider-kind').value,
      base_url: $('settings-base-url').value.trim(),
      default_model: $('settings-default-model').value.trim() || null,
      models: lines($('settings-models').value),
      api_key_env: $('settings-api-key-env').value.trim() || null,
      allow_insecure_http: $('settings-insecure').checked,
      make_default: $('settings-default').checked,
    };
    try {
      state = await request('/api/settings/profile', body);
      render();
      $('settings-profile-choice').value = body.name;
      fill(selectedProfile());
      await changed?.();
    } catch (error) { fail(error.message); }
  });
  $('settings-remove-profile').addEventListener('click',async()=>{
    const profile = selectedProfile();
    if (!profile || profile.builtin) return;
    if (!confirm(`Remove the configured override for ${profile.name}? This does not delete any environment secret.`)) return;
    try {
      state = await request('/api/settings/profile/remove',{name:profile.name});
      render();
      await changed?.();
    } catch(error){ fail(error.message); }
  });
  $('settings-make-default').addEventListener('click',async()=>{
    const profile = selectedProfile();
    if (!profile) return;
    try {
      state = await request('/api/settings/default',{name:profile.name});
      render();
      await changed?.();
    } catch(error){ fail(error.message); }
  });
  $('settings-save-ui').addEventListener('click',async()=>{
    const enabled = [...$('settings-status-items').querySelectorAll('input[type=checkbox]:checked')].map(input=>input.value);
    const ordered = statusOrder.filter(item=>enabled.includes(item));
    let statusLinesNext = statusLines()
      .map(line => ({
        region: line.region,
        items: line.items.filter(item => enabled.includes(item)).sort((a,b)=>statusOrder.indexOf(a)-statusOrder.indexOf(b)),
      }))
      .filter(line => line.items.length);
    const present = new Set(statusLinesNext.flatMap(line => line.items));
    const added = ordered.filter(item => !present.has(item));
    if (added.length) {
      let below = statusLinesNext.find(line => line.region === 'below-composer');
      if (!below) {
        below = {region:'below-composer',items:[]};
        statusLinesNext.push(below);
      }
      let fullscreen = statusLinesNext.find(line => line.region === 'fullscreen-status');
      if (!fullscreen) {
        fullscreen = {region:'fullscreen-status',items:[]};
        statusLinesNext.push(fullscreen);
      }
      below.items.push(...added);
      fullscreen.items.push(...added);
    }
    const body = {
      status_lines: statusLinesNext,
      tui_mode: $('settings-tui-mode').value,
      editor_padding_x: Number($('settings-editor-padding').value),
      output_padding: Number($('settings-output-padding').value),
      autocomplete_max_visible: Number($('settings-autocomplete-max').value),
      show_performance: $('settings-performance').checked,
      show_ttft: $('settings-ttft').checked,
      show_waterfall: $('settings-waterfall').checked,
      show_thinking: $('settings-thinking').checked,
      show_tool_calls: $('settings-tools').checked,
    };
    try {
      state = await request('/api/settings/ui',body);
      render();
    } catch(error){ fail(error.message); }
  });
  $('settings-close').addEventListener('click',()=>$('settings-dialog').close());

  return {
    load,
    open:()=>load(true),
    getState:()=>state,
  };
}
