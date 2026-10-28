// Plain-text search in all GML.
import { h, attempt } from '../ui.js';

export async function render(main, ctx) {
  const { capi, ch } = ctx;
  const page = h('div', { class: 'page' });
  const input = h('input', { type: 'search', placeholder: 'Text to find in all code', style: { width: '400px' }, value: ctx.query.q || '' });
  const out = h('div');
  const run = async () => {
    if (!input.value) return;
    history.replaceState(null, '', `#/${encodeURIComponent(ch)}/search?q=${encodeURIComponent(input.value)}`);
    out.replaceChildren(h('p', { class: 'dim' }, 'Searching...'));
    const hits = await attempt(() => capi.search(input.value, 2000));
    if (!hits) return;
    out.replaceChildren(h('p', { class: 'dim' }, `${hits.length} results${hits.length >= 2000 ? ' (limit)' : ''}`),
      h('table', null, hits.map((x) => h('tr', null,
        h('td', null, h('a', { class: 'mono small', href: `#/${encodeURIComponent(ch)}/assets/code/${encodeURIComponent(x.code)}?line=${x.line}` }, `${x.code}:${x.line}`)),
        h('td', { class: 'mono small' }, x.text)))));
  };
  input.addEventListener('keydown', (e) => { if (e.key === 'Enter') run(); });
  page.append(h('h2', null, 'Search code'), h('div', { class: 'row' }, input, h('button', { class: 'primary', onclick: run }, 'Search')), out);
  main.append(page);
  if (input.value) run();
}
