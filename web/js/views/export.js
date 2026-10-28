// Export mod: make a .dremod package of the changes. Translation changes (lang files) by key.
import { api, chapterApi } from '../api.js';
import { h, attempt, confirmDialog, toast, add, promptDialog } from '../ui.js';

const enc = encodeURIComponent;

function size(n) {
  return n > 1 << 20 ? `${(n / (1 << 20)).toFixed(1)} MB` : `${Math.ceil(n / 1024)} KB`;
}

export async function render(main, ctx) {
  const page = h('div', { class: 'page' });
  main.append(page);
  const chapters = ctx.state.chapters.filter((c) => c.extracted);
  if (!chapters.length) { page.append(h('p', null, 'No chapter is extracted. Go to ', h('a', { href: '#/' }, 'Setup'), '.')); return; }

  const form = { id: '', name: '', version: '1.0.0', author: '', description: '' };
  const input = (key, label, attrs = {}) => h('label', { class: 'field' }, h('span', null, label),
    h('input', { type: 'text', value: form[key], oninput: (e) => { form[key] = e.target.value; }, ...attrs }));
  const picks = {};
  const chapterBox = h('div', { class: 'fields' });
  const result = h('div');
  const exportsBox = h('div', { class: 'card' });
  const langBox = h('div', { class: 'card' });

  add(page,
    h('h2', null, 'Export mod'),
    h('p', { class: 'dim' },
      'A .dremod package has the changed and new assets (by name) and the changed translation keys. It never has data.win or other game files. ',
      'Players install it in ', h('a', { href: '#/mods' }, 'Mods and backups'), ' with one click; it is built on their own game files.'),
    h('div', { class: 'card col' },
      h('div', { class: 'fields' },
        input('id', 'Id (letters, digits, _ - .)', { placeholder: 'my_mod', pattern: '[A-Za-z0-9_.-]+' }),
        input('name', 'Name'),
        input('version', 'Version'),
        input('author', 'Author')),
      h('label', { class: 'field' }, h('span', null, 'Description'),
        h('textarea', { rows: 2, oninput: (e) => { form.description = e.target.value; } })),
      h('div', null, h('span', { class: 'dim small' }, 'Chapters: '), chapterBox),
      h('div', null, h('button', { class: 'primary', onclick: doExport }, 'Export .dremod'))),
    result,
    h('h2', null, 'Version check'),
    h('p', { class: 'dim' }, 'Each changed asset keeps the hash of the game version it started from. After a game update, extract the chapter again; the chapter overview then lists changes that the game also changed.'),
    h('h2', null, 'Translation changes (lang files)'),
    langBox,
    h('h2', null, 'Exported packages'),
    exportsBox);

  for (const c of chapters) {
    picks[c.id] = true;
    chapterBox.append(h('label', { class: 'field check' },
      h('input', { type: 'checkbox', checked: true, onchange: (e) => { picks[c.id] = e.target.checked; } }),
      h('a', { href: `#/${enc(c.id)}` }, c.id), c.game_changed ? h('span', { class: 'badge mod' }, 'game updated') : null));
  }

  async function doExport() {
    if (!/^[A-Za-z0-9_.-]+$/.test(form.id) || form.id.startsWith('.')) { toast('Give an id: letters, digits, _ - .', 'error'); return; }
    if (form.version && !/^[A-Za-z0-9_.-]+$/.test(form.version)) { toast('Version: letters, digits, _ - . only', 'error'); return; }
    const body = { ...form, chapters: chapters.map((c) => c.id).filter((id) => picks[id]) };
    const r = await attempt(() => api.post('/api/export', body), 'Exported');
    if (!r) return;
    const parts = Object.entries(r.manifest.chapters).map(([id, c]) => `${id}: ${c.assets.length} assets, ${c.lang.length} lang files`);
    result.replaceChildren(h('div', { class: 'note' },
      h('a', { href: `/api/exports/${enc(r.file)}`, download: r.file }, `Download ${r.file}`), ` (${size(r.size)}). `, parts.join('; ')));
    loadExports();
  }

  async function loadExports() {
    const list = await api.get('/api/exports');
    exportsBox.replaceChildren(list.length ? h('table', null,
      h('tr', null, h('th', null, 'File'), h('th', null, 'Size'), h('th', null, 'Time')),
      list.map((e) => h('tr', null,
        h('td', null, h('a', { class: 'mono', href: `/api/exports/${enc(e.file)}`, download: e.file }, e.file)),
        h('td', null, size(e.size)),
        h('td', null, new Date(e.modified * 1000).toLocaleString())))) : h('p', { class: 'dim' }, 'None yet. Files are in workspace/exports.'));
  }

  // Translation changes: one chapter and file at a time.
  let langCh = chapters[0].id;
  let langFile = '';
  async function loadLang() {
    const capi = chapterApi(langCh);
    const idx = await capi.langIndex();
    const files = [...new Set([...idx.files, ...Object.keys(idx.changed)])];
    if (!files.includes(langFile)) langFile = files[0] || '';
    const chSel = h('select', { onchange: (e) => { langCh = e.target.value; langFile = ''; loadLang(); } },
      chapters.map((c) => h('option', { value: c.id, selected: c.id === langCh }, c.id)));
    const fileSel = h('select', { onchange: (e) => { langFile = e.target.value; loadLang(); } },
      files.map((f) => h('option', { value: f, selected: f === langFile }, `${f}${idx.changed[f] ? ` (${idx.changed[f]} changed)` : ''}`)));
    const head = h('div', { class: 'row' }, chSel, fileSel);
    if (!langFile) { langBox.replaceChildren(head, h('p', { class: 'dim' }, 'This chapter has no lang/*.json files.')); return; }
    const ov = await capi.lang(langFile);
    const set = async (changes, msg) => { if (await attempt(() => capi.putLang(langFile, changes), msg)) loadLang(); };
    langBox.replaceChildren(head,
      h('p', { class: 'dim small' }, 'Keys are the third argument of msgsetloc/stringsetloc (see Dialogue). Only these keys change in the game file.'),
      h('table', null,
        h('tr', null, h('th', null, 'Key'), h('th', null, 'New text'), h('th', null, '')),
        Object.entries(ov).map(([k, v]) => h('tr', null,
          h('td', { class: 'mono small' }, k),
          h('td', null, h('input', { type: 'text', value: v, style: { width: '100%' }, onchange: (e) => set({ [k]: e.target.value }, 'Saved') })),
          h('td', null, h('button', { class: 'small danger', onclick: () => set({ [k]: null }) }, 'Remove'))))),
      h('div', { class: 'row' },
        h('button', { onclick: async () => {
          const k = await promptDialog('Key', '', 'obj_talk_slash_Create_0_gml_2_0');
          if (!k) return;
          const v = await promptDialog(`Text for ${k}`, ov[k] || '');
          if (v !== null) set({ [k]: v }, 'Saved');
        } }, 'Add key'),
        Object.keys(ov).length ? h('button', { class: 'danger', onclick: async () => {
          if (await confirmDialog(`Remove all changes of ${langFile}?`, 'Remove') && await attempt(() => capi.clearLang(langFile))) loadLang();
        } }, 'Remove all') : null));
  }

  await Promise.all([loadExports(), loadLang()]);
}
