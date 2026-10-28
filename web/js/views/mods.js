// Mod loader: install .dremod packages, load order, apply to the game, version check, game and save backups.
import { api } from '../api.js';
import { h, attempt, confirmDialog, modal, toast, add, clear, pickFiles } from '../ui.js';
import { followJob } from './jobs.js';

const enc = encodeURIComponent;
const STATE = {
  original: ['', 'original'],
  modded: ['new', 'mods in game'],
  game_updated: ['mod', 'game updated'],
};
const CHECK = {
  same: ['new', 'same'], new: ['new', 'new asset'], changed: ['mod', 'game changed it'],
  removed: ['mod', 'not in game'], name_taken: ['mod', 'name used by game'], unknown: ['', 'unknown'],
};

function badge([cls, text]) {
  return h('span', { class: `badge ${cls}` }, text);
}

/** Replaces the children; skips null like `add`. */
function fill(el, ...children) {
  return add(clear(el), ...children);
}

function size(n) {
  return n > 1 << 20 ? `${(n / (1 << 20)).toFixed(1)} MB` : `${Math.ceil(n / 1024)} KB`;
}

export async function render(main) {
  const page = h('div', { class: 'page' });
  main.append(page);
  const top = h('div');
  const chaptersBox = h('div', { class: 'card' });
  const modsBox = h('div', { class: 'card' });
  const savesBox = h('div', { class: 'card' });
  const jobs = h('div');
  add(page,
    h('h2', null, 'Mods and backups'),
    h('p', { class: 'dim' },
      'Install .dremod packages, then Apply. Apply backs up the original game files and the save folder first. ',
      'The original data files stay in workspace/loader/backup. A package has only the changes, never data.win.'),
    top, jobs,
    h('h2', null, 'Game'), chaptersBox,
    h('h2', null, 'Installed mods'), modsBox,
    h('h2', null, 'Save backups'), savesBox);

  let status;
  async function load() {
    try {
      status = await api.get('/api/loader');
    } catch (e) {
      fill(top, h('div', { class: 'error-box' }, e.status === 409 ? 'Set the Deltarune folder in Setup first.' : e.message));
      return false;
    }
    renderTop();
    renderChapters();
    renderMods();
    await renderSaves();
    return true;
  }

  function renderTop() {
    const pending = status.chapters.some((c) => c.pending);
    fill(top, h('div', { class: 'row' },
      h('button', { class: 'primary', onclick: installAndApply }, 'Install .dremod and apply'),
      h('button', { onclick: install }, 'Install only'),
      h('button', { class: pending ? 'primary' : '', onclick: apply }, pending ? 'Apply changes to game' : 'Apply again'),
      h('div', { class: 'spacer grow' }),
      h('button', { class: 'danger', onclick: restore }, 'Restore original game files')),
    pending ? h('div', { class: 'note warn' }, 'The game does not have the current mod list yet. Press Apply.') : null,
    status.utmt_cli ? null : h('div', { class: 'note warn' }, 'UndertaleModCli is not set (Setup). Mods that change assets need it. Mods with only translation changes do not.'));
  }

  function renderChapters() {
    fill(chaptersBox, h('table', null,
      h('tr', null, h('th', null, 'Chapter'), h('th', null, 'State'), h('th', null, 'Mods in game'), h('th', null, 'Enabled mods'), h('th', null, 'Backup')),
      status.chapters.map((c) => h('tr', null,
        h('td', null, c.id),
        h('td', null, badge(STATE[c.state] || ['', c.state]), c.pending ? h('span', { class: 'badge mod' }, 'apply needed') : null),
        h('td', { class: 'mono small' }, c.applied_mods.join(', ') || '-'),
        h('td', { class: 'mono small' }, c.wanted_mods.join(', ') || '-'),
        h('td', null, c.backup ? 'yes' : '-')))),
    status.chapters.some((c) => c.state === 'game_updated')
      ? h('div', { class: 'note warn' }, 'The game changed its files (update or Steam file check). Apply puts the mods on the new version and keeps it as the new backup. Use Check on each mod first.')
      : null);
  }

  function renderMods() {
    const mods = status.mods;
    if (!mods.length) { modsBox.replaceChildren(h('p', { class: 'dim' }, 'No mods installed.')); return; }
    const save = async (list) => {
      if (await attempt(() => api.put('/api/loader/mods', list.map(({ id, enabled }) => ({ id, enabled }))))) load();
    };
    const move = (i, d) => {
      const list = [...mods];
      [list[i], list[i + d]] = [list[i + d], list[i]];
      save(list);
    };
    modsBox.replaceChildren(
      h('p', { class: 'dim small' }, 'Load order: a mod lower in the list wins when two mods change the same asset or text key.'),
      h('table', null,
        h('tr', null, h('th', null, 'On'), h('th', null, 'Mod'), h('th', null, 'Version'), h('th', null, 'Chapters'), h('th', null, ''), h('th', null, '')),
        mods.map((m, i) => {
          const man = m.manifest || {};
          const chapters = Object.entries(man.chapters || {}).map(([id, c]) => `${id} (${c.assets.length} assets${c.lang.length ? `, ${c.lang.length} lang` : ''})`);
          return h('tr', null,
            h('td', null, h('input', { type: 'checkbox', checked: m.enabled, onchange: (e) => save(mods.map((x) => (x.id === m.id ? { ...x, enabled: e.target.checked } : x))) })),
            h('td', null, h('b', null, man.name || m.id), man.author ? h('span', { class: 'dim' }, ` by ${man.author}`) : null,
              man.description ? h('div', { class: 'dim small' }, man.description) : null),
            h('td', { class: 'mono small' }, man.version || '-'),
            h('td', { class: 'small' }, chapters.join(', ')),
            h('td', null,
              h('button', { class: 'small', disabled: i === 0, onclick: () => move(i, -1) }, '↑'),
              h('button', { class: 'small', disabled: i === mods.length - 1, onclick: () => move(i, 1) }, '↓')),
            h('td', null,
              h('button', { class: 'small', onclick: () => check(m.id, man.name || m.id) }, 'Check'),
              h('button', { class: 'small danger', onclick: () => uninstall(m.id) }, 'Uninstall')));
        })));
  }

  async function renderSaves() {
    const s = await api.get('/api/loader/saves');
    fill(savesBox,
      h('p', { class: 'dim' }, 'Save folder: ', h('span', { class: 'mono' }, s.save_dir || 'not found (set it in Setup)')),
      h('div', { class: 'row' }, h('button', { disabled: !s.save_dir, onclick: async () => {
        if (await attempt(() => api.post('/api/loader/saves'), 'Saves backed up')) renderSaves();
      } }, 'Back up saves now')),
      s.backups.length ? h('table', null,
        h('tr', null, h('th', null, 'Backup'), h('th', null, 'Time'), h('th', null, 'Files'), h('th', null, '')),
        s.backups.map((b) => h('tr', null,
          h('td', { class: 'mono small' }, b.name),
          h('td', null, new Date(b.created).toLocaleString()),
          h('td', null, `${b.files} (${size(b.bytes)})`),
          h('td', null,
            h('button', { class: 'small', disabled: !s.save_dir, onclick: async () => {
              if (!(await confirmDialog(`Replace the save folder with backup ${b.name}? The current saves get a backup first.`, 'Restore'))) return;
              if (await attempt(() => api.post(`/api/loader/saves/${enc(b.name)}/restore`), 'Saves restored')) renderSaves();
            } }, 'Restore'),
            h('button', { class: 'small danger', onclick: async () => {
              if (!(await confirmDialog(`Delete backup ${b.name}?`, 'Delete'))) return;
              if (await attempt(() => api.del(`/api/loader/saves/${enc(b.name)}`))) renderSaves();
            } }, 'Delete'))))) : h('p', { class: 'dim' }, 'No save backups yet.'));
  }

  async function install() {
    const [file] = await pickFiles('.dremod,.zip');
    if (!file) return null;
    const m = await attempt(() => api.postRaw('/api/loader/mods', file), `Installed ${file.name}`);
    await load();
    return m;
  }

  async function installAndApply() {
    const m = await install();
    if (m) await apply();
  }

  async function apply() {
    const r = await attempt(() => api.post('/api/loader/apply'));
    if (!r) return;
    const job = await followJob(jobs, r.job, 'Apply mods');
    toast(job.status === 'succeeded' ? 'Mods applied. Start the game.' : 'Apply failed. See the log.', job.status === 'succeeded' ? 'ok' : 'error');
    load();
  }

  async function restore() {
    if (!(await confirmDialog('Put the original game files back? Mods stay installed. Apply puts them in again.', 'Restore'))) return;
    const r = await attempt(() => api.post('/api/loader/restore'));
    if (r) toast(r.restored.length ? `Restored: ${r.restored.join(', ')}` : 'Nothing to restore', 'ok');
    load();
  }

  async function uninstall(id) {
    if (!(await confirmDialog(`Uninstall ${id}? Press Apply after this to take it out of the game.`, 'Uninstall'))) return;
    if (await attempt(() => api.del(`/api/loader/mods/${enc(id)}`), 'Uninstalled')) load();
  }

  async function check(id, name) {
    const r = await attempt(() => api.get(`/api/loader/mods/${enc(id)}/check`));
    if (!r) return;
    const body = h('div', null, r.map((c) => {
      if (!c.in_game) return h('div', { class: 'note warn' }, `${c.chapter}: not in this game.`);
      const bad = c.assets.filter((a) => !['same', 'new'].includes(a.status));
      return h('div', { class: 'card' },
        h('h3', null, c.chapter, ' ', c.same_version ? badge(['new', 'same game version']) : badge(['mod', 'other game version'])),
        !c.checked ? h('p', { class: 'dim' }, 'The mod was made on another game version. Extract this chapter (Setup) to check each asset.') : null,
        c.checked && !bad.length && c.assets.length ? h('p', null, `All ${c.assets.length} changed assets match this game version.`) : null,
        bad.length ? h('table', null, h('tr', null, h('th', null, 'Asset'), h('th', null, 'State')),
          bad.map((a) => h('tr', null, h('td', { class: 'mono small' }, `${a.type}/${a.name}`), h('td', null, badge(CHECK[a.status] || ['', a.status]))))) : null,
        bad.length ? h('p', { class: 'dim small' }, '"Game changed it": the mod replaces the new game version of this asset. Test it in the game.') : null,
        c.lang.length ? h('p', { class: 'dim small' }, `Translation changes: ${c.lang.join(', ')} (by key, safe on other versions).`) : null);
    }));
    modal(`Version check: ${name}`, body);
  }

  await load();
}
