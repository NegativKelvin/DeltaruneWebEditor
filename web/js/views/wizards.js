// Templates and wizards: make all assets and code for a new NPC, shop, enemy, room, or cutscene trigger.
// Preview shows every asset the server will write (dry run). Create writes them into the mod (all or nothing).
import { h, attempt, toast, assetDatalist, tabs, confirmDialog } from '../ui.js';
import { refreshChapters } from '../app.js';
import { parseCall } from '../editors/cutscenecore.js';

const PLACE = [
  { key: 'place.room', label: 'Put in room (empty: do not)', list: 'rooms' },
  { key: 'place.x', label: 'X', type: 'num' },
  { key: 'place.y', label: 'Y', type: 'num' },
];

const KINDS = {
  npc: {
    label: 'NPC',
    help: 'An object that talks when Kris presses the confirm key near it. The text shows in the Dialogue editor and in the room preview (Z).',
    defaults: () => ({ object: 'obj_npc_', sprite: '', parent: '', solid: false, speaker: '', lines: '* Hello^1, Kris.\n* Nice day^1, right?', range: 24, place: { room: '', x: 0, y: 0 } }),
    fields: [
      { key: 'object', label: 'Object name' }, { key: 'sprite', label: 'Sprite', list: 'sprites' }, { key: 'parent', label: 'Parent object (optional)', list: 'objects' },
      { key: 'solid', label: 'Solid', type: 'bool' }, { key: 'speaker', label: 'Speaker (scr_speaker: susie, ralsei, ... empty: none)' },
      { key: 'lines', label: 'Text: one box per line (& = new line in a box)', type: 'area' }, { key: 'range', label: 'Talk distance (px)', type: 'num' }, ...PLACE,
    ],
  },
  shop: {
    label: 'Shop',
    help: 'A shopkeeper object: a choice box with up to 3 items and Leave, a gold check, and scr_itemget. It is a simple shop, not a full Deltarune shop room.',
    defaults: () => ({ object: 'obj_shop_', sprite: '', speaker: '', items: [{ name: 'Dark Candy', item: 1, price: 25 }], greeting: '', bought: '', poor: '', full: '', bye: '', range: 24, place: { room: '', x: 0, y: 0 } }),
    fields: [
      { key: 'object', label: 'Object name' }, { key: 'sprite', label: 'Sprite', list: 'sprites' }, { key: 'speaker', label: 'Speaker' },
      { key: 'items', label: 'Items (item = number for scr_itemget)', type: 'rows', max: 3, cols: [{ key: 'name', label: 'Name' }, { key: 'item', label: 'Item no.', type: 'num' }, { key: 'price', label: 'Price', type: 'num' }], add: () => ({ name: 'Item', item: 1, price: 10 }) },
      { key: 'greeting', label: 'Greeting (empty: default)' }, { key: 'bought', label: 'After buying' }, { key: 'poor', label: 'Not enough money' },
      { key: 'full', label: 'Inventory full' }, { key: 'bye', label: 'Leave' }, { key: 'range', label: 'Talk distance (px)', type: 'num' }, ...PLACE,
    ],
  },
  enemy: {
    label: 'Enemy',
    help: 'An enemy object, a new case in the monster setup switch (stats), and (optional) a new encounter case. The enemy makes the bullet pattern wave on its turn.',
    defaults: () => ({ object: 'obj_dre_enemy_', sprite: '', parent: 'obj_monsterparent', name: 'Enemy', hp: 100, at: 5, df: 0, exp: 10, gold: 10, wave: '', turn_frames: 0,
      encounter: true, x: 420, y: 160, battle_text: '', setup_code: '', enemy_type: '', encounter_code: '', encounter_no: '' }),
    fields: [
      { key: 'object', label: 'Object name' }, { key: 'sprite', label: 'Sprite', list: 'sprites' }, { key: 'parent', label: 'Parent object', list: 'objects' },
      { key: 'name', label: 'Name in battle' }, { key: 'hp', label: 'HP', type: 'num' }, { key: 'at', label: 'Attack', type: 'num' }, { key: 'df', label: 'Defense', type: 'num' },
      { key: 'exp', label: 'EXP', type: 'num' }, { key: 'gold', label: 'Gold (D$)', type: 'num' }, { key: 'wave', label: 'Bullet pattern', type: 'pattern' },
      { key: 'turn_frames', label: 'Turn length (0: pattern duration)', type: 'num' }, { key: 'encounter', label: 'Add an encounter', type: 'bool' },
      { key: 'x', label: 'Battle X', type: 'num' }, { key: 'y', label: 'Battle Y', type: 'num' }, { key: 'battle_text', label: 'Battle text (empty: "* <name> appears.")' },
      { key: 'setup_code', label: 'Stats code (empty: find it)', list: 'code' }, { key: 'enemy_type', label: 'Enemy type no. (empty: next free)', type: 'num', empty: true },
      { key: 'encounter_code', label: 'Encounter code (empty: find it)', list: 'code' }, { key: 'encounter_no', label: 'Encounter no. (empty: next free)', type: 'num', empty: true },
    ],
  },
  room: {
    label: 'Room',
    help: 'A new room with the layout (views, layers, settings) of a template room, no instances, entrance markers, and doors. A door sends Kris to the marker with the same letter in the target room.',
    defaults: () => ({ name: 'room_dre_', template: '', width: '', height: '', markers: [{ letter: 'A', x: 160, y: 120 }], doors: [], creation_code: '' }),
    fields: [
      { key: 'name', label: 'Room name' }, { key: 'template', label: 'Copy the layout of (empty: first room)', list: 'rooms' },
      { key: 'width', label: 'Width (empty: template)', type: 'num', empty: true }, { key: 'height', label: 'Height (empty: template)', type: 'num', empty: true },
      { key: 'markers', label: 'Entrance markers (obj_markerA ...)', type: 'rows', cols: [{ key: 'letter', label: 'Letter' }, { key: 'x', label: 'X', type: 'num' }, { key: 'y', label: 'Y', type: 'num' }], add: () => ({ letter: 'B', x: 0, y: 0 }) },
      { key: 'doors', label: 'Doors (obj_dre_doorA ...)', type: 'rows', cols: [{ key: 'letter', label: 'Letter' }, { key: 'x', label: 'X', type: 'num' }, { key: 'y', label: 'Y', type: 'num' },
        { key: 'w', label: 'W', type: 'num' }, { key: 'h', label: 'H', type: 'num' }, { key: 'target', label: 'Target room', list: 'rooms' }], add: () => ({ letter: 'A', x: 0, y: 0, w: 40, h: 40, target: '' }) },
      { key: 'creation_code', label: 'Room creation code (GML, optional)', type: 'area' },
    ],
  },
  cutscene: {
    label: 'Cutscene',
    help: 'A trigger object that runs a cutscene (scr_cutscene_make, c_* steps, c_terminatekillactors). Edit the steps later in the visual cutscene editor.',
    defaults: () => ({ object: 'obj_dre_cs_', trigger: 'touch', w: 40, h: 40, flag: '', steps: 'c_sel(kr);\nc_facing("d");\nc_wait(30);', place: { room: '', x: 0, y: 0 } }),
    fields: [
      { key: 'object', label: 'Object name' }, { key: 'trigger', label: 'Start', type: 'select', options: ['touch', 'talk', 'auto'] },
      { key: 'w', label: 'Touch box W', type: 'num' }, { key: 'h', label: 'Touch box H', type: 'num' },
      { key: 'flag', label: 'Run once: global.flag index (empty: every time)', type: 'num', empty: true },
      { key: 'steps', label: 'Steps: one c_* call per line', type: 'area' }, ...PLACE,
    ],
  },
};

const getPath = (o, k) => k.split('.').reduce((x, p) => x?.[p], o);
const setPath = (o, k, v) => { const ps = k.split('.'); const last = ps.pop(); ps.reduce((x, p) => (x[p] ||= {}), o)[last] = v; };

/** Form values -> wizard parameters. */
function toParams(kind, v) {
  const p = structuredClone(v);
  for (const f of KINDS[kind].fields) {
    if (f.empty && (getPath(p, f.key) === '' || getPath(p, f.key) === null)) setPath(p, f.key, null);
  }
  if ('sprite' in p && !p.sprite) p.sprite = null;
  if (kind === 'npc') {
    if (!p.parent) p.parent = null;
    p.lines = p.lines.split('\n').filter((l) => l.trim());
  }
  if (kind === 'cutscene') {
    const bad = [];
    p.steps = p.steps.split('\n').filter((l) => l.trim()).map((l) => parseCall(l) || (bad.push(l.trim()), null)).filter(Boolean);
    if (bad.length) throw new Error(`Not a c_* call: ${bad.join(' | ')}`);
  }
  if (kind === 'room') for (const r of [...p.markers, ...p.doors]) r.letter = String(r.letter).trim().toUpperCase();
  return p;
}

export async function render(main, ctx) {
  const { capi, ch, query } = ctx;
  const c = encodeURIComponent(ch);
  const page = h('div', { class: 'page' });
  main.append(page);
  const patterns = await capi.patterns().catch(() => []);
  const values = Object.fromEntries(Object.keys(KINDS).map((k) => [k, KINDS[k].defaults()]));
  page.append(h('h2', null, 'Templates and wizards'),
    h('p', { class: 'dim small' }, 'Each wizard writes new objects, code, and room changes into the mod. Preview first: it shows every asset and its code. ',
      'New assets must not exist yet. Build the chapter after you create. The made GML uses Deltarune names (global.interact, msgsetloc, obj_dialoguer, scr_cutscene_make, ...): test it in the game.'));
  const t = tabs(Object.entries(KINDS).map(([kind, k]) => ({ id: kind, label: k.label, render: (el) => form(el, kind) })), KINDS[query.kind] ? query.kind : 'npc');
  page.append(t.el);

  function input(f, obj, key) {
    const val = getPath(obj, key);
    const set = (v) => setPath(obj, key, v);
    const list = f.list === 'code' ? assetDatalist(capi, ch, 'code') : f.list ? assetDatalist(capi, ch, f.list) : undefined;
    if (f.type === 'bool') return h('input', { type: 'checkbox', checked: !!val, onchange: (e) => set(e.target.checked) });
    if (f.type === 'area') return h('textarea', { rows: 5, style: { width: '100%' }, value: val, oninput: (e) => set(e.target.value) });
    if (f.type === 'select') return h('select', { onchange: (e) => set(e.target.value) }, f.options.map((o) => h('option', { value: o, selected: o === val }, o)));
    if (f.type === 'pattern') {
      return h('select', { onchange: (e) => set(e.target.value) }, h('option', { value: '' }, '(none)'),
        patterns.map((p) => h('option', { value: p.name, selected: p.name === val }, `${p.name} (${p.duration} steps)`)));
    }
    if (f.type === 'num') {
      return h('input', { type: 'number', value: val ?? '', oninput: (e) => set(e.target.value === '' ? (f.empty ? '' : 0) : Number(e.target.value)) });
    }
    return h('input', { type: 'text', value: val ?? '', list, size: 24, oninput: (e) => set(e.target.value) });
  }

  function rows(el, f, v) {
    const arr = getPath(v, f.key);
    el.replaceChildren(...arr.map((r, i) => h('div', { class: 'row' },
      f.cols.map((col) => h('label', { class: 'field' }, h('span', null, col.label), input(col, r, col.key))),
      h('button', { class: 'small danger', onclick: () => { arr.splice(i, 1); rows(el, f, v); } }, '×'))),
    (!f.max || arr.length < f.max) ? h('button', { class: 'small', onclick: () => { arr.push(f.add()); rows(el, f, v); } }, '+ Add') : null);
  }

  function form(el, kind) {
    const k = KINDS[kind];
    const v = values[kind];
    const out = h('div', { class: 'wiz-plan' });
    const fieldsEl = h('div', { class: 'fields' });
    for (const f of k.fields) {
      if (f.type === 'rows') {
        const box = h('div', { class: 'wiz-rows' });
        rows(box, f, v);
        fieldsEl.append(h('div', { class: 'field', style: { flexBasis: '100%' } }, h('span', null, f.label), box));
      } else if (f.type === 'area') {
        fieldsEl.append(h('label', { class: 'field', style: { flexBasis: '100%' } }, h('span', null, f.label), input(f, v, f.key)));
      } else if (f.type === 'bool') {
        fieldsEl.append(h('label', { class: 'field check' }, input(f, v, f.key), h('span', null, f.label)));
      } else {
        fieldsEl.append(h('label', { class: 'field' }, h('span', null, f.label), input(f, v, f.key)));
      }
    }
    const run = async (dry) => {
      let params;
      try { params = toParams(kind, v); } catch (e) { toast(e.message, 'error'); return; }
      if (!dry && !(await confirmDialog(`Create the ${k.label.toLowerCase()} in the mod?`, 'Create'))) return;
      const r = await attempt(() => capi.wizard(kind, params, dry), dry ? null : 'Created');
      if (!r) return;
      showPlan(out, r);
      if (!dry) await refreshChapters();
    };
    el.append(h('div', { class: 'card' }, h('p', { class: 'small' }, k.help), fieldsEl,
      h('div', { class: 'row', style: { marginTop: '10px' } }, h('button', { onclick: () => run(true) }, 'Preview'), h('button', { class: 'primary', onclick: () => run(false) }, 'Create'))), out);
  }

  function showPlan(out, r) {
    const link = (w) => {
      if (w.type === 'code' && /_Step_0$/.test(w.name) && /\bc_\w+\(/.test(w.source || '')) return h('a', { href: `#/${c}/cutscenes/${encodeURIComponent(w.name)}` }, 'cutscene editor');
      return h('a', { href: `#/${c}/assets/${w.type}/${encodeURIComponent(w.name)}` }, 'open');
    };
    out.replaceChildren(h('div', { class: 'card' },
      h('h3', null, r.applied ? `Written: ${r.writes.length} assets` : `Preview: ${r.writes.length} assets (nothing written yet)`),
      r.notes.length ? h('ul', { class: 'small' }, r.notes.map((n) => h('li', null, n))) : null,
      r.writes.map((w) => h('div', { class: `w ${w.new ? 'new' : 'change'}` },
        h('div', { class: 'row' }, h('span', { class: `badge ${w.new ? 'new' : 'mod'}` }, w.new ? 'new' : 'change'), h('span', { class: 'mono' }, `${w.type}/${w.name}`), r.applied ? link(w) : null),
        h('details', null, h('summary', { class: 'small dim' }, w.source !== undefined ? 'GML' : 'JSON'),
          h('pre', { class: 'gml-out' }, w.source !== undefined ? w.source : JSON.stringify(w.data, null, 2)))))));
  }
}
