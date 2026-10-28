// Cutscene editor: shows runs of c_* calls in a code entry as steps.
// Change, add, delete, and reorder steps. Saving replaces only the changed blocks.
import { h, attempt, debounce, toast } from '../ui.js';
import { setDirtyCheck } from '../app.js';
import { textPreview, displayExpr } from './textpreview.js';

// Argument names for common Deltarune cutscene functions. Hints only: any function and argument list is accepted.
export const CATALOG = {
  c_sel: ['actor'], c_walk: ['dir', 'speed', 'frames'], c_walk_wait: ['dir', 'speed', 'frames'], c_walkdirect: ['x', 'y', 'frames'],
  c_walkdirect_wait: ['x', 'y', 'frames'], c_wait: ['frames'], c_facing: ['dir'], c_sprite: ['sprite'], c_autowalk: ['on'],
  c_imagespeed: ['speed'], c_imageindex: ['index'], c_speaker: ['name'], c_msgside: ['side'], c_msgsetloc: ['index', 'text', 'key'],
  c_msgnextloc: ['text', 'key'], c_msgsetsubloc: ['index', 'text', 'sub1', 'key'], c_msgnextsubloc: ['text', 'sub1', 'key'],
  c_fe: ['face'], c_talk: [], c_talk_wait: [], c_wait_talk: [], c_wait_box: ['box'], c_pan: ['x', 'y', 'frames'], c_pan_wait: ['x', 'y', 'frames'],
  c_panobj: ['object', 'frames'], c_pannable: ['on'], c_soundplay: ['sound'], c_snd_play: ['sound'], c_mus: ['command'], c_mus2: ['command', 'arg'],
  c_shake: [], c_emote: ['type', 'frames'], c_setxy: ['x', 'y'], c_addxy: ['x', 'y'], c_visible: ['on'], c_depth: ['depth'], c_flip: ['axis'],
  c_halt: [], c_actortokris: [], c_actortocaterpillar: [], c_terminatekillactors: [], c_jump: ['x', 'y', 'height', 'frames'],
  c_var_instance: ['instance', 'variable', 'value'], c_var_lerp_instance: ['instance', 'variable', 'from', 'to', 'frames'],
  c_script_instance: ['instance', 'script', 'arg'], c_instance: ['x', 'y', 'object'], c_waitcustom: [], c_waitcustom_end: [], c_customfunc: ['function'],
};

export async function render(main, ctx) {
  const { capi, ch, arg } = ctx;
  const split = h('div', { class: 'split' });
  main.append(split);
  const c = encodeURIComponent(ch);

  // List of code entries with steps.
  const q = h('input', { type: 'search', placeholder: 'Filter code entries' });
  const items = h('div', { class: 'items' });
  const info = h('span', { class: 'dim small' });
  const loadList = async () => {
    const r = await attempt(() => capi.cutscenes(q.value));
    if (!r) return;
    info.textContent = `${r.total} code entries with c_* steps`;
    items.replaceChildren(...r.items.map((it) => h('a', { href: `#/${c}/cutscenes/${encodeURIComponent(it.code)}`, class: it.code === arg ? 'active' : '', title: it.code },
      h('span', { style: { overflow: 'hidden', textOverflow: 'ellipsis' } }, it.code.replace(/^gml_(Object_|Script_|GlobalScript_)?/, '')), h('span', { class: 'dim' }, String(it.steps)))));
    items.querySelector('a.active')?.scrollIntoView({ block: 'center' });
  };
  q.addEventListener('input', debounce(loadList, 250));
  split.append(h('div', { class: 'list-pane' }, h('div', { class: 'list-head' }, h('b', null, 'Cutscenes'), q, info), items));
  loadList();

  const pane = h('div', { class: 'editor-pane' });
  split.append(pane);
  if (!arg) {
    pane.append(h('div', { class: 'editor-body dim' }, 'Select a code entry. Cutscenes are runs of c_* calls (c_sel, c_walk, c_msgsetloc, c_wait, ...).'));
    return;
  }
  const cs = await capi.cutscene(arg);
  const changed = new Set();
  setDirtyCheck(() => changed.size > 0);
  const saveBtn = h('button', { class: 'primary', disabled: true, onclick: () => save() }, 'Save');
  const markChanged = (bi) => { changed.add(bi); saveBtn.disabled = false; saveBtn.textContent = `Save (${changed.size} blocks)`; };
  const blocks = cs.blocks.map((b) => ({ ...b, steps: b.steps.map((s) => ({ func: s.func, args: [...s.args], line: s.line })) }));
  const funcList = h('datalist', { id: 'dl-cfuncs' }, Object.keys(CATALOG).map((f) => h('option', { value: f })));

  pane.append(h('div', { class: 'editor-head' }, h('h2', null, arg), cs.modified ? h('span', { class: 'badge mod' }, 'changed') : null, h('div', { class: 'grow' }),
    h('a', { class: 'button', href: `#/${c}/assets/code/${encodeURIComponent(arg)}` }, 'Open full code'), saveBtn), funcList);
  const body = h('div', { class: 'editor-body' });
  pane.append(body);
  body.append(h('p', { class: 'dim small' }, 'Arguments are GML: strings need quotes ("r"), names do not (kr, obj_x). Argument names are hints. Saving changes only the edited blocks.'));
  if (!blocks.length) body.append(h('p', null, 'No c_* steps in this code.'));

  blocks.forEach((b, bi) => {
    const box = h('div', { class: 'block' });
    const list = h('div');
    const draw = () => {
      list.replaceChildren();
      b.steps.forEach((s, si) => list.append(stepRow(b, bi, s, si, draw)));
      list.append(h('div', { class: 'row', style: { padding: '6px' } },
        h('button', { class: 'small', onclick: () => { b.steps.push({ func: 'c_wait', args: ['30'] }); markChanged(bi); draw(); } }, '+ Add step'),
        h('button', { class: 'small', onclick: () => { b.steps.push({ func: 'c_msgsetloc', args: ['0', '"* Text./%"', `"${arg}_dre_${Date.now().toString(36)}"`] }); markChanged(bi); draw(); } }, '+ Add text')));
    };
    box.append(h('div', { class: 'block-head' }, h('b', null, `Line ${b.line}`), h('span', { class: 'dim' }, b.context), h('div', { class: 'grow' }),
      h('a', { class: 'small', href: `#/${c}/assets/code/${encodeURIComponent(arg)}?line=${b.line}` }, 'in code')), list);
    draw();
    body.append(box);
  });

  function stepRow(b, bi, s, si, redraw) {
    const names = CATALOG[s.func] || [];
    const args = h('div', { class: 'args' });
    const drawArgs = () => {
      args.replaceChildren();
      s.args.forEach((a, ai) => {
        const label = names[ai] || (names[names.length - 1] === 'arg' ? 'arg' : `arg ${ai}`);
        args.append(h('label', null, label, h('input', { type: 'text', value: a, size: Math.max(4, Math.min(60, a.length + 1)), oninput: (e) => { s.args[ai] = e.target.value; markChanged(bi); updatePreview(); } })));
      });
      args.append(h('span', { class: 'col', style: { gap: '2px' } },
        h('button', { class: 'small', title: 'Add argument', onclick: () => { s.args.push('0'); markChanged(bi); drawArgs(); } }, '+'),
        s.args.length ? h('button', { class: 'small', title: 'Remove last argument', onclick: () => { s.args.pop(); markChanged(bi); drawArgs(); } }, '−') : null));
    };
    drawArgs();
    const preview = h('div');
    const updatePreview = () => {
      preview.replaceChildren();
      if (!/msg/.test(s.func)) return;
      const t = s.args.find((a) => /^"/.test(a.trim()) && !/_slash_|_dre_/.test(a));
      if (t) preview.append(textPreview(displayExpr(t.trim()).replace(/\\\\/g, '\\')));
    };
    updatePreview();
    const move = (d) => { const [x] = b.steps.splice(si, 1); b.steps.splice(si + d, 0, x); markChanged(bi); redraw(); };
    return h('div', { class: `step ${changed.has(bi) ? '' : ''}` },
      h('span', { class: 'num' }, String(si + 1)),
      h('input', { type: 'text', value: s.func, list: 'dl-cfuncs', class: 'mono', oninput: (e) => { s.func = e.target.value; markChanged(bi); } , onchange: () => redraw() }),
      h('div', null, args, preview),
      h('div', { class: 'row', style: { flexWrap: 'nowrap' } },
        h('button', { class: 'small', disabled: si === 0, title: 'Move up', onclick: () => move(-1) }, '↑'),
        h('button', { class: 'small', disabled: si === b.steps.length - 1, title: 'Move down', onclick: () => move(1) }, '↓'),
        h('button', { class: 'small', title: 'Duplicate', onclick: () => { b.steps.splice(si + 1, 0, { func: s.func, args: [...s.args] }); markChanged(bi); redraw(); } }, '⧉'),
        h('button', { class: 'small danger', title: 'Delete', onclick: () => { b.steps.splice(si, 1); markChanged(bi); redraw(); } }, '×')));
  }

  async function save() {
    const bad = [];
    const edits = [...changed].map((bi) => {
      const b = blocks[bi];
      for (const s of b.steps) if (!/^c_\w+$/.test(s.func)) bad.push(s.func);
      return { start: b.start, end: b.end, text: b.steps.map((s) => `${s.func}(${s.args.join(', ')});`).join(`\n${b.indent}`) };
    });
    if (bad.length) { toast(`Function names must start with c_: ${bad.join(', ')}`, 'error'); return; }
    if (await attempt(() => capi.patchCode(arg, cs.rev, edits), 'Cutscene saved')) {
      changed.clear();
      location.hash = `#/${c}/cutscenes/${encodeURIComponent(arg)}?r=${Date.now()}`;
    }
  }
}
