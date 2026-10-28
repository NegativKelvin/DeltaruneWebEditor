// Chapter overview: asset counts, changes, build.
import { h, attempt, confirmDialog, toast, add } from '../ui.js';
import { followJob } from './jobs.js';
import { refreshChapters } from '../app.js';

export async function render(main, ctx) {
  const { capi, ch, labels } = ctx;
  const page = h('div', { class: 'page' });
  main.append(page);
  const chapter = ctx.state.chapters.find((c) => c.id === ch);
  if (!chapter) { page.append(h('p', null, `Chapter ${ch} is not known. Check Setup.`)); return; }
  if (!chapter.extracted) { page.append(h('p', null, `Chapter ${ch} is not extracted. Go to `, h('a', { href: '#/' }, 'Setup'), '.')); return; }

  const counts = await capi.counts();
  const changesBox = h('div', { class: 'card' });
  const jobs = h('div');
  add(page,
    h('h2', null, `${ch}`),
    h('p', { class: 'dim mono small' }, chapter.data_file),
    h('div', { class: 'card' }, h('div', { class: 'fields' },
      counts.map((c) => h('a', { href: `#/${encodeURIComponent(ch)}/assets/${c.slug}`, class: 'button' }, `${labels[c.slug]}: ${c.count}`)))),
    h('div', { class: 'row' }, h('h2', null, 'Changes'), h('div', { class: 'spacer grow' }),
      h('button', { class: 'primary', onclick: build }, 'Build data.win')),
    changesBox,
    jobs,
  );

  async function loadChanges() {
    const changes = await capi.changes();
    changesBox.replaceChildren();
    const types = Object.keys(changes);
    if (!types.length) { changesBox.append(h('p', { class: 'dim' }, 'No changes yet.')); return; }
    changesBox.append(h('table', null,
      h('tr', null, h('th', null, 'Type'), h('th', null, 'Asset'), h('th', null, ''), h('th', null, '')),
      types.flatMap((t) => changes[t].map((e) => h('tr', null,
        h('td', null, labels[t]),
        h('td', null, h('a', { class: 'mono', href: `#/${encodeURIComponent(ch)}/assets/${t}/${encodeURIComponent(e.name)}` }, e.name)),
        h('td', null, e.added ? h('span', { class: 'badge new' }, 'new') : h('span', { class: 'badge mod' }, 'changed')),
        h('td', null, h('button', { class: 'small danger', onclick: async () => {
          if (!(await confirmDialog(`${e.added ? 'Delete the new asset' : 'Revert'} ${t}/${e.name}?`, e.added ? 'Delete' : 'Revert'))) return;
          if (await attempt(() => capi.revert(t, e.name), 'Reverted')) loadChanges();
        } }, e.added ? 'Delete' : 'Revert')))))));
  }

  async function build() {
    const r = await attempt(() => capi.build());
    if (!r) return;
    const job = await followJob(jobs, r.job, `Build ${ch}`);
    if (job.status === 'succeeded') toast('Build done: workspace/' + ch + '/build/data.win', 'ok');
    else toast('Build failed. See the log.', 'error');
    refreshChapters();
  }

  page.append(h('div', { class: 'note' },
    'Build writes workspace/', ch, '/build/data.win. It imports only the changed assets into a copy of the original data file. GML compile errors show in the log with the code entry name and line.'));
  await loadChanges();
}
