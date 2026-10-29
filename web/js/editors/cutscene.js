// Visual cutscene editor. Code entries with c_* calls show as:
// - a state graph: each `if (con == N)` part is a node, `con = M` gives an arrow;
// - per block of steps: a timeline (lanes per actor, text, camera, sound; only wait steps move time) with play,
//   and a step list with typed fields (direction, actor, frames, text, assets), drag to reorder.
// Saving sends the steps; the server checks each argument and writes only the changed blocks and new parts.
import { h, attempt, debounce, toast, promptDialog, assetDatalist, modal } from '../ui.js';
import { setDirtyCheck } from '../app.js';
import { textPreview, displayExpr } from './textpreview.js';
import { CATALOG, ACTORS, DIRS, argSpec, parseCall, stepGml, timeline, actorPositions, textAt, graphLayout } from './cutscenecore.js';

// Steps in the "add" menu, with their first arguments.
const PALETTE = [
  ['c_sel', ['kr']], ['c_walk', ['"r"', '4', '10']], ['c_walk_wait', ['"r"', '4', '10']], ['c_walkdirect', ['0', '0', '30']],
  ['c_facing', ['"d"']], ['c_sprite', ['spr_']], ['c_wait', ['30']], ['c_speaker', ['"susie"']], ['c_msgsetloc', ['0', '"* Text./%"', 'KEY']],
  ['c_msgnextloc', ['"* More text./%"', 'KEY']], ['c_talk_wait', []], ['c_pan', ['0', '0', '30']], ['c_pan_wait', ['0', '0', '30']],
  ['c_soundplay', ['snd_']], ['c_emote', ['"!"', '30']], ['c_shake', []], ['c_setxy', ['0', '0']], ['c_visible', ['1']], ['c_actortokris', []],
  ['c_terminatekillactors', []],
];
const LANE_COLORS = { text: '#ffd400', camera: '#36c5f0', sound: '#4ade80', wait: '#9090a8' };

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
  split.append(h('div', { class: 'list-pane' }, h('div', { class: 'list-head' }, h('b', null, 'Cutscenes'), q, info,
    h('a', { class: 'small', href: `#/${c}/wizards?kind=cutscene` }, '+ New cutscene trigger (wizard)')), items));
  loadList();

  const pane = h('div', { class: 'editor-pane' });
  split.append(pane);
  if (!arg) {
    pane.append(h('div', { class: 'editor-body dim' }, 'Select a code entry. Cutscenes are runs of c_* calls (c_sel, c_walk, c_msgsetloc, c_wait, ...).'));
    return;
  }
  const cs = await capi.cutscene(arg);
  const keyBase = `${arg.replace(/^gml_(Object_|Script_|GlobalScript_)?/, '')}_dre`;
  const sectionOf = new Map();
  cs.sections.forEach((s) => s.blocks.forEach((bi) => sectionOf.set(bi, s)));
  // Units: existing blocks and new sections. Each has its own step list.
  const units = cs.blocks.map((b, bi) => ({ kind: 'block', bi, start: b.start, end: b.end, indent: b.indent, line: b.line, context: b.context,
    section: sectionOf.get(bi), steps: b.steps.map((s) => ({ func: s.func, args: [...s.args] })), dirty: false }));
  const dirtyCount = () => units.filter((u) => u.dirty || u.kind === 'new').length;
  setDirtyCheck(() => dirtyCount() > 0);
  const saveBtn = h('button', { class: 'primary', disabled: true, onclick: () => save() }, 'Save');
  const markChanged = (u) => {
    u.dirty = true;
    const n = dirtyCount();
    saveBtn.disabled = !n;
    saveBtn.textContent = n ? `Save (${n})` : 'Save';
    u.redrawTimeline?.();
  };
  const funcList = h('datalist', { id: 'dl-cfuncs' }, Object.keys(CATALOG).map((f) => h('option', { value: f })));
  const actorList = h('datalist', { id: 'dl-actors' }, ACTORS.map((a) => h('option', { value: a })));

  pane.append(h('div', { class: 'editor-head' }, h('h2', null, arg), cs.modified ? h('span', { class: 'badge mod' }, 'changed') : null, h('div', { class: 'grow' }),
    h('button', { onclick: () => addSection(), title: 'Add an if (con == N) part after the last one' }, '+ Add part'),
    h('a', { class: 'button', href: `#/${c}/assets/code/${encodeURIComponent(arg)}` }, 'Open full code'), saveBtn), funcList, actorList);
  const body = h('div', { class: 'editor-body' });
  pane.append(body);
  body.append(h('p', { class: 'dim small' }, 'Arguments are GML: strings need quotes ("r"), names do not (kr, obj_x). The timeline is an estimate: ',
    'only wait steps (c_wait, *_wait, c_talk_wait) move time; text waits show as 45 steps. Drag ⠿ to reorder. Saving checks every argument and changes only the edited blocks.'));

  const graphBox = h('div', { class: 'card cs-graph' });
  body.append(graphBox);
  const unitBox = h('div');
  body.append(unitBox);
  drawGraph();
  if (!units.length) unitBox.append(h('p', null, 'No c_* steps in this code. Add a part, or use the code editor.'));
  for (const u of units) unitBox.append(unitCard(u));

  function drawGraph() {
    const secs = [...cs.sections, ...units.filter((u) => u.kind === 'new').map((u) => ({ con: String(u.con), next: u.next === null ? [] : [String(u.next)], blocks: [], fresh: true }))];
    graphBox.replaceChildren(h('div', { class: 'row' }, h('b', null, 'State graph (con)'),
      h('span', { class: 'dim small' }, secs.length ? 'Click a part to go to its steps. Dashed: set but no part found.' : 'No if (con == N) parts in this code.')));
    if (!secs.length) return;
    const g = graphLayout(secs);
    const W = 120, H = 44, GX = 150, top = 16;
    const NS = 'http://www.w3.org/2000/svg';
    const el = (tag, attrs, text) => {
      const e = document.createElementNS(NS, tag);
      for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
      if (text !== undefined) e.textContent = text;
      return e;
    };
    const svg = el('svg', { width: g.nodes.length * GX + 20, height: top + H + 70, class: 'cs-svg' });
    svg.append(el('defs', {}));
    svg.firstChild.append(Object.assign(el('marker', { id: 'cs-arrow', viewBox: '0 0 10 10', refX: 9, refY: 5, markerWidth: 7, markerHeight: 7, orient: 'auto-start-reverse' }), {}));
    svg.querySelector('marker').append(el('path', { d: 'M0,0 L10,5 L0,10 z', fill: '#9090a8' }));
    const pos = new Map(g.nodes.map((n) => [n.con, { x: 10 + n.x * GX, y: top }]));
    g.edges.forEach((e, k) => {
      const a = pos.get(e.from), b = pos.get(e.to);
      if (!a || !b) return;
      if (e.from === e.to) return;
      const d = e.back
        ? `M${a.x + W / 2},${a.y + H} C${a.x + W / 2},${a.y + H + 50 + k % 3 * 6} ${b.x + W / 2},${b.y + H + 50 + k % 3 * 6} ${b.x + W / 2},${b.y + H + 2}`
        : `M${a.x + W},${a.y + H / 2} L${b.x - 2},${b.y + H / 2}`;
      svg.append(el('path', { d, class: e.back ? 'edge back' : 'edge', 'marker-end': 'url(#cs-arrow)' }));
    });
    for (const n of g.nodes) {
      const p = pos.get(n.con);
      const sec = secs.find((s) => s.con === n.con);
      const steps = sec ? (sec.fresh ? units.find((u) => u.kind === 'new' && String(u.con) === n.con)?.steps.length : sec.blocks.reduce((t, bi) => t + units[bi].steps.length, 0)) : 0;
      const grp = el('g', { class: `node${n.missing ? ' missing' : ''}${sec?.fresh ? ' fresh' : ''}${steps ? '' : ' empty'}`, transform: `translate(${p.x},${p.y})` });
      grp.append(el('rect', { width: W, height: H, rx: 6 }), el('text', { x: W / 2, y: 18, 'text-anchor': 'middle' }, `con == ${n.con}`),
        el('text', { x: W / 2, y: 34, 'text-anchor': 'middle', class: 'sub' }, n.missing ? 'no part' : `${steps} steps${sec?.fresh ? ' (new)' : ''}`));
      grp.addEventListener('click', () => {
        const u = sec?.fresh ? units.find((x) => x.kind === 'new' && String(x.con) === n.con) : units[sec?.blocks[0]];
        u?.card?.scrollIntoView({ behavior: 'smooth', block: 'start' });
      });
      svg.append(grp);
    }
    graphBox.append(h('div', { style: { overflowX: 'auto' } }, svg));
  }

  function unitCard(u) {
    const list = h('div');
    const tlBox = h('div');
    const title = u.kind === 'new' ? `New part: con == ${u.con}${u.next !== null ? ` → ${u.next}` : ''}` : `Line ${u.line}`;
    const ctxText = u.kind === 'new' ? 'added after the last part' : (u.section ? `con == ${u.section.con}${u.section.next.length ? ` → ${u.section.next.join(', ')}` : ''}` : u.context);
    const addSel = h('select', { class: 'small', onchange: (e) => {
      const p = PALETTE.find((x) => x[0] === e.target.value);
      e.target.value = '';
      if (!p) return;
      u.steps.push({ func: p[0], args: p[1].map((a) => (a === 'KEY' ? `"${keyBase}_${Date.now().toString(36)}"` : a)) });
      markChanged(u);
      draw();
    } }, h('option', { value: '' }, '+ Add step'), PALETTE.map(([f]) => h('option', { value: f }, f)));
    const draw = () => {
      list.replaceChildren();
      u.steps.forEach((s, si) => list.append(stepRow(u, s, si, draw)));
      list.append(h('div', { class: 'row', style: { padding: '6px' } }, addSel,
        h('button', { class: 'small', onclick: () => pasteGml(u, draw) }, 'Paste GML'),
        u.kind === 'new' ? h('button', { class: 'small danger', onclick: () => { units.splice(units.indexOf(u), 1); u.card.remove(); markChanged(u); drawGraph(); } }, 'Remove part') : null));
      u.redrawTimeline();
    };
    u.redrawTimeline = () => drawTimeline(u, tlBox);
    u.card = h('div', { class: `block${u.kind === 'new' ? ' fresh' : ''}` },
      h('div', { class: 'block-head' }, h('b', null, title), h('span', { class: 'dim' }, ctxText), h('div', { class: 'grow' }),
        u.kind === 'block' ? h('a', { class: 'small', href: `#/${c}/assets/code/${encodeURIComponent(arg)}?line=${u.line}` }, 'in code') : null),
      tlBox, list);
    draw();
    return u.card;
  }

  function drawTimeline(u, box) {
    stopPlay(u);
    const tl = timeline(u.steps);
    const scale = Math.max(0.5, Math.min(6, 680 / tl.length));
    const rows = tl.lanes.map((lane) => {
      const row = h('div', { class: 'tl-lane' }, h('span', { class: 'tl-label' }, lane));
      for (const it of tl.items.filter((x) => x.lane === lane)) {
        const s = u.steps[it.i];
        const color = LANE_COLORS[lane] || '#ff7ab2';
        row.append(h('div', { class: `tl-item${it.blocking ? ' blocking' : ''}${it.known ? '' : ' unknown'}`, title: `${it.i + 1}: ${stepGml(s)}  (start ${it.start}, ${it.dur} steps)`,
          style: { left: `${90 + it.start * scale}px`, width: `${Math.max(8, it.dur * scale)}px`, background: color },
          onclick: () => selectStep(u, it.i) }, s.func.replace(/^c_/, '')));
      }
      return row;
    });
    const head = h('div', { class: 'tl-lane tl-ruler' }, h('span', { class: 'tl-label dim' }, `${tl.length} steps`));
    for (let f = 0; f <= tl.length; f += tl.length > 600 ? 150 : tl.length > 150 ? 30 : 10) {
      head.append(h('span', { class: 'tl-tick', style: { left: `${90 + f * scale}px` } }, String(f)));
    }
    const playhead = h('div', { class: 'tl-playhead', hidden: true });
    const lanes = h('div', { class: 'tl', style: { width: `${100 + tl.length * scale}px` } }, head, ...rows, playhead);
    const stage = h('canvas', { width: 320, height: 160, class: 'tl-stage', hidden: true });
    const text = h('div', { class: 'tl-text' });
    const playBtn = h('button', { class: 'small', disabled: !u.steps.length, onclick: () => (u.play ? stopPlay(u) : startPlay()) }, '▶ Play');
    box.replaceChildren(h('div', { class: 'tl-wrap' }, h('div', { class: 'row', style: { padding: '4px 8px' } }, playBtn, h('span', { class: 'dim small' }, 'Stage: actors start at 0,0; walks and c_setxy/c_addxy move them.')),
      h('div', { style: { overflowX: 'auto' } }, lanes), h('div', { class: 'row', style: { alignItems: 'flex-start', padding: '0 8px 6px' } }, stage, text)));
    function frameAt(t) {
      playhead.hidden = false;
      playhead.style.left = `${90 + t * scale}px`;
      const pos = actorPositions(u.steps, t);
      stage.hidden = false;
      const g = stage.getContext('2d');
      g.fillStyle = '#050508';
      g.fillRect(0, 0, stage.width, stage.height);
      const pts = Object.values(pos);
      const span = Math.max(80, ...pts.map((p) => Math.max(Math.abs(p.x), Math.abs(p.y)) * 2 + 40));
      const k = Math.min(stage.width, stage.height) / span;
      Object.entries(pos).forEach(([name, p], i) => {
        const x = stage.width / 2 + p.x * k, y = stage.height / 2 + p.y * k;
        g.fillStyle = ['#36c5f0', '#ff7ab2', '#4ade80', '#ffd400'][i % 4];
        g.beginPath(); g.arc(x, y, 6, 0, Math.PI * 2); g.fill();
        g.fillStyle = '#e6e6f0'; g.font = '11px monospace'; g.fillText(name, x + 8, y + 4);
      });
      g.fillStyle = '#9090a8'; g.fillText(`step ${Math.floor(t)}`, 4, 12);
      const tx = textAt(u.steps, t);
      text.replaceChildren(tx ? textPreview(tx) : h('span', { class: 'dim small' }, 'no text'));
    }
    function startPlay() {
      let t = 0;
      playBtn.textContent = '■ Stop';
      u.play = setInterval(() => {
        frameAt(t);
        t += 1;
        if (t > tl.length) stopPlay(u);
      }, 1000 / 30);
      u.stopUi = () => { playBtn.textContent = '▶ Play'; };
    }
  }

  function stopPlay(u) {
    if (u.play) clearInterval(u.play);
    u.play = null;
    u.stopUi?.();
  }

  function selectStep(u, si) {
    const row = u.card.querySelectorAll('.step')[si];
    if (!row) return;
    u.card.querySelectorAll('.step.sel').forEach((r) => r.classList.remove('sel'));
    row.classList.add('sel');
    row.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
    row.querySelector('input,select')?.focus();
  }

  function argInput(u, s, ai, spec, onChange) {
    const value = s.args[ai];
    const set = (v) => { s.args[ai] = v; markChanged(u); onChange(); };
    const t = spec?.type || 'gml';
    if (t === 'dir' || t === 'bool') {
      const opts = t === 'dir' ? DIRS : ['0', '1'];
      return h('select', { onchange: (e) => set(e.target.value) },
        [...new Set([...opts, value])].map((o) => h('option', { value: o, selected: o === value }, o)));
    }
    const list = t === 'actor' ? 'dl-actors' : ['sprites', 'sounds', 'objects'].includes(t) ? assetDatalist(capi, ch, t) : undefined;
    const size = Math.max(t === 'frames' || t === 'num' ? 4 : 8, Math.min(60, value.length + 1));
    return h('input', { type: 'text', value, size, list, class: t === 'frames' ? 'arg-frames' : '', oninput: (e) => set(e.target.value) });
  }

  function stepRow(u, s, si, redraw) {
    const args = h('div', { class: 'args' });
    const preview = h('div');
    const updatePreview = () => {
      preview.replaceChildren();
      if (!/msg/.test(s.func)) return;
      const k = argSpec(s.func).findIndex((a) => a.name === 'text');
      const t = s.args[k >= 0 ? k : 0];
      if (t && /^"/.test(t.trim())) preview.append(textPreview(displayExpr(t.trim()).replace(/\\\\/g, '\\')));
    };
    const drawArgs = () => {
      args.replaceChildren();
      const spec = argSpec(s.func);
      s.args.forEach((a, ai) => {
        const label = spec[ai]?.name || `arg ${ai}`;
        args.append(h('label', null, label, argInput(u, s, ai, spec[ai], updatePreview)));
      });
      args.append(h('span', { class: 'col', style: { gap: '2px' } },
        h('button', { class: 'small', title: 'Add argument', onclick: () => { s.args.push('0'); markChanged(u); drawArgs(); } }, '+'),
        s.args.length ? h('button', { class: 'small', title: 'Remove last argument', onclick: () => { s.args.pop(); markChanged(u); drawArgs(); } }, '−') : null));
    };
    drawArgs();
    updatePreview();
    const move = (from, to) => { const [x] = u.steps.splice(from, 1); u.steps.splice(to, 0, x); markChanged(u); redraw(); };
    const row = h('div', { class: 'step', draggable: true },
      h('span', { class: 'num', title: 'Drag to move' }, `⠿ ${si + 1}`),
      h('input', { type: 'text', value: s.func, list: 'dl-cfuncs', class: 'mono', oninput: (e) => { s.func = e.target.value; markChanged(u); }, onchange: () => {
        // A known function gets its argument count.
        const n = (CATALOG[s.func] || null)?.length;
        if (n !== undefined && s.args.length < n) while (s.args.length < n) s.args.push('0');
        redraw();
      } }),
      h('div', null, args, preview),
      h('div', { class: 'row', style: { flexWrap: 'nowrap' } },
        h('button', { class: 'small', disabled: si === 0, title: 'Move up', onclick: () => move(si, si - 1) }, '↑'),
        h('button', { class: 'small', disabled: si === u.steps.length - 1, title: 'Move down', onclick: () => move(si, si + 1) }, '↓'),
        h('button', { class: 'small', title: 'Duplicate', onclick: () => { u.steps.splice(si + 1, 0, { func: s.func, args: [...s.args] }); markChanged(u); redraw(); } }, '⧉'),
        h('button', { class: 'small danger', title: 'Delete', onclick: () => { u.steps.splice(si, 1); markChanged(u); redraw(); } }, '×')));
    row.addEventListener('dragstart', (e) => { e.dataTransfer.setData('text/x-step', `${units.indexOf(u)}:${si}`); row.classList.add('dragging'); });
    row.addEventListener('dragend', () => row.classList.remove('dragging'));
    row.addEventListener('dragover', (e) => { if (e.dataTransfer.types.includes('text/x-step')) { e.preventDefault(); row.classList.add('drop'); } });
    row.addEventListener('dragleave', () => row.classList.remove('drop'));
    row.addEventListener('drop', (e) => {
      e.preventDefault();
      row.classList.remove('drop');
      const [ui, from] = e.dataTransfer.getData('text/x-step').split(':').map(Number);
      const src = units[ui];
      if (!src) return;
      if (src === u) { if (from !== si) move(from, si); return; }
      // Move between blocks.
      const [x] = src.steps.splice(from, 1);
      u.steps.splice(si, 0, x);
      markChanged(src);
      markChanged(u);
      for (const v of [src, u]) v.card.replaceWith(unitCard(v));
    });
    return row;
  }

  async function pasteGml(u, redraw) {
    const ta = h('textarea', { rows: 8, style: { width: '100%' }, placeholder: 'c_sel(kr);\nc_walk("r", 4, 10);\nc_wait(30);' });
    const text = await modal('Paste GML steps (one c_* call per line)', ta, [{ label: 'Cancel', value: null }, { label: 'Add', value: () => ta.value, primary: true }]);
    if (!text) return;
    const bad = [];
    for (const line of text.split('\n').filter((l) => l.trim())) {
      const st = parseCall(line);
      if (st) u.steps.push(st); else bad.push(line.trim());
    }
    if (bad.length) toast(`Not a c_* call, not added: ${bad.join(' | ')}`, 'error');
    markChanged(u);
    redraw();
  }

  async function addSection() {
    const known = [...cs.sections.map((s) => Number(s.con)), ...units.filter((u) => u.kind === 'new').map((u) => u.con), ...cs.sections.flatMap((s) => s.next.map(Number))].filter(Number.isFinite);
    const firstFree = known.length ? Math.max(...known) + 1 : 1;
    // Prefer a value that a part sets but no part handles yet.
    const wanted = cs.sections.flatMap((s) => s.next).map(Number).find((v) => Number.isFinite(v) && !cs.sections.some((s) => Number(s.con) === v) && !units.some((u) => u.con === v));
    const v = await promptDialog('New part: con value', String(wanted ?? firstFree));
    if (v === null) return;
    const con = Number(v);
    if (!Number.isInteger(con)) { toast('Use a whole number.', 'error'); return; }
    if (cs.sections.some((s) => Number(s.con) === con) || units.some((u) => u.con === con)) { toast(`con == ${con} exists already.`, 'error'); return; }
    const nx = await promptDialog('Set con to this value first (empty: do not set)', String(con + 1));
    if (nx === null) return;
    const next = nx.trim() === '' ? null : Number(nx);
    if (next !== null && !Number.isInteger(next)) { toast('Use a whole number.', 'error'); return; }
    const u = { kind: 'new', con, next, steps: [{ func: 'c_wait', args: ['30'] }], dirty: true };
    units.push(u);
    unitBox.append(unitCard(u));
    markChanged(u);
    drawGraph();
    u.card.scrollIntoView({ behavior: 'smooth' });
  }

  async function save() {
    const bad = units.flatMap((u) => u.steps.filter((s) => !/^c_\w+$/.test(s.func)).map((s) => s.func));
    if (bad.length) { toast(`Function names must start with c_: ${bad.join(', ')}`, 'error'); return; }
    const body = {
      rev: cs.rev,
      blocks: units.filter((u) => u.kind === 'block' && u.dirty).map((u) => ({ start: u.start, end: u.end, indent: u.indent, steps: u.steps })),
      sections: units.filter((u) => u.kind === 'new').map((u) => ({ con: u.con, next: u.next, steps: u.steps })),
    };
    if (await attempt(() => capi.putCutscene(arg, body), 'Cutscene saved')) {
      for (const u of units) u.dirty = false;
      units.splice(0, units.length, ...units.filter((u) => u.kind !== 'new'));
      location.hash = `#/${c}/cutscenes/${encodeURIComponent(arg)}?r=${Date.now()}`;
    }
  }

  return () => units.forEach(stopPlay);
}
