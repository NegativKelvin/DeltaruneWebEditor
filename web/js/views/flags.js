// Flag and variable browser: every global.<name> and global.<name>[i] with the code that reads or writes it.
import { live } from '../api.js';
import { h, add, attempt, clear, debounce, promptDialog, toast } from '../ui.js';

const MAX_ROWS = 2000;

export const key = (x) => (x.index === null || x.index === undefined ? x.name : `${x.name}[${x.index}]`);

/** Live command that sets a global in the running game. Returns null when the value cannot go. */
export function setCommand(name, index, value) {
  const num = /^-?\d+(\.\d+)?$/.test(value);
  if (index === null || index === undefined) return { op: num ? 'global' : 'globalstr', args: [name, value] };
  if (name === 'flag' && /^\d+$/.test(index)) return { op: num ? 'flag' : 'flagstr', args: [index, value] };
  return null;
}

/** Asks the game for its flag values. Resolves with the array, or null. */
export async function readGameFlags() {
  const before = Date.now();
  if (!(await attempt(() => live.send([{ op: 'dumpflags', args: [] }])))) return null;
  for (let i = 0; i < 20; i++) {
    await new Promise((r) => setTimeout(r, 500));
    const f = await live.flags().catch(() => null);
    if (f && Date.now() - f.age_ms >= before - 200) return f.values;
  }
  toast('The game did not answer. Is it running with the hot reload object?', 'error');
  return null;
}

export async function render(main, ctx) {
  const { capi, ch } = ctx;
  const page = h('div', { class: 'page' });
  main.append(page);
  let notes = (await attempt(() => capi.flagNotes())) || {};
  let gameFlags = null;
  let selected = null;
  const q = h('input', { type: 'search', placeholder: 'Variable name (flag, plot, ...)', value: ctx.query.q || '', style: { width: '260px' } });
  const hideExpr = h('input', { type: 'checkbox', checked: true });
  const info = h('span', { class: 'dim small' });
  const refsBox = h('div');
  const table = h('div');
  const saveNotes = debounce(async () => { notes = (await attempt(() => capi.putFlagNotes(notes))) || notes; }, 600);
  let items = [];

  page.append(
    h('h2', null, 'Flags and variables'),
    h('p', { class: 'dim' }, 'Every global variable in the code of this chapter. Deltarune keeps story state in global.flag[n] and global.plot. Click a row to see where the game reads and writes it.'),
    h('div', { class: 'row' }, q,
      h('label', { class: 'field check' }, hideExpr, h('span', null, 'Hide indexes that are expressions')),
      h('button', { onclick: async () => { gameFlags = await readGameFlags(); draw(); } }, 'Read values from game'),
      info),
    refsBox, table);

  async function load() {
    history.replaceState(null, '', `#/${encodeURIComponent(ch)}/flags${q.value ? `?q=${encodeURIComponent(q.value)}` : ''}`);
    const r = await attempt(() => capi.flags({ q: q.value, limit: 100000 }));
    if (!r) return;
    items = r.items;
    draw();
  }

  function draw() {
    const shown = items.filter((x) => !hideExpr.checked || x.index === null || /^\d+$/.test(x.index));
    info.textContent = `${shown.length} variables${shown.length > MAX_ROWS ? ` (first ${MAX_ROWS} shown)` : ''}`;
    add(clear(table), h('table', null,
      h('tr', null, ['Variable', 'Reads', 'Writes', 'Values in code', 'Note', gameFlags ? 'Game' : null, ''].filter((x) => x !== null).map((x) => h('th', null, x))),
      shown.slice(0, MAX_ROWS).map((x) => {
        const k = key(x);
        const game = gameFlags && x.name === 'flag' && /^\d+$/.test(x.index ?? '') ? gameFlags[Number(x.index)] ?? '' : '';
        return h('tr', { class: selected === k ? 'sel' : '', onclick: (e) => { if (!e.target.closest('input,button')) showRefs(x); } },
          h('td', { class: 'mono' }, `global.${k}`),
          h('td', null, x.reads), h('td', null, x.writes),
          h('td', { class: 'mono small' }, x.values.slice(0, 10).join(', '), x.values.length > 10 ? ' ...' : ''),
          h('td', null, h('input', { type: 'text', value: notes[k] || '', placeholder: 'note', oninput: (e) => { notes[k] = e.target.value; saveNotes(); } })),
          gameFlags ? h('td', { class: 'mono' }, game) : null,
          h('td', null, setCommand(x.name, x.index, '0') ? h('button', { class: 'small', onclick: () => setInGame(x) }, 'Set in game') : null));
      })));
  }

  async function setInGame(x) {
    const v = await promptDialog(`Set global.${key(x)} in the running game`, '1');
    if (v === null) return;
    const cmd = setCommand(x.name, x.index, v);
    if (cmd && (await attempt(() => live.send([cmd]), 'Sent to game'))) {
      if (gameFlags && cmd.op.startsWith('flag')) gameFlags[Number(x.index)] = v;
      draw();
    }
  }

  async function showRefs(x) {
    selected = key(x);
    draw();
    const refs = await attempt(() => capi.flagRefs(x.name, x.index ?? undefined));
    if (!refs) return;
    const link = (r) => `#/${encodeURIComponent(ch)}/assets/code/${encodeURIComponent(r.code)}?line=${r.line}`;
    const part = (write) => refs.filter((r) => r.write === write).map((r) => h('tr', null,
      h('td', null, h('a', { class: 'mono small', href: link(r) }, `${r.code}:${r.line}`)),
      h('td', { class: 'mono small' }, r.detail || '')));
    add(clear(refsBox), h('div', { class: 'card' },
      h('div', { class: 'row' }, h('h3', { class: 'grow' }, `global.${selected}`), h('button', { class: 'small', onclick: () => { selected = null; add(clear(refsBox), ); draw(); } }, 'Close')),
      h('h3', null, `Writes (${x.writes})`), h('table', null, part(true)),
      h('h3', null, `Reads (${x.reads})`), h('table', null, part(false))));
  }

  q.addEventListener('input', debounce(load, 300));
  hideExpr.addEventListener('change', draw);
  load();
}
