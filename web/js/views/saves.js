// Save file editor: each line of filech<N>_<slot> with its meaning (from the game's save script),
// story point fields (plot, room), presets, backups, and sending changed flags to the running game.
import { api, chapterApi, live } from '../api.js';
import { h, attempt, clear, confirmDialog, promptDialog, toast, add } from '../ui.js';
import { setDirtyCheck } from '../app.js';
import { setCommand } from './flags.js';

const enc = encodeURIComponent;

/** `global.flag[12]` -> {name: 'flag', index: '12'}; `global.plot` -> {name: 'plot', index: null}. */
export function parseLabel(label) {
  const m = /^global\.([A-Za-z_]\w*)(?:\[(\d+)\])?$/.exec(label || '');
  return m ? { name: m[1], index: m[2] ?? null } : null;
}

export async function render(main, ctx) {
  const page = h('div', { class: 'page' });
  main.append(page);
  page.append(h('h2', null, 'Save files'));
  let info;
  try {
    info = await api.get('/api/saves');
  } catch (e) {
    page.append(h('p', null, `${e.message}. Set the save folder in `, h('a', { href: '#/' }, 'Setup'), '.'));
    return;
  }
  let file = ctx.arg || null;
  let lines = [];
  let original = [];
  let layout = null;
  const listBox = h('div', { class: 'card' });
  const editor = h('div');
  add(page,
    h('p', { class: 'dim' }, 'Each line of a Deltarune save, with the variable that the game writes there. Change plot, room, and flags to start at any story point. Every write makes a backup first.'),
    h('p', { class: 'dim mono small' }, info.dir),
    listBox, editor);
  setDirtyCheck(() => lines.some((l, i) => l !== original[i]));

  async function refreshList() {
    info = (await attempt(() => api.get('/api/saves'))) || info;
    const row = (f, kind) => h('tr', { class: f.name === file ? 'sel' : '' },
      h('td', { class: 'mono' }, kind === 'file' ? h('a', { href: `#/${enc(ctx.ch)}/saves/${enc(f.name)}` }, f.name) : f.name),
      h('td', { class: 'small dim' }, new Date(f.modified * 1000).toLocaleString()),
      h('td', null, kind === 'copy' && file ? h('button', { class: 'small', onclick: () => loadCopy(f.name) }, `Load into ${file}`) : null,
        kind === 'copy' ? h('button', { class: 'small danger', onclick: () => delCopy(f.name) }, 'Delete') : null));
    add(clear(listBox),
      h('h3', null, 'Save slots'),
      info.files.length ? h('table', null, info.files.map((f) => row(f, 'file'))) : h('p', { class: 'dim' }, 'No filech* files in this folder.'),
      h('h3', null, 'Presets and backups'),
      info.copies.length ? h('table', null, info.copies.map((f) => row(f, 'copy'))) : h('p', { class: 'dim' }, 'None yet. "Save as preset" keeps a copy of a slot.'));
  }

  async function loadCopy(name) {
    if (!(await confirmDialog(`Replace ${file} with "${name}"? The current file goes to a backup.`, 'Replace'))) return;
    if (await attempt(() => api.post(`/api/saves/${enc(file)}/load`, { name }), `Loaded ${name}`)) { await openFile(); await refreshList(); }
  }

  async function delCopy(name) {
    if (!(await confirmDialog(`Delete "${name}"?`, 'Delete'))) return;
    if ((await attempt(() => api.del(`/api/save-copies/${enc(name)}`))) !== undefined) refreshList();
  }

  async function openFile() {
    editor.replaceChildren();
    if (!file) return;
    const t = await attempt(() => api.get(`/api/saves/${enc(file)}`));
    if (!t) return;
    lines = [...t.lines];
    original = [...t.lines];
    const n = /^filech(\d+)_/.exec(file)?.[1];
    const chId = `chapter${n}`;
    const chapter = ctx.state.chapters.find((c) => c.id === chId && c.extracted);
    layout = chapter ? await chapterApi(chId).saveLayout().catch(() => null) : null;
    const plotValues = chapter ? ((await chapterApi(chId).flags({ name: 'plot' }).catch(() => null))?.items?.[0]?.values || []) : [];
    drawEditor(chapter ? chId : null, plotValues);
  }

  function drawEditor(chId, plotValues) {
    const labels = new Map((layout?.fields || []).map((f) => [f.line, f.label]));
    const findLine = (pred) => [...labels.entries()].find(([, l]) => pred(l))?.[0];
    // Only lines that are in the file (a layout from another game version can be longer).
    const inFile = (i) => (i !== undefined && i < lines.length ? i : undefined);
    const plotLine = inFile(findLine((l) => l === 'global.plot'));
    const roomLine = inFile(findLine((l) => /room/i.test(l) && !/\[/.test(l)));
    const filter = h('input', { type: 'search', placeholder: 'Filter by label or value' });
    const body = h('tbody');
    const inputs = [];
    const drawRows = () => {
      const f = filter.value.toLowerCase();
      body.replaceChildren(...lines.map((v, i) => {
        const label = labels.get(i) || '';
        if (f && !label.toLowerCase().includes(f) && !String(v).toLowerCase().includes(f)) return null;
        const input = h('input', { type: 'text', value: v, style: { width: '100%' }, oninput: (e) => {
          lines[i] = e.target.value;
          e.target.closest('tr').classList.toggle('changed', lines[i] !== original[i]);
        } });
        inputs[i] = input;
        return h('tr', { class: v !== original[i] ? 'changed' : '' },
          h('td', { class: 'dim small' }, i + 1), h('td', { class: 'mono small' }, label), h('td', null, input));
      }).filter(Boolean));
    };
    const setLine = (i, v) => { lines[i] = v; if (inputs[i]) { inputs[i].value = v; inputs[i].closest('tr')?.classList.toggle('changed', v !== original[i]); } };
    const story = [];
    if (plotLine !== undefined) {
      const dl = h('datalist', { id: 'dl-plot-values' }, plotValues.map((v) => h('option', { value: v })));
      story.push(h('label', { class: 'field' }, h('span', null, `Plot (global.plot, line ${plotLine + 1})`),
        h('input', { type: 'text', list: 'dl-plot-values', value: lines[plotLine], oninput: (e) => setLine(plotLine, e.target.value) }), dl));
    }
    if (roomLine !== undefined) {
      story.push(h('label', { class: 'field' }, h('span', null, `Room (${labels.get(roomLine)}, line ${roomLine + 1})`),
        h('input', { type: 'text', value: lines[roomLine], oninput: (e) => setLine(roomLine, e.target.value) })));
    }
    const flagN = h('input', { type: 'number', placeholder: 'n', step: 1, min: 0 });
    const flagV = h('input', { type: 'text', placeholder: 'value', style: { width: '80px' } });
    story.push(h('label', { class: 'field' }, h('span', null, 'Set flag'), h('div', { class: 'row' }, flagN, flagV,
      h('button', { onclick: () => {
        const i = inFile(findLine((l) => l === `global.flag[${flagN.value}]`));
        if (i === undefined) { toast(`No line for global.flag[${flagN.value}]`, 'error'); return; }
        setLine(i, flagV.value);
      } }, 'Set'))));
    editor.replaceChildren();
    add(editor,
      h('div', { class: 'row' }, h('h3', { class: 'grow' }, file),
        h('button', { class: 'primary', onclick: save }, 'Save'),
        h('button', { onclick: saveAsPreset }, 'Save as preset'),
        h('button', { onclick: sendToGame, title: 'Send changed flags and globals to the running game (hot reload)' }, 'Send changes to game')),
      layout?.code ? h('p', { class: 'dim small' }, `Labels from ${layout.code}.`, chId ? null : '') : h('p', { class: 'note warn' }, chId ? 'No save layout found in the save script: lines have no labels.' : 'Extract this chapter to get line labels.'),
      (layout?.warnings || []).map((w) => h('p', { class: 'note warn small' }, w)),
      layout?.fields && layout.fields.length !== lines.length ? h('p', { class: 'note warn small' }, `The save script writes ${layout.fields.length} lines, the file has ${lines.length}. Labels can be wrong.`) : null,
      story.length ? h('div', { class: 'card' }, h('h3', null, 'Story point'), h('div', { class: 'fields' }, story)) : null,
      h('div', { class: 'row' }, filter),
      h('table', null, h('thead', null, h('tr', null, h('th', null, 'Line'), h('th', null, 'Variable'), h('th', null, 'Value'))), body));
    filter.addEventListener('input', drawRows);
    drawRows();
  }

  async function save() {
    const r = await attempt(() => api.put(`/api/saves/${enc(file)}`, { lines }), 'Saved');
    if (r) { original = [...lines]; await refreshList(); await openFile(); }
  }

  async function saveAsPreset() {
    if (lines.some((l, i) => l !== original[i])) { toast('Save the changes first: a preset copies the file on disk.', 'error'); return; }
    const name = await promptDialog('Preset name (letters, digits, _ - .)', `${file}-plot`);
    if (!name) return;
    if ((await attempt(() => api.post(`/api/saves/${enc(file)}/copy`, { name }), 'Preset saved')) !== undefined) refreshList();
  }

  async function sendToGame() {
    const labels = new Map((layout?.fields || []).map((f) => [f.line, f.label]));
    const cmds = [];
    lines.forEach((v, i) => {
      if (v === original[i]) return;
      const p = parseLabel(labels.get(i));
      const c = p && setCommand(p.name, p.index, v);
      if (c) cmds.push(c);
    });
    if (!cmds.length) { toast('No changed flag or global lines.'); return; }
    await attempt(() => live.send(cmds), `Sent ${cmds.length} values to the game`);
  }

  await refreshList();
  await openFile();
}
