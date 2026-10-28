// Read-only browser for the generic dump of every list in the data file.
import { h, attempt } from '../ui.js';

/** Collapsible JSON tree. */
export function jsonTree(v, key = null, depth = 0) {
  if (v === null || typeof v !== 'object') {
    const cls = typeof v === 'string' ? 'tok-str' : typeof v === 'number' ? 'tok-num' : 'tok-kw';
    return h('div', { class: 'mono small' }, key !== null ? h('span', { class: 'dim' }, `${key}: `) : null,
      h('span', { class: cls }, JSON.stringify(v)));
  }
  if (v.$ref !== undefined) {
    return h('div', { class: 'mono small' }, key !== null ? h('span', { class: 'dim' }, `${key}: `) : null,
      h('span', { class: 'tok-glob' }, `→ ${v.$ref}`), h('span', { class: 'dim' }, ` (${v.$kind})`));
  }
  const entries = Array.isArray(v) ? v.map((x, i) => [i, x]) : Object.entries(v);
  const label = Array.isArray(v) ? `[${v.length}]` : (v.$kind || '{…}');
  const d = h('details', { open: depth < 1 }, h('summary', null, key !== null ? `${key}: ` : '', label));
  let filled = false;
  d.addEventListener('toggle', () => {
    if (!d.open || filled) return;
    filled = true;
    for (const [k, x] of entries.slice(0, 500)) d.append(jsonTree(x, k, depth + 1));
    if (entries.length > 500) d.append(h('div', { class: 'dim small' }, `… ${entries.length - 500} more`));
  });
  if (d.open) d.dispatchEvent(new Event('toggle'));
  return d;
}

export async function render(main, ctx) {
  const { capi, ch, arg } = ctx;
  const page = h('div', { class: 'split' });
  main.append(page);
  const index = await capi.generic();
  const items = h('div', { class: 'items' }, Object.entries(index).sort().map(([k, n]) =>
    h('a', { href: `#/${encodeURIComponent(ch)}/data/${encodeURIComponent(k)}`, class: k === arg ? 'active' : '' }, k, h('span', { class: 'dim' }, n < 0 ? '1' : String(n)))));
  const body = h('div', { class: 'editor-pane' });
  page.append(h('div', { class: 'list-pane' }, h('div', { class: 'list-head' }, h('b', null, 'Data lists'),
    h('span', { class: 'dim small' }, 'Read-only view of every list in data.win. Edit assets in the editors.')), items), body);
  if (!arg) { body.append(h('div', { class: 'editor-body dim' }, 'Select a list.')); return; }

  const q = h('input', { type: 'search', placeholder: 'Filter by name' });
  const out = h('div', { class: 'editor-body' });
  let offset = 0;
  const load = async () => {
    const r = await attempt(() => capi.genericList(arg, { q: q.value, offset, limit: 100 }));
    if (!r) return;
    out.replaceChildren();
    if (r.items === undefined) { out.append(jsonTree(r)); return; }
    out.append(h('div', { class: 'row' }, h('span', { class: 'dim' }, `${r.total} items, ${offset + 1}-${Math.min(offset + 100, r.total)}`),
      h('button', { disabled: offset === 0, onclick: () => { offset = Math.max(0, offset - 100); load(); } }, 'Previous'),
      h('button', { disabled: offset + 100 >= r.total, onclick: () => { offset += 100; load(); } }, 'Next')));
    for (const it of r.items) out.append(jsonTree(it.value, it.$index, 1));
  };
  q.addEventListener('keydown', (e) => { if (e.key === 'Enter') { offset = 0; load(); } });
  body.append(h('div', { class: 'editor-head' }, h('h2', null, arg), q), out);
  load();
}
