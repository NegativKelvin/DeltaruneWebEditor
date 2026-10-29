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
  const versionBox = h('div');
  const jobs = h('div');
  add(page,
    h('h2', null, `${ch}`),
    h('p', { class: 'dim mono small' }, chapter.data_file),
    chapter.game_changed ? h('div', { class: 'note warn' }, 'The game data file changed since the extract (game update?). ',
      h('a', { href: '#/' }, 'Extract again'), '. Your changes stay. Then check the version list below.') : null,
    h('div', { class: 'card' }, h('div', { class: 'fields' },
      counts.map((c) => h('a', { href: `#/${encodeURIComponent(ch)}/assets/${c.slug}`, class: 'button' }, `${labels[c.slug]}: ${c.count}`)))),
    h('div', { class: 'row' }, h('h2', null, 'Changes'), h('div', { class: 'spacer grow' }),
      h('button', { class: 'primary', onclick: build }, 'Build data.win')),
    changesBox,
    versionBox,
    jobs,
  );

  const CHECK = {
    changed: 'the game changed this asset after your change started; your version replaces it',
    removed: 'the game no longer has this asset',
    name_taken: 'your new asset has a name that the game now uses',
    unknown: 'no record of the game version',
  };
  async function loadVersions() {
    const r = await capi.versionCheck();
    const bad = r.items.filter((i) => CHECK[i.status]);
    versionBox.replaceChildren();
    if (!bad.length) return;
    versionBox.append(h('h2', null, 'Version check'), h('div', { class: 'card' },
      h('p', { class: 'dim' }, 'These changes started from another game version. Open each one and compare. Keep: mark it as checked. Revert: take the game version.'),
      h('table', null,
        h('tr', null, h('th', null, 'Asset'), h('th', null, 'State'), h('th', null, '')),
        bad.map((i) => h('tr', null,
          h('td', null, h('a', { class: 'mono', href: `#/${encodeURIComponent(ch)}/assets/${i.type}/${encodeURIComponent(i.name)}` }, `${i.type}/${i.name}`)),
          h('td', null, h('span', { class: 'badge mod' }, i.status.replace('_', ' ')), ' ', h('span', { class: 'dim small' }, CHECK[i.status])),
          h('td', null,
            i.status !== 'removed' ? h('button', { class: 'small', onclick: async () => {
              if (await attempt(() => capi.acceptVersion(i.type, i.name), 'Marked as checked')) loadVersions();
            } }, 'Keep') : null,
            h('button', { class: 'small danger', onclick: async () => {
              if (!(await confirmDialog(`Revert ${i.type}/${i.name}? Your change is lost.`, 'Revert'))) return;
              if (await attempt(() => capi.revert(i.type, i.name), 'Reverted')) { loadChanges(); loadVersions(); }
            } }, 'Revert')))))));
  }

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
    'Build writes workspace/', ch, '/build/data.win. It imports only the changed assets into a copy of the original data file. GML compile errors show in the log with the code entry name and line. ',
    'To share the changes, use ', h('a', { href: '#/export' }, 'Export mod'), '.'));
  await Promise.all([loadChanges(), loadVersions()]);
}
