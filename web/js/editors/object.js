// Object editor: properties, sprite preview, and the code of each event inline.
import { h, attempt, assetDatalist, toast, selectField, modal, add } from '../ui.js';
import { jsonForm } from './jsonform.js';
import { codeEditor } from './codeedit.js';

const CATEGORIES = ['Create', 'Destroy', 'Alarm', 'Step', 'Collision', 'Keyboard', 'Mouse', 'Other', 'Draw', 'KeyPress', 'KeyRelease', 'CleanUp', 'PreCreate'];
const SUBTYPES = {
  Step: ['Step', 'BeginStep', 'EndStep'],
  Draw: ['Draw', 'DrawGUI', 'Resize', 'DrawBegin', 'DrawEnd', 'DrawGUIBegin', 'DrawGUIEnd', 'PreDraw', 'PostDraw'],
  Alarm: Array.from({ length: 12 }, (_, i) => String(i)),
  Other: ['OutsideRoom', 'IntersectBoundary', 'GameStart', 'GameEnd', 'RoomStart', 'RoomEnd', 'AnimationEnd', 'EndOfPath', ...Array.from({ length: 16 }, (_, i) => `User${i}`)],
};
// Number used in code entry names (gml_Object_<obj>_<Category>_<n>).
const SUB_NUM = { Step: 0, BeginStep: 1, EndStep: 2, Draw: 0, DrawGUI: 64, Resize: 65, DrawBegin: 72, DrawEnd: 73, DrawGUIBegin: 74, DrawGUIEnd: 75, PreDraw: 76, PostDraw: 77,
  OutsideRoom: 0, IntersectBoundary: 1, GameStart: 2, GameEnd: 3, RoomStart: 4, RoomEnd: 5, AnimationEnd: 7, EndOfPath: 8 };
for (let i = 0; i < 16; i++) SUB_NUM[`User${i}`] = 10 + i;

export function eventCodeName(obj, cat, sub) {
  let n = 0;
  if (cat === 'Alarm') n = +sub || 0;
  else if (cat === 'Collision') n = sub;
  else if (sub && SUB_NUM[sub] !== undefined) n = SUB_NUM[sub];
  return `gml_Object_${obj}_${cat}_${n}`;
}

export async function open(el, ectx) {
  const { capi, ch, name, asset, setDirty } = ectx;
  const data = structuredClone(asset.data);
  const dirtyCode = new Map(); // code name -> editor
  const c = encodeURIComponent(ch);

  const preview = h('div', { class: 'row' });
  const showSprite = () => {
    preview.replaceChildren();
    if (data.Sprite) preview.append(h('img', { class: 'thumb', style: { maxHeight: '96px' }, src: capi.file('sprites', data.Sprite, `${data.Sprite}_0.png`) }),
      h('a', { href: `#/${c}/assets/sprites/${encodeURIComponent(data.Sprite)}` }, `Edit ${data.Sprite}`));
    else preview.append(h('span', { class: 'dim' }, 'No sprite.'));
  };
  showSprite();

  const props = { ...data };
  delete props.Events;
  const form = jsonForm(props, () => { Object.assign(data, props); showSprite(); setDirty(true); }, ectx);

  const events = h('div');
  const drawEvents = () => {
    events.replaceChildren();
    data.Events.forEach((ev, i) => {
      const box = h('div', { class: 'card' });
      const head = h('div', { class: 'row' },
        h('b', null, `${ev.Category}${ev.Subtype ? ` · ${ev.Subtype}` : ''}`),
        ev.CodeEntry ? h('a', { class: 'mono small', href: `#/${c}/assets/code/${encodeURIComponent(ev.CodeEntry)}` }, ev.CodeEntry) : h('span', { class: 'dim' }, 'no code'),
        h('div', { class: 'grow' }),
        h('button', { class: 'small danger', onclick: () => { data.Events.splice(i, 1); setDirty(true); drawEvents(); } }, 'Remove event'));
      const body = h('div');
      const det = h('details', null, h('summary', null, 'Code'), body);
      let loaded = false;
      det.addEventListener('toggle', async () => {
        if (!det.open || loaded || !ev.CodeEntry) return;
        loaded = true;
        const code = await capi.read('code', ev.CodeEntry).catch(() => ({ source: '' }));
        const ed = codeEditor({ value: code.source, height: '40vh', onChange: () => { dirtyCode.set(ev.CodeEntry, ed); setDirty(true); }, onSave: ectx.save });
        body.append(ed.el);
      });
      box.append(head, det);
      events.append(box);
    });
  };
  drawEvents();

  const addEvent = async () => {
    const ev = { Category: 'Step', Subtype: 'Step' };
    const subWrap = h('div');
    const drawSub = () => {
      subWrap.replaceChildren();
      if (ev.Category === 'Collision') {
        ev.Subtype = ev.Subtype || '';
        subWrap.append(h('label', { class: 'field' }, h('span', null, 'Other object'),
          h('input', { type: 'text', list: assetDatalist(capi, ch, 'objects'), oninput: (e) => { ev.Subtype = e.target.value; } })));
      } else if (SUBTYPES[ev.Category]) {
        ev.Subtype = SUBTYPES[ev.Category][0];
        subWrap.append(selectField('Subtype', ev, 'Subtype', SUBTYPES[ev.Category]));
      } else {
        ev.Subtype = '';
      }
    };
    drawSub();
    const ok = await modal('Add event', h('div', { class: 'col' }, selectField('Category', ev, 'Category', CATEGORIES, drawSub), subWrap),
      [{ label: 'Cancel', value: false }, { label: 'Add', value: true, primary: true }]);
    if (!ok) return;
    const codeName = eventCodeName(name, ev.Category, ev.Subtype);
    if (data.Events.some((e) => e.CodeEntry === codeName)) { toast('This event exists.', 'error'); return; }
    if (!(await capi.read('code', codeName).then(() => true, () => false))) {
      if (!(await attempt(() => capi.write('code', codeName, { source: '' })))) return;
    }
    data.Events.push({ Category: ev.Category, Subtype: ev.Subtype, CodeEntry: codeName });
    setDirty(true);
    drawEvents();
  };

  add(el,
    h('div', { class: 'card' }, h('h3', null, 'Sprite'), preview),
    h('div', { class: 'card' }, h('h3', null, 'Properties'), form),
    h('div', { class: 'row' }, h('h3', null, `Events (${data.Events.length})`), h('button', { class: 'small', onclick: addEvent }, '+ Add event'),
      h('a', { class: 'small', href: `#/${c}/search?q=${encodeURIComponent(name)}` }, 'Find uses in code')),
    events);

  return {
    save: async () => {
      for (const [code, ed] of dirtyCode) await capi.write('code', code, { source: ed.value });
      dirtyCode.clear();
      const r = await capi.write('objects', name, { data });
      asset.data = r.data;
    },
  };
}

