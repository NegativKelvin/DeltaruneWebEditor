// App shell: chapter state, navigation, hash router, unsaved-change guard.
import { api, chapterApi } from './api.js';
import { h, clear, toast, errMsg, confirmDialog, forgetDatalists, add } from './ui.js';

const ASSET_LABELS = {
  rooms: 'Rooms', objects: 'Objects', code: 'Code', scripts: 'Scripts', sprites: 'Sprites', backgrounds: 'Backgrounds / tilesets',
  sounds: 'Sounds', fonts: 'Fonts', shaders: 'Shaders', paths: 'Paths', sequences: 'Sequences', animcurves: 'Animation curves',
};

export const state = {
  chapters: [],
  ch: null,
  counts: {},
};

const views = {
  home: () => import('./views/home.js'),
  overview: () => import('./views/build.js'),
  assets: () => import('./views/assets.js'),
  dialogue: () => import('./editors/dialogue.js'),
  cutscenes: () => import('./editors/cutscene.js'),
  enemies: () => import('./editors/enemy.js'),
  preview: () => import('./views/preview.js'),
  battle: () => import('./editors/battle.js'),
  data: () => import('./views/datalists.js'),
  search: () => import('./views/search.js'),
  mods: () => import('./views/mods.js'),
  export: () => import('./views/export.js'),
};

let dirtyCheck = null;
let disposeView = null;
let lastHash = location.hash;
let ignoreHash = false;

/** Views call this with a function that says whether there are unsaved changes. */
export function setDirtyCheck(fn) {
  dirtyCheck = fn;
}

export function go(hash) {
  location.hash = hash;
}

function parseHash() {
  const raw = location.hash.replace(/^#\/?/, '');
  const [path, query = ''] = raw.split('?');
  const parts = path.split('/').filter(Boolean).map(decodeURIComponent);
  return { parts, query: Object.fromEntries(new URLSearchParams(query)) };
}

/** Maps a hash to {view, ch, params}. */
function route() {
  const { parts, query } = parseHash();
  if (!parts.length) return { view: 'home', query };
  if (parts[0] === 'mods' || parts[0] === 'export') return { view: parts[0], query };
  const [ch, section, a, b] = parts;
  if (!section) return { view: 'overview', ch, query };
  if (section === 'assets') return { view: 'assets', ch, type: a, name: b, query };
  if (ASSET_LABELS[section]) return { view: 'assets', ch, type: section, name: a, query };
  return { view: section, ch, arg: a, query };
}

async function loadChapters() {
  try {
    state.chapters = await api.get('/api/chapters');
  } catch (e) {
    state.chapters = [];
    if (e.status !== 409) toast(errMsg(e), 'error');
  }
  const sel = clear(document.getElementById('chapter-select'));
  sel.append(h('option', { value: '' }, state.chapters.length ? 'Select chapter' : 'No game set'));
  for (const c of state.chapters) {
    sel.append(h('option', { value: c.id, disabled: !c.extracted }, `${c.id}${c.extracted ? '' : ' (not extracted)'}`));
  }
  sel.value = state.ch || '';
}

export async function refreshChapters() {
  await loadChapters();
  if (state.ch) await loadCounts();
  renderNav();
}

async function loadCounts() {
  try {
    const list = await chapterApi(state.ch).counts();
    state.counts = Object.fromEntries(list.map((x) => [x.slug, x.count]));
  } catch {
    state.counts = {};
  }
}

function navLink(href, label, extra) {
  const active = location.hash === href || location.hash.startsWith(href + '/') || location.hash.startsWith(href + '?');
  return h('a', { href, class: active ? 'active' : '' }, label, extra ? h('span', { class: 'count' }, extra) : null);
}

function renderNav() {
  const nav = clear(document.getElementById('nav'));
  nav.append(navLink('#/', 'Setup'), navLink('#/mods', 'Mods and backups'), navLink('#/export', 'Export mod'));
  if (!state.ch) return;
  const c = `#/${encodeURIComponent(state.ch)}`;
  add(nav,
    h('div', { class: 'nav-head' }, state.ch),
    navLink(c, 'Overview and build'),
    h('div', { class: 'nav-head' }, 'Editors'),
    navLink(`${c}/rooms`, 'Rooms and triggers', state.counts.rooms),
    navLink(`${c}/dialogue`, 'Dialogue'),
    navLink(`${c}/cutscenes`, 'Cutscenes'),
    navLink(`${c}/sprites`, 'Sprites', state.counts.sprites),
    navLink(`${c}/enemies`, 'Enemies and encounters'),
    navLink(`${c}/shaders`, 'Shaders', state.counts.shaders),
    h('div', { class: 'nav-head' }, 'Test'),
    navLink(`${c}/preview`, 'Room preview'),
    navLink(`${c}/battle`, 'Bullet patterns and battle box'),
    h('div', { class: 'nav-head' }, 'All assets'),
    Object.entries(ASSET_LABELS).map(([slug, label]) => navLink(`${c}/assets/${slug}`, label, state.counts[slug])),
    h('div', { class: 'nav-head' }, 'Tools'),
    navLink(`${c}/data`, 'Data lists (read-only)'),
    navLink(`${c}/search`, 'Search code'),
  );
}

async function render() {
  const r = route();
  if (r.ch && r.ch !== state.ch) {
    state.ch = r.ch;
    forgetDatalists();
    document.getElementById('chapter-select').value = r.ch;
    await loadCounts();
  }
  if (!r.ch && r.view === 'home') {
    // Keep the chapter for the nav.
  }
  document.getElementById('build-link').href = state.ch ? `#/${encodeURIComponent(state.ch)}` : '#/';
  renderNav();
  const main = clear(document.getElementById('main'));
  if (disposeView) { try { disposeView(); } catch { /* ignore */ } }
  disposeView = null;
  dirtyCheck = null;
  const load = views[r.view];
  if (!load) {
    main.append(h('p', null, `Unknown page: ${r.view}`));
    return;
  }
  try {
    const mod = await load();
    const ctx = { ...r, capi: r.ch ? chapterApi(r.ch) : null, state, labels: ASSET_LABELS };
    disposeView = (await mod.render(main, ctx)) || null;
  } catch (e) {
    console.error(e);
    main.append(h('div', { class: 'error-box' }, errMsg(e)));
  }
}

window.addEventListener('hashchange', async () => {
  if (ignoreHash) { ignoreHash = false; lastHash = location.hash; return; }
  if (dirtyCheck && dirtyCheck() && !(await confirmDialog('You have unsaved changes. Leave this page?', 'Leave'))) {
    ignoreHash = true;
    location.hash = lastHash;
    return;
  }
  lastHash = location.hash;
  render();
});

window.addEventListener('beforeunload', (e) => {
  if (dirtyCheck && dirtyCheck()) { e.preventDefault(); e.returnValue = ''; }
});

document.getElementById('chapter-select').addEventListener('change', (e) => {
  if (e.target.value) go(`#/${encodeURIComponent(e.target.value)}`);
});

// Job indicator in the top bar.
async function pollJobs() {
  try {
    const jobs = await api.get('/api/jobs');
    const running = jobs.filter((j) => j.status === 'running');
    document.getElementById('job-status').textContent = running.length ? `Running: ${running.map((j) => `${j.kind} ${j.chapter}`).join(', ')}` : '';
  } catch { /* server down */ }
}
setInterval(pollJobs, 2000);

(async () => {
  await loadChapters();
  const first = route();
  if (!first.ch) {
    const ex = state.chapters.find((c) => c.extracted);
    if (ex && !location.hash) state.ch = ex.id;
  }
  render();
})();
