// Setup: game folder, UndertaleModCli path, chapter extraction.
import { api, chapterApi } from '../api.js';
import { h, attempt, toast, add } from '../ui.js';
import { state, refreshChapters, go } from '../app.js';
import { followJob } from './jobs.js';

export async function render(main) {
  const page = h('div', { class: 'page' });
  main.append(page);
  const [health, settings] = await Promise.all([api.get('/api/health'), api.get('/api/settings')]);
  const s = { game_dir: settings.game_dir || '', utmt_cli: settings.utmt_cli || '', save_dir: settings.save_dir || '' };
  const game = h('input', { type: 'text', value: s.game_dir, style: { width: '100%' }, placeholder: 'C:/Program Files (x86)/Steam/steamapps/common/DELTARUNE' });
  const saves = h('input', { type: 'text', value: s.save_dir, style: { width: '100%' }, placeholder: 'empty: found automatically (Windows: %LOCALAPPDATA%/DELTARUNE)' });
  const cli = h('input', { type: 'text', value: s.utmt_cli, style: { width: '100%' }, placeholder: 'C:/tools/UTMT/UndertaleModCli.exe' });
  add(page,
    h('h2', null, 'Setup'),
    h('p', { class: 'dim' }, `Server ${health.version}. The editor does not change game files: edits go to workspace/<chapter>/mod, builds to workspace/<chapter>/build. Only Apply in Mods and backups writes to the game folder, after a backup.`),
    h('div', { class: 'card col' },
      h('label', { class: 'field' }, h('span', null, 'Deltarune folder (holds data.win and chapterN_windows)'), game),
      h('label', { class: 'field' }, h('span', null, 'UndertaleModCli executable (0.9.2.0 or later)'), cli),
      h('label', { class: 'field' }, h('span', null, 'Deltarune save folder (for save backups in Mods and backups)'), saves),
      h('div', null, h('button', { class: 'primary', onclick: async () => {
        const body = {};
        if (game.value.trim()) body.game_dir = game.value.trim();
        if (cli.value.trim()) body.utmt_cli = cli.value.trim();
        if (saves.value.trim()) body.save_dir = saves.value.trim();
        if (await attempt(() => api.put('/api/settings', body), 'Settings saved')) {
          await refreshChapters();
          renderChapters();
        }
      } }, 'Save settings'))),
  );
  const chapters = h('div', { class: 'card' });
  const jobs = h('div');
  page.append(h('h2', null, 'Chapters'), chapters, jobs);

  function renderChapters() {
    chapters.replaceChildren();
    if (!state.chapters.length) {
      chapters.append(h('p', { class: 'dim' }, 'No chapters found. Set the Deltarune folder above.'));
      return;
    }
    chapters.append(h('table', null,
      h('tr', null, h('th', null, 'Chapter'), h('th', null, 'Data file'), h('th', null, 'State'), h('th', null, '')),
      state.chapters.map((c) => h('tr', null,
        h('td', null, c.extracted ? h('a', { href: `#/${encodeURIComponent(c.id)}` }, c.id) : c.id),
        h('td', { class: 'mono small' }, c.data_file),
        h('td', null, c.extracted ? h('span', { class: 'badge new' }, 'extracted') : h('span', { class: 'badge' }, 'not extracted'),
          c.game_changed ? h('span', { class: 'badge mod' }, 'game updated: extract again') : null),
        h('td', null, h('button', { onclick: () => extract(c.id, c.extracted) }, c.extracted ? 'Extract again' : 'Extract'))))));
  }

  async function extract(id, again) {
    if (again) toast('Extract again replaces base/ only. Your changes in mod/ stay.');
    const r = await attempt(() => chapterApi(id).extract());
    if (!r) return;
    const job = await followJob(jobs, r.job, `Extract ${id}`);
    await refreshChapters();
    renderChapters();
    if (job.status === 'succeeded') {
      toast(`${id} extracted`, 'ok');
      go(`#/${encodeURIComponent(id)}`);
    }
  }

  renderChapters();
}
