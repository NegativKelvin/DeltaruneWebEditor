// Asset browser: list of one type + editor for the selected asset.
// Every type gets: editor (dedicated or generic form), raw JSON, side files, revert, duplicate.
import { h, attempt, toast, confirmDialog, promptDialog, debounce, download, pickFiles, forgetImages, add } from '../ui.js';
import { setDirtyCheck, go, refreshChapters } from '../app.js';
import { jsonForm } from '../editors/jsonform.js';
import { codeEditor } from '../editors/codeedit.js';

const EDITORS = {
  rooms: () => import('../editors/room.js'),
  sprites: () => import('../editors/sprite.js'),
  shaders: () => import('../editors/shader.js'),
  code: () => import('../editors/code.js'),
  objects: () => import('../editors/object.js'),
  backgrounds: () => import('../editors/media.js'),
  sounds: () => import('../editors/media.js'),
  fonts: () => import('../editors/media.js'),
  paths: () => import('../editors/path.js'),
};

const NAME_RE = /^[A-Za-z0-9_][A-Za-z0-9_.-]*$/;
const PAGE = 300;

export async function render(main, ctx) {
  const { capi, ch, type, name, labels } = ctx;
  if (!labels[type]) { main.append(h('p', { class: 'page' }, `Unknown asset type: ${type}`)); return; }
  const split = h('div', { class: 'split' });
  main.append(split);

  // ---- List pane ----
  const search = h('input', { type: 'search', placeholder: `Filter ${labels[type].toLowerCase()}`, value: ctx.query.q || '' });
  const onlyChanged = h('input', { type: 'checkbox' });
  const items = h('div', { class: 'items' });
  const info = h('span', { class: 'dim small' });
  let offset = 0;
  const loadList = async (append = false) => {
    if (!append) offset = 0;
    const r = await attempt(() => capi.list(type, { q: search.value, offset, limit: PAGE }));
    if (!r) return;
    if (!append) items.replaceChildren();
    for (const it of r.items) {
      if (onlyChanged.checked && !it.modified) continue;
      items.append(h('a', { href: `#/${encodeURIComponent(ch)}/assets/${type}/${encodeURIComponent(it.name)}`, class: it.name === name ? 'active' : '', title: it.name },
        h('span', { style: { overflow: 'hidden', textOverflow: 'ellipsis' } }, it.name),
        it.added ? h('span', { class: 'badge new' }, 'new') : it.modified ? h('span', { class: 'badge mod' }, 'mod') : null));
    }
    offset += r.items.length;
    info.textContent = `${r.total} total`;
    items.querySelector('.more')?.remove();
    if (offset < r.total) items.append(h('a', { class: 'more', href: 'javascript:void 0', onclick: () => loadList(true) }, `Show more (${r.total - offset})`));
    if (!append) items.querySelector('a.active')?.scrollIntoView({ block: 'center' });
  };
  search.addEventListener('input', debounce(() => loadList(), 250));
  onlyChanged.addEventListener('change', () => loadList());
  split.append(h('div', { class: 'list-pane' },
    h('div', { class: 'list-head' },
      h('div', { class: 'row' }, h('b', { class: 'grow' }, labels[type]), h('button', { class: 'small', onclick: () => newAsset(ctx) }, '+ New')),
      search,
      h('label', { class: 'field check' }, onlyChanged, h('span', null, 'Only changed')),
      info),
    items));
  loadList();

  // ---- Editor pane ----
  const pane = h('div', { class: 'editor-pane' });
  split.append(pane);
  if (!name) {
    pane.append(h('div', { class: 'editor-body dim' }, `Select an asset on the left, or make a new one.`));
    return;
  }
  let asset;
  try {
    asset = await capi.read(type, name);
  } catch (e) {
    pane.append(h('div', { class: 'error-box' }, e.message));
    return;
  }
  return openEditor(pane, { ...ctx, asset, reloadList: loadList });
}

async function openEditor(pane, ctx) {
  const { capi, ch, type, name, asset } = ctx;
  let editor = null;
  let dirty = false;
  const saveBtn = h('button', { class: 'primary', disabled: true, onclick: () => save() }, 'Save');
  const setDirty = (d = true) => { dirty = d; saveBtn.disabled = !d; saveBtn.textContent = d ? 'Save *' : 'Save'; };
  setDirtyCheck(() => dirty);

  const save = async () => {
    if (!editor?.save) return;
    const ok = await attempt(async () => { await editor.save(); return true; }, 'Saved');
    if (ok) {
      setDirty(false);
      badge.replaceChildren(h('span', { class: 'badge mod' }, 'changed'));
      ctx.reloadList();
    }
  };
  const onKey = (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 's') { e.preventDefault(); if (dirty) save(); }
  };
  document.addEventListener('keydown', onKey);

  const badge = h('span', null, asset.modified ? h('span', { class: 'badge mod' }, 'changed') : h('span', { class: 'badge' }, 'original'));
  const head = h('div', { class: 'editor-head' },
    h('h2', null, ctx.name), badge, h('div', { class: 'grow' }),
    saveBtn,
    h('button', { onclick: () => duplicate(ctx) }, 'Duplicate'),
    h('button', { onclick: () => download(new Blob([JSON.stringify(asset.data, null, 2)], { type: 'application/json' }), `${name}.json`) }, 'Download JSON'),
    asset.modified ? h('button', { class: 'danger', onclick: async () => {
      if (!(await confirmDialog(`Revert ${type}/${name} to the original? Your changes to it are lost.`, 'Revert'))) return;
      if (await attempt(() => capi.revert(type, name), 'Reverted')) {
        setDirty(false);
        forgetImages(capi.asset(type, name));
        await refreshChapters();
        go(`#/${encodeURIComponent(ch)}/assets/${type}${(await capi.read(type, name).catch(() => null)) ? `/${encodeURIComponent(name)}` : ''}?r=${Date.now()}`);
      }
    } }, 'Revert') : null);
  const body = h('div', { class: 'editor-body' });
  pane.append(head, body);

  const ectx = { ...ctx, setDirty, save };
  const panes = [];
  const loader = EDITORS[type];
  const tabsHead = h('div', { class: 'tabs' });
  const tabBody = h('div');
  body.append(tabsHead, tabBody);
  const addTab = (id, label, renderFn) => {
    const el = h('div', { hidden: true });
    tabBody.append(el);
    let done = false;
    const btn = h('button', { onclick: () => select(id) }, label);
    tabsHead.append(btn);
    panes.push({ id, el, btn, show: async () => { if (!done) { done = true; await renderFn(el); } } });
  };
  const select = async (id) => {
    for (const p of panes) { p.el.hidden = p.id !== id; p.btn.classList.toggle('active', p.id === id); }
    await panes.find((p) => p.id === id).show();
  };

  addTab('editor', 'Editor', async (el) => {
    const mod = loader ? await loader() : null;
    if (mod) {
      editor = await mod.open(el, ectx);
    } else {
      editor = genericEditor(el, ectx);
    }
  });
  addTab('json', 'Raw JSON', (el) => rawJson(el, ectx));
  if (asset.files.length || ['sprites', 'backgrounds', 'sounds', 'fonts', 'shaders'].includes(type)) addTab('files', `Files (${asset.files.length})`, (el) => filesTab(el, ectx));
  await select(ctx.query.tab || 'editor');

  return () => {
    document.removeEventListener('keydown', onKey);
    editor?.dispose?.();
  };
}

/** Generic form editor for any type. */
export function genericEditor(el, ectx) {
  const { capi, type, name, asset, setDirty } = ectx;
  const data = structuredClone(asset.data);
  add(el,
    h('p', { class: 'dim small' }, `All fields of ${data.$type || type} in UTMT project format. Empty reference fields mean null.`),
    jsonForm(data, () => setDirty(true), ectx));
  return { save: () => capi.write(type, name, { data }).then((r) => { asset.data = r.data; }) };
}

function rawJson(el, ectx) {
  const { capi, type, name, asset } = ectx;
  const ed = codeEditor({ value: JSON.stringify(asset.data, null, 2), lang: 'json', height: '65vh' });
  const msg = h('span', { class: 'dim small' });
  el.append(h('div', { class: 'row' },
    h('button', { class: 'primary', onclick: async () => {
      let data;
      try { data = JSON.parse(ed.value); } catch (e) { msg.textContent = `JSON error: ${e.message}`; return; }
      if (await attempt(() => capi.write(type, name, { data }), 'Saved raw JSON')) {
        go(`#/${encodeURIComponent(ectx.ch)}/assets/${type}/${encodeURIComponent(name)}?tab=json&r=${Date.now()}`);
      }
    } }, 'Save raw JSON'), msg,
    h('span', { class: 'dim small' }, 'This saves the JSON directly, and the Editor tab reloads.')), ed.el);
}

function filesTab(el, ectx) {
  const { capi, type, name, asset } = ectx;
  const reload = () => go(`#/${encodeURIComponent(ectx.ch)}/assets/${type}/${encodeURIComponent(name)}?tab=files&r=${Date.now()}`);
  const table = h('table', null, h('tr', null, h('th', null, 'File'), h('th', null, 'Preview'), h('th', null, '')));
  for (const f of asset.files) {
    const url = capi.file(type, name, f);
    let preview = null;
    if (/\.png$/i.test(f)) preview = h('img', { class: 'thumb', src: `${url}?t=${Date.now()}`, style: { maxHeight: '64px' } });
    else if (/\.(ogg|wav|mp3)$/i.test(f)) preview = h('audio', { controls: true, preload: 'none', src: url });
    table.append(h('tr', null,
      h('td', { class: 'mono small' }, f),
      h('td', null, preview),
      h('td', null, h('div', { class: 'row' },
        h('a', { class: 'button small', href: url, download: f }, 'Download'),
        h('button', { class: 'small', onclick: async () => {
          const [file] = await pickFiles();
          if (file && await attempt(() => capi.writeFile(type, name, f, file), `Replaced ${f}`)) { forgetImages(capi.asset(type, name)); reload(); }
        } }, 'Replace'),
        h('button', { class: 'small danger', onclick: async () => {
          if (await confirmDialog(`Remove ${f} from the mod copy of ${name}?`, 'Remove') && await attempt(() => capi.deleteFile(type, name, f), 'Removed')) reload();
        } }, 'Remove')))));
  }
  el.append(table, h('div', { class: 'row', style: { marginTop: '8px' } }, h('button', { onclick: async () => {
    const files = await pickFiles('', true);
    for (const file of files) {
      if (type !== 'sprites' && type !== 'backgrounds' && type !== 'sounds' && type !== 'fonts' && type !== 'shaders' && !file.name.startsWith(name + '.')) {
        toast(`Files of ${type} must be named ${name}.<ext>`, 'error');
        continue;
      }
      await attempt(() => capi.writeFile(type, name, file.name, file), `Added ${file.name}`);
    }
    if (files.length) reload();
  } }, 'Add files'), h('span', { class: 'dim small' }, 'The file name is kept. UTMT reads files by name (for example spr_x_0.png, GLSL_ES_Fragment.txt).')));
}

// ---- New / duplicate ----

const TEMPLATES = {
  objects: () => ({ $type: 'GameObject', Sprite: null, Visible: true, Solid: false, Depth: 0, Persistent: false, ParentObject: null, MaskSprite: null,
    UsesPhysics: false, IsSensor: false, CollisionShape: 'Circle', Density: 0.5, Restitution: 0.1, Group: 0, LinearDamping: 0.1, AngularDamping: 0.1,
    Friction: 0.2, Awake: false, Kinematic: false, PhysicsVertices: [], Events: [] }),
  sprites: () => ({ $type: 'Sprite', Width: 32, Height: 32, MarginLeft: 0, MarginRight: 31, MarginBottom: 31, MarginTop: 0, Transparent: false,
    Smooth: false, Preload: false, BBoxMode: 0, CollisionKind: 'AxisAlignedRect', OriginX: 0, OriginY: 0, TextureCount: 1, MaskCount: 1, Sequence: null }),
  shaders: () => ({ $type: 'Shader', Type: 'GLSL_ES', Version: 2, VertexShaderAttributes: ['in_Position', 'in_Colour', 'in_TextureCoord'] }),
  paths: () => ({ $type: 'Path', IsSmooth: false, IsClosed: false, Precision: 4, Points: [] }),
};

const DEFAULT_VSH = `attribute vec3 in_Position;
attribute vec4 in_Colour;
attribute vec2 in_TextureCoord;
varying vec2 v_vTexcoord;
varying vec4 v_vColour;
void main()
{
    gl_Position = gm_Matrices[MATRIX_WORLD_VIEW_PROJECTION] * vec4(in_Position, 1.0);
    v_vColour = in_Colour;
    v_vTexcoord = in_TextureCoord;
}
`;
const DEFAULT_FSH = `varying vec2 v_vTexcoord;
varying vec4 v_vColour;
void main()
{
    gl_FragColor = v_vColour * texture2D(gm_BaseTexture, v_vTexcoord);
}
`;

async function blankPng(w, h2) {
  const c = new OffscreenCanvas(w, h2);
  c.getContext('2d');
  return c.convertToBlob({ type: 'image/png' });
}

async function newAsset(ctx) {
  const { capi, ch, type } = ctx;
  const prefix = { rooms: 'room_', objects: 'obj_', code: 'gml_Script_', scripts: 'scr_', sprites: 'spr_', backgrounds: 'bg_', sounds: 'snd_', fonts: 'fnt_', shaders: 'shd_', paths: 'path_', sequences: 'seq_', animcurves: 'curve_' }[type];
  const name = await promptDialog(`New ${type.replace(/s$/, '')} name`, prefix);
  if (!name) return;
  if (!NAME_RE.test(name)) { toast('Use letters, digits, and _ only.', 'error'); return; }
  const exists = await capi.read(type, name).then(() => true, () => false);
  if (exists) { toast(`${name} exists already.`, 'error'); return; }
  const ok = await attempt(async () => {
    if (type === 'code') {
      await capi.write('code', name, { source: '// New code\n' });
    } else if (type === 'scripts') {
      const other = (await capi.list('scripts', { limit: 1 })).items[0];
      const gms23 = other ? ((await capi.read('scripts', other.name)).data.Code || '').startsWith('gml_GlobalScript_') : true;
      const code = `${gms23 ? 'gml_GlobalScript_' : 'gml_Script_'}${name}`;
      await capi.write('code', code, { source: gms23 ? `function ${name}()\n{\n}\n` : `// ${name}\nreturn 0;\n` });
      await capi.write('scripts', name, { data: { $type: 'Script', Code: code } });
    } else if (type === 'rooms') {
      const rooms = await capi.list('rooms', { limit: 1 });
      if (!rooms.items.length) throw new Error('No room to copy the layout from.');
      const base = (await capi.read('rooms', rooms.items[0].name)).data;
      base.GameObjects = [];
      base.Tiles = [];
      base.CreationCodeEntry = null;
      base.InstanceCreationOrderIDs = base.InstanceCreationOrderIDs ? [] : null;
      for (const l of base.Layers || []) {
        if (l.InstanceIndices) l.InstanceIndices = [];
        if (l.TileData) l.TileData = l.TileData.map(() => 0);
        if (l.Sprites) l.Sprites = [];
        if (l.LegacyTiles) l.LegacyTiles = [];
      }
      await capi.write('rooms', name, { data: base });
    } else if (TEMPLATES[type]) {
      await capi.write(type, name, { data: TEMPLATES[type]() });
      if (type === 'sprites') {
        await capi.writeFile('sprites', name, `${name}_0.png`, await blankPng(32, 32));
        const c = new OffscreenCanvas(32, 32);
        const g = c.getContext('2d');
        g.fillStyle = '#fff';
        g.fillRect(0, 0, 32, 32);
        await capi.writeFile('sprites', name, `${name}_0.mask.png`, await c.convertToBlob({ type: 'image/png' }));
      }
      if (type === 'shaders') {
        await capi.writeFile('shaders', name, 'GLSL_ES_Vertex.txt', new Blob([DEFAULT_VSH]));
        await capi.writeFile('shaders', name, 'GLSL_ES_Fragment.txt', new Blob([DEFAULT_FSH]));
      }
    } else {
      throw new Error(`Use Duplicate on an existing ${type.replace(/s$/, '')} to make a new one.`);
    }
    return true;
  }, `Made ${name}`);
  if (ok) {
    await refreshChapters();
    go(`#/${encodeURIComponent(ch)}/assets/${type}/${encodeURIComponent(name)}`);
  }
}

async function duplicate(ctx) {
  const { capi, ch, type, name, asset } = ctx;
  const nn = await promptDialog(`Duplicate ${name} as`, `${name}_copy`);
  if (!nn || nn === name) return;
  if (!NAME_RE.test(nn)) { toast('Use letters, digits, and _ only.', 'error'); return; }
  if (await capi.read(type, nn).then(() => true, () => false)) { toast(`${nn} exists already.`, 'error'); return; }
  const ok = await attempt(async () => {
    if (type === 'code') {
      await capi.write('code', nn, { source: asset.source });
      return true;
    }
    await capi.write(type, nn, { data: structuredClone(asset.data) });
    for (const f of asset.files) {
      const nf = f.startsWith(name) ? nn + f.slice(name.length) : f;
      const blob = await (await capi.readFile(type, name, f)).blob();
      await capi.writeFile(type, nn, nf, blob);
    }
    return true;
  }, `Duplicated as ${nn}`);
  if (ok) {
    await refreshChapters();
    go(`#/${encodeURIComponent(ch)}/assets/${type}/${encodeURIComponent(nn)}`);
  }
}
