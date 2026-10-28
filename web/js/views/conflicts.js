// Mod conflict checker: assets that two or more mods change, and whether the changes can go together.
import { h, attempt, add, clear } from '../ui.js';

const KIND = {
  conflict: ['badge bad', 'conflict'],
  merge: ['badge mod', 'different parts'],
  same: ['badge new', 'same change'],
};

export async function render(main, ctx) {
  const { capi, ch } = ctx;
  const page = h('div', { class: 'page' });
  main.append(page);
  const info = await attempt(() => capi.mods());
  const own = h('input', { type: 'checkbox', checked: true });
  const paths = h('textarea', { rows: 3, style: { width: '100%' }, placeholder: 'More mod folders, one per line (each a UTMT project folder with project.json)' });
  const out = h('div');
  add(page,
    h('h2', null, 'Mod conflicts'),
    h('p', { class: 'dim' }, 'Finds assets that two or more mods change. Code is compared line by line with the original: changes on different lines can go together, changes on the same or next lines conflict. Other assets are compared by field and by side file (sprite frames, sounds).'),
    h('div', { class: 'card col' },
      h('p', { class: 'small' }, 'Put other mods (UTMT project folders) in ', h('span', { class: 'mono' }, info?.folder || 'workspace/<chapter>/mods/'), '.'),
      info?.mods?.length ? h('table', null, info.mods.map((m) => h('tr', null, h('td', null, m.label), h('td', { class: 'dim small' }, `${m.assets} assets`), h('td', { class: 'mono small dim' }, m.dir))))
        : h('p', { class: 'dim small' }, 'No mods in that folder yet.'),
      h('label', { class: 'field check' }, own, h('span', null, `Include this mod (${ch}/mod)`)),
      paths,
      h('div', null, h('button', { class: 'primary', onclick: run }, 'Check'))),
    out);

  async function run() {
    const body = { own: own.checked, paths: paths.value.split('\n').map((x) => x.trim()).filter(Boolean) };
    add(clear(out), h('p', { class: 'dim' }, 'Checking...'));
    const r = await attempt(() => capi.conflicts(body));
    if (!r) { clear(out); return; }
    const n = (k) => r.findings.filter((f) => f.kind === k).length;
    clear(out);
    add(out,
      h('p', null, `${r.mods.length} mods: ${r.mods.map((m) => m.label).join(', ')}. `,
        `${n('conflict')} conflicts, ${n('merge')} assets changed in different parts, ${n('same')} same changes.`),
      r.mods.length < 2 ? h('p', { class: 'note warn' }, 'Give two or more mods to compare.') : null,
      r.findings.length ? h('table', null,
        h('tr', null, ['Result', 'Type', 'Asset', 'Mods', 'Details'].map((x) => h('th', null, x))),
        r.findings.map((f) => h('tr', null,
          h('td', null, h('span', { class: KIND[f.kind][0] }, KIND[f.kind][1])),
          h('td', null, f.type),
          h('td', null, h('a', { class: 'mono small', href: `#/${encodeURIComponent(ch)}/assets/${f.type}/${encodeURIComponent(f.name)}` }, f.name)),
          h('td', { class: 'small' }, f.mods.join(', ')),
          h('td', { class: 'small' }, f.details.map((d) => h('div', null, d)))))) : null);
  }
}
