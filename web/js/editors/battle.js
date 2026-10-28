// Bullet pattern and battle box editor. Edit emitters and box keyframes, and see the wave at once:
// scrub the timeline, or test it with the soul (arrows, X or Shift to go slow). Patterns are kept on the server,
// made into GML for a wave object, and installed into the mod (obj_dre_wave_<name>).
import { h, add, clear, toast, attempt, modal, promptDialog, assetDatalist, loadImage } from '../ui.js';
import { api } from '../api.js';
import { setDirtyCheck } from '../app.js';
import { SCREEN_W, SCREEN_H, DEFAULT_EMITTER, normalize, createSim, simulateTo, boxAt } from './battlesim.js';

const enc = encodeURIComponent;
const NAME_RE = /^[A-Za-z0-9_]{1,64}$/;

// Emitter fields: [key, label, kind, title]. kind: n (number), i (whole number), b (check), s (text), o (origin), c (colour).
const EMITTER_FIELDS = [
  ['Timing', [
    ['start', 'start', 'i', 'First step that can fire (30 steps = 1 s)'],
    ['end', 'end', 'i', 'Last step that can fire'],
    ['interval', 'every', 'i', 'Steps between shots'],
  ]],
  ['Position', [
    ['origin', 'from', 'o', 'Box centre, soul (at the shot), or screen top-left'],
    ['x', 'x', 'n', 'Offset from the origin'],
    ['y', 'y', 'n', 'Offset from the origin (down is +)'],
    ['jitter_x', '± x', 'n', 'Random x offset'],
    ['jitter_y', '± y', 'n', 'Random y offset'],
    ['radius', 'radius', 'n', 'Bullets start this far out, in their direction'],
  ]],
  ['Direction', [
    ['count', 'count', 'i', 'Bullets per shot'],
    ['angle', 'angle', 'n', 'Degrees: 0 right, 90 up, 180 left, 270 down'],
    ['spread', 'spread', 'n', 'Arc of the shot in degrees (360 = full ring)'],
    ['spin', 'spin', 'n', 'Angle change per shot (spirals)'],
    ['jitter_angle', '± angle', 'n', 'Random angle change'],
    ['aim', 'aim at soul', 'b', 'Add the direction to the soul'],
  ]],
  ['Motion', [
    ['speed', 'speed', 'n', 'Pixels per step'],
    ['speed_step', '+speed/bullet', 'n', 'Speed added for each next bullet of a shot (layers)'],
    ['accel', 'accel', 'n', 'Speed change per step'],
    ['turn', 'turn', 'n', 'Direction change per step (curves)'],
    ['gravity', 'gravity', 'n', 'GameMaker gravity'],
    ['gravity_dir', 'gravity dir', 'n', 'Gravity direction (270 = down)'],
    ['life', 'life', 'i', 'Steps before the bullet goes away. 0: until far off screen'],
  ]],
  ['Bullet', [
    ['sprite', 'sprite', 's', 'Sprite of the bullet. Empty: the sprite of the bullet object'],
    ['scale', 'scale', 'n', 'image_xscale and image_yscale'],
    ['rotate', 'rotate to motion', 'b', 'image_angle follows the direction'],
    ['size', 'hit radius', 'n', 'Hit radius in the test (the game uses the sprite mask)'],
    ['color', 'colour', 'c', 'Colour in the test when there is no sprite'],
    ['damage', 'damage', 'i', 'Sets damage on the bullet when more than 0'],
  ]],
];

const PRESETS = {
  'Ring from the top': { name: 'ring', origin: 'box', y: -140, count: 12, angle: 270, spread: 360, spin: 9, interval: 18, speed: 2.5 },
  'Aimed burst': { name: 'aimed', origin: 'box', x: 0, y: -150, count: 3, angle: 0, spread: 24, aim: true, interval: 15, speed: 5 },
  Rain: { name: 'rain', origin: 'box', x: 0, y: -130, jitter_x: 80, count: 1, angle: 270, spread: 0, spin: 0, interval: 4, speed: 4, accel: 0.05 },
  Spiral: { name: 'spiral', origin: 'box', x: 0, y: 0, radius: 120, count: 4, angle: 0, spread: 360, spin: 11, interval: 6, speed: -1.5, life: 80 },
  'Side wall': { name: 'wall', origin: 'box', x: -160, y: 0, jitter_y: 60, count: 1, angle: 0, spread: 0, spin: 0, interval: 8, speed: 3.5 },
  'Falling arcs': { name: 'arcs', origin: 'box', x: 0, y: -60, count: 5, angle: 90, spread: 80, spin: 0, interval: 30, speed: 4, gravity: 0.15, gravity_dir: 270 },
};

export async function render(main, ctx) {
  const { capi, ch, arg } = ctx;
  const c = enc(ch);
  const pbase = `${capi.base}/patterns`;
  const [sprIdx, objIdx] = await Promise.all([
    capi.index('sprites', ['Width', 'Height', 'OriginX', 'OriginY', 'TextureCount']).catch(() => ({})),
    capi.index('objects', ['Sprite']).catch(() => ({})),
  ]);
  const heartSpr = sprIdx.spr_heart ? 'spr_heart' : null;
  let list = await api.get(pbase).catch(() => []);
  let name = arg && NAME_RE.test(arg) ? arg : null;
  let pat = normalize(name ? await api.get(`${pbase}/${enc(name)}`).catch(() => ({})) : {});
  let saved = JSON.stringify(pat);
  const dirty = () => JSON.stringify(pat) !== saved;
  setDirtyCheck(dirty);

  // ---- Simulation state ----
  let sim = createSim(pat);
  let playing = true, test = false, loop = true, speed = 1, acc = 0, last = 0, running = true, seed = 1;
  const view = { hitboxes: false, origins: true, trails: false };
  const keys = new Set();

  // ---- Layout ----
  const canvas = h('canvas', { tabindex: 0, width: SCREEN_W, height: SCREEN_H });
  const wrap = h('div', { class: 'canvas-wrap' }, canvas);
  const scrub = h('input', { type: 'range', min: 0, max: pat.duration, value: 0, oninput: (e) => seek(+e.target.value) });
  const frameLabel = h('span', { class: 'mono small', style: { minWidth: '150px' } });
  const playBtn = h('button', { class: 'small', onclick: () => setPlaying(!playing) }, 'Pause');
  const testBtn = h('button', { class: 'small', title: 'Control the soul: arrows move, X or Shift slow (T)', onclick: () => setTest(!test) }, 'Test with soul');
  const stats = h('div', { class: 'status' });
  const timeline = h('div', { class: 'timeline' },
    playBtn,
    h('button', { class: 'small', title: 'Back to step 0 (R)', onclick: () => restart() }, '⏮'),
    h('button', { class: 'small', title: 'One step (.)', onclick: () => { setPlaying(false); stepOnce(); } }, '+1'),
    scrub, frameLabel,
    h('select', { title: 'Speed', onchange: (e) => { speed = +e.target.value; } }, ['0.25', '0.5', '1', '2', '4'].map((v) => h('option', { value: v, selected: v === '1' }, `${v}×`))),
    h('label', { class: 'field check small' }, h('input', { type: 'checkbox', checked: loop, onchange: (e) => { loop = e.target.checked; } }), h('span', null, 'loop')),
    testBtn);
  const viewOpts = h('div', { class: 'toolbar' },
    ...Object.entries({ hitboxes: 'hit circles', origins: 'emitter points', trails: 'trails' }).map(([k, label]) =>
      h('label', { class: 'field check small' }, h('input', { type: 'checkbox', checked: view[k], onchange: (e) => { view[k] = e.target.checked; } }), h('span', null, label))),
    h('span', { class: 'dim small' }, 'Edits show at once. Click the stage, then T to test, R to restart, Space to pause.'));
  const side = h('div', { class: 'side battle-side' });
  main.append(h('div', { class: 'battle' }, h('div', { class: 'battle-stage' }, viewOpts, wrap, timeline, stats), side));

  // ---- Pattern list and file actions ----
  const go = (n) => { location.hash = `#/${c}/battle${n ? `/${enc(n)}` : ''}`; };
  async function save() {
    if (!name) {
      const n = await promptDialog('Pattern name (letters, digits, _)', 'wave1');
      if (!n) return false;
      if (!NAME_RE.test(n)) { toast('Use 1-64 letters, digits, or _.', 'error'); return false; }
      name = n;
    }
    const r = await attempt(() => api.put(`${pbase}/${enc(name)}`, pat), `Saved ${name}`);
    if (!r) return false;
    saved = JSON.stringify(pat);
    list = await api.get(pbase).catch(() => list);
    if (location.hash !== `#/${c}/battle/${enc(name)}`) { setDirtyCheck(null); go(name); } else drawSide();
    return true;
  }
  async function showGml() {
    if (!name && !(await save())) return;
    const g = await attempt(() => api.post(`${pbase}/${enc(name)}/gml`, pat));
    if (!g) return;
    await modal(`GML for ${g.object}`, h('div', { class: 'col' },
      h('p', { class: 'small dim' }, 'Start the wave from an enemy attack: ', h('code', null, `instance_create(0, 0, ${g.object});`)),
      h('b', { class: 'mono small' }, g.create.code), h('pre', { class: 'gml-out mono' }, g.create.source),
      h('b', { class: 'mono small' }, g.step.code), h('pre', { class: 'gml-out mono' }, g.step.source)), [{ label: 'Close', value: true, primary: true }]);
  }
  async function install() {
    if (!name && !(await save())) return;
    const r = await attempt(() => api.post(`${pbase}/${enc(name)}/install`, pat));
    if (!r) return;
    saved = JSON.stringify(pat);
    await modal('Installed in the mod', h('div', { class: 'col' },
      h('p', null, 'Made ', h('a', { href: `#/${c}/assets/objects/${enc(r.object)}` }, r.object), ' with Create and Step code. Build the chapter to put it in data.win.'),
      h('p', { class: 'small' }, 'Start the wave from enemy attack code, for example in the enemy object where it makes its bullets:'),
      h('pre', { class: 'gml-out mono' }, `instance_create(0, 0, ${r.object});`),
      h('p', { class: 'small dim' }, `Bullets are ${pat.bullet_object} instances. Install again after each change.`)), [{ label: 'OK', value: true, primary: true }]);
    drawSide();
  }
  async function remove() {
    if (!name) return;
    const how = await modal(`Delete pattern ${name}?`, h('p', null, 'An installed wave object can stay in the mod, or go with the pattern.'),
      [{ label: 'Cancel', value: null }, { label: 'Delete with wave object', value: 'all' }, { label: 'Delete, keep wave object', value: 'keep', primary: true }]);
    if (!how) return;
    const q = how === 'all' ? '?uninstall=true' : '';
    if (await attempt(() => api.del(`${pbase}/${enc(name)}${q}`), 'Deleted')) { saved = JSON.stringify(pat); setDirtyCheck(null); go(''); }
  }
  async function duplicate() {
    const n = await promptDialog('Name of the copy', name ? `${name}_copy` : 'wave1');
    if (!n) return;
    if (!NAME_RE.test(n)) { toast('Use 1-64 letters, digits, or _.', 'error'); return; }
    if (await attempt(() => api.put(`${pbase}/${enc(n)}`, pat), `Saved ${n}`)) { saved = JSON.stringify(pat); setDirtyCheck(null); go(n); }
  }
  const onSaveKey = (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 's') { e.preventDefault(); save(); }
  };
  document.addEventListener('keydown', onSaveKey);

  // ---- Change handling: edits apply at once ----
  function changed(structural) {
    pat.duration = Math.max(1, Math.floor(pat.duration) || 1);
    scrub.max = pat.duration;
    resim();
    if (structural) drawSide();
    else updateHead();
  }
  function resim() {
    // Run the new pattern to the same step. In a test the soul stays where it is.
    const t = Math.min(sim.t, pat.duration);
    const soul = test ? { x: sim.soul.x, y: sim.soul.y } : null;
    sim = createSim(pat, { seed, soul });
    while (sim.t < t) sim.step(test ? {} : null);
  }
  function seek(t) {
    sim = simulateTo(pat, t, { seed });
    if (test) setTest(false);
  }
  function restart() {
    seed = test ? (seed * 1103515245 + 12345) >>> 0 : 1;
    sim = createSim(pat, { seed });
  }
  function stepOnce() {
    if (sim.done) { if (loop) restart(); else return; }
    sim.step(test ? input() : null);
  }
  const input = () => ({ left: keys.has('left'), right: keys.has('right'), up: keys.has('up'), down: keys.has('down'), slow: keys.has('slow') });
  function setPlaying(p) { playing = p; playBtn.textContent = p ? 'Pause' : 'Play'; }
  function setTest(t) {
    test = t;
    testBtn.classList.toggle('active', t);
    testBtn.textContent = t ? 'Testing (T)' : 'Test with soul';
    if (t) { restart(); setPlaying(true); canvas.focus(); }
  }

  // ---- Side panel ----
  const head = h('div');
  function updateHead() {
    head.replaceChildren(h('div', { class: 'row' },
      h('select', { style: { flex: 1 }, onchange: (e) => go(e.target.value) },
        h('option', { value: '' }, name ? '(new pattern)' : '(new pattern, not saved)'),
        list.map((p) => h('option', { value: p.name, selected: p.name === name }, `${p.name} · ${p.emitters} emitter(s)`))),
      h('button', { class: 'small', onclick: () => go('') }, 'New')),
    h('div', { class: 'row', style: { marginTop: '6px' } },
      h('button', { class: dirty() ? 'primary small' : 'small', title: 'Ctrl+S', onclick: save }, dirty() ? 'Save*' : 'Save'),
      h('button', { class: 'small', onclick: duplicate }, 'Duplicate'),
      h('button', { class: 'small', onclick: showGml }, 'GML'),
      h('button', { class: 'small', title: 'Save, then write the wave object and its code into the mod', onclick: install }, 'Install in mod'),
      name ? h('button', { class: 'small danger', onclick: remove }, 'Delete') : null),
    h('div', { class: 'small dim', style: { marginTop: '4px' } }, name ? `Wave object: obj_dre_wave_${name}` : 'Save to choose a name.'));
  }

  const numIn = (obj, key, onC, attrs = {}) => h('input', { type: 'number', step: attrs.step ?? 'any', value: obj[key], style: { width: attrs.width || '68px' }, title: attrs.title, oninput: (e) => {
    if (e.target.value === '') return;
    let v = Number(e.target.value);
    if (Number.isNaN(v)) return;
    if (attrs.int) v = Math.round(v);
    if (attrs.min !== undefined) v = Math.max(attrs.min, v);
    obj[key] = v;
    onC();
  } });
  const fieldWrap = (label, input, title) => h('label', { class: 'field', title }, h('span', null, label), input);

  function emitterCard(e, i) {
    const onC = () => changed(false);
    const body = h('div', { class: 'col', style: { gap: '6px' } });
    for (const [group, fields] of EMITTER_FIELDS) {
      body.append(h('div', { class: 'small dim' }, group), h('div', { class: 'fields' }, fields.map(([k, label, kind, title]) => {
        if (kind === 'b') return h('label', { class: 'field check small', title }, h('input', { type: 'checkbox', checked: !!e[k], onchange: (ev) => { e[k] = ev.target.checked; onC(); } }), h('span', null, label));
        if (kind === 'o') return fieldWrap(label, h('select', { onchange: (ev) => { e[k] = ev.target.value; onC(); } }, ['box', 'soul', 'screen'].map((o) => h('option', { value: o, selected: e[k] === o }, o))), title);
        if (kind === 'c') return fieldWrap(label, h('input', { type: 'color', value: e[k] || '#ffffff', oninput: (ev) => { e[k] = ev.target.value; onC(); } }), title);
        if (kind === 's') return fieldWrap(label, h('input', { type: 'text', value: e[k], list: assetDatalist(capi, ch, 'sprites'), style: { width: '150px' }, oninput: (ev) => { e[k] = ev.target.value.trim(); onC(); } }), title);
        return fieldWrap(label, numIn(e, k, onC, { int: kind === 'i', min: ['interval', 'count'].includes(k) ? 1 : kind === 'i' ? 0 : undefined, step: kind === 'i' ? 1 : 'any' }), title);
      })));
    }
    body.append(h('details', null, h('summary', { class: 'small' }, 'GML on spawn (runs with the new bullet)'),
      h('textarea', { class: 'mono', rows: 3, style: { width: '100%' }, placeholder: 'image_blend = c_yellow;', value: e.on_spawn, oninput: (ev) => { e.on_spawn = ev.target.value; updateHead(); } })));
    const move = (d) => { const j = i + d; if (j < 0 || j >= pat.emitters.length) return; [pat.emitters[i], pat.emitters[j]] = [pat.emitters[j], pat.emitters[i]]; changed(true); };
    return h('div', { class: `card emitter${e.enabled ? '' : ' off'}` },
      h('div', { class: 'row', style: { flexWrap: 'nowrap' } },
        h('input', { type: 'checkbox', checked: e.enabled, title: 'On', onchange: (ev) => { e.enabled = ev.target.checked; changed(true); } }),
        h('input', { type: 'text', value: e.name, style: { flex: 1, minWidth: 0 }, oninput: (ev) => { e.name = ev.target.value; updateHead(); } }),
        h('button', { class: 'small', title: 'Up', onclick: () => move(-1) }, '↑'),
        h('button', { class: 'small', title: 'Down', onclick: () => move(1) }, '↓'),
        h('button', { class: 'small', title: 'Copy', onclick: () => { pat.emitters.splice(i + 1, 0, structuredClone(e)); changed(true); } }, '⧉'),
        h('button', { class: 'small danger', title: 'Remove', onclick: () => { pat.emitters.splice(i, 1); changed(true); } }, '✕')),
      h('details', { open: pat.emitters.length <= 2 }, h('summary', { class: 'small' }, `${e.count}× every ${e.interval} from ${e.start} to ${e.end}`), body));
  }

  function boxCard() {
    const onC = () => changed(false);
    const keyRow = (k, idx) => h('div', { class: 'fields' },
      idx === null ? h('b', { class: 'small', style: { width: '40px' } }, 'step 0') : fieldWrap('step', numIn(k, 'frame', onC, { int: true, min: 1, width: '56px' })),
      ['x', 'y', 'w', 'h'].map((f) => fieldWrap(f, numIn(k, f, onC, { min: f === 'w' || f === 'h' ? 0 : undefined, width: '56px' }))),
      idx === null ? null : h('button', { class: 'small danger', onclick: () => { pat.box_keys.splice(idx, 1); changed(true); } }, '✕'));
    return h('div', { class: 'card' }, h('h3', null, 'Battle box'),
      h('p', { class: 'small dim' }, 'Centre point and size in screen pixels (640×480). More keys move and resize the box over time.'),
      keyRow(pat.box, null),
      pat.box_keys.map((k, i) => keyRow(k, i)),
      h('div', { class: 'row', style: { marginTop: '6px' } },
        h('button', { class: 'small', onclick: () => { const last = pat.box_keys[pat.box_keys.length - 1] || pat.box; pat.box_keys.push({ ...last, frame: Math.min(pat.duration, (last.frame || 0) + 30) }); changed(true); } }, '+ Key'),
        h('button', { class: 'small', title: 'Add a key at the current step with the box as it is now', onclick: () => { pat.box_keys.push({ frame: Math.max(1, sim.t), ...roundBox(boxAt(pat, sim.t)) }); changed(true); } }, '+ Key here'),
        h('label', { class: 'field check small', title: 'Make GML that moves and resizes obj_growtangle. Off: the game box stays as the game sets it.' },
          h('input', { type: 'checkbox', checked: pat.box_code, onchange: (e) => { pat.box_code = e.target.checked; changed(true); } }), h('span', null, 'write box code (experimental)'))),
      pat.box_keys.length && !pat.box_code ? h('p', { class: 'small', style: { color: 'var(--accent)' } }, 'The keys change the box only here until you turn on "write box code".') : null);
  }
  const roundBox = (b) => ({ x: Math.round(b.x), y: Math.round(b.y), w: Math.round(b.w), h: Math.round(b.h) });

  function drawSide() {
    updateHead();
    const s = pat.soul;
    const onC = () => changed(false);
    add(clear(side), head,
      h('div', { class: 'card' }, h('h3', null, 'Wave'),
        h('div', { class: 'fields' },
          fieldWrap('length (steps)', numIn(pat, 'duration', onC, { int: true, min: 1 }), '30 steps = 1 second'),
          fieldWrap('bullet object', h('input', { type: 'text', value: pat.bullet_object, list: assetDatalist(capi, ch, 'objects'), style: { width: '170px' }, oninput: (e) => { pat.bullet_object = e.target.value.trim(); updateHead(); } }),
            'Object made for each bullet. obj_collidebullet hurts the soul in Deltarune.')),
        h('textarea', { rows: 2, placeholder: 'Notes', style: { width: '100%', marginTop: '6px' }, value: pat.notes, oninput: (e) => { pat.notes = e.target.value; updateHead(); } })),
      boxCard(),
      h('div', { class: 'row' }, h('h3', { class: 'grow' }, `Emitters (${pat.emitters.length})`),
        h('select', { class: 'small', onchange: (e) => {
          const v = e.target.value;
          e.target.value = '';
          if (!v) return;
          pat.emitters.push({ ...DEFAULT_EMITTER, start: 0, end: pat.duration, ...(PRESETS[v] || {}) });
          changed(true);
        } }, h('option', { value: '' }, '+ Add emitter'), h('option', { value: 'Empty' }, 'Default'), Object.keys(PRESETS).map((k) => h('option', { value: k }, k)))),
      pat.emitters.map(emitterCard),
      h('div', { class: 'card' }, h('h3', null, 'Soul (test only)'),
        h('div', { class: 'fields' },
          fieldWrap('speed', numIn(s, 'speed', onC, { min: 0 })), fieldWrap('slow', numIn(s, 'slow', onC, { min: 0 })),
          fieldWrap('hit radius', numIn(s, 'radius', onC, { min: 0 })), fieldWrap('graze radius', numIn(s, 'graze', onC, { min: 0 })),
          fieldWrap('i-frames', numIn(s, 'iframes', onC, { int: true, min: 0 })), fieldWrap('HP', numIn(s, 'hp', onC, { int: true, min: 1 })))),
      h('p', { class: 'small dim' }, 'The test uses circles for hits. The game uses the sprite masks of the bullet object, and the damage and soul rules of the battle.'));
  }
  drawSide();

  // ---- Drawing ----
  const g = canvas.getContext('2d');
  const images = new Map();
  const img = (spr) => {
    if (!spr || !sprIdx[spr]) return null;
    if (!images.has(spr)) { images.set(spr, null); loadImage(capi.file('sprites', spr, `${spr}_0.png`)).then((im) => images.set(spr, im || false)); }
    return images.get(spr) || null;
  };
  const objSpr = () => objIdx[pat.bullet_object]?.Sprite || null;
  let trail = null;

  function drawHeart(x, y, alpha) {
    g.save();
    g.globalAlpha = alpha;
    const im = heartSpr && img(heartSpr);
    if (im) {
      const s = sprIdx[heartSpr];
      g.drawImage(im, x - s.Width, y - s.Height, s.Width * 2, s.Height * 2);
    } else {
      g.fillStyle = '#ff0000';
      g.beginPath();
      g.moveTo(x, y + 8);
      g.bezierCurveTo(x - 12, y - 1, x - 8, y - 10, x, y - 4);
      g.bezierCurveTo(x + 8, y - 10, x + 12, y - 1, x, y + 8);
      g.fill();
    }
    g.restore();
  }

  function paint() {
    const r = wrap.getBoundingClientRect();
    const z = Math.max(0.25, Math.min(r.width / SCREEN_W, r.height / SCREEN_H));
    const w = Math.floor(SCREEN_W * z), hh = Math.floor(SCREEN_H * z);
    if (canvas.width !== w || canvas.height !== hh) { canvas.width = w; canvas.height = hh; trail = null; }
    canvas.style.left = `${Math.floor((r.width - w) / 2)}px`;
    canvas.style.top = `${Math.floor((r.height - hh) / 2)}px`;
    g.setTransform(1, 0, 0, 1, 0, 0);
    if (view.trails && trail) {
      g.fillStyle = 'rgba(0,0,0,0.25)';
      g.fillRect(0, 0, w, hh);
    } else {
      g.fillStyle = '#000';
      g.fillRect(0, 0, w, hh);
    }
    trail = view.trails;
    g.setTransform(z, 0, 0, z, 0, 0);
    g.imageSmoothingEnabled = false;
    const b = sim.box;
    // Box: green border outside the inner area, as in Deltarune.
    g.fillStyle = '#000';
    g.fillRect(b.x - b.w / 2, b.y - b.h / 2, b.w, b.h);
    g.strokeStyle = '#00c000';
    g.lineWidth = 6;
    g.strokeRect(b.x - b.w / 2 - 3, b.y - b.h / 2 - 3, b.w + 6, b.h + 6);
    for (const bl of sim.bullets) {
      const e = pat.emitters[bl.em];
      const spr = e.sprite || objSpr();
      const im = img(spr);
      g.save();
      g.translate(bl.x, bl.y);
      if (im) {
        const s = sprIdx[spr];
        if (bl.angle) g.rotate(-bl.angle * Math.PI / 180);
        g.scale(e.scale, e.scale);
        g.drawImage(im, -s.OriginX, -s.OriginY);
      } else {
        g.fillStyle = e.color || '#fff';
        g.beginPath();
        g.arc(0, 0, Math.max(1, e.size), 0, Math.PI * 2);
        g.fill();
      }
      g.restore();
      if (view.hitboxes) {
        g.strokeStyle = bl.grazed ? '#ffd400' : 'rgba(255,90,90,0.9)';
        g.lineWidth = 1 / z;
        g.beginPath();
        g.arc(bl.x, bl.y, e.size, 0, Math.PI * 2);
        g.stroke();
      }
    }
    const s = sim.soul;
    drawHeart(s.x, s.y, s.inv > 0 && Math.floor(s.inv / 3) % 2 ? 0.35 : 1);
    if (view.hitboxes) {
      g.lineWidth = 1 / z;
      g.strokeStyle = '#4ade80';
      g.beginPath(); g.arc(s.x, s.y, pat.soul.radius, 0, Math.PI * 2); g.stroke();
      g.strokeStyle = 'rgba(74,222,128,0.35)';
      g.beginPath(); g.arc(s.x, s.y, pat.soul.radius + pat.soul.graze, 0, Math.PI * 2); g.stroke();
    }
    if (view.origins) {
      g.lineWidth = 1 / z;
      g.font = `${10}px monospace`;
      pat.emitters.forEach((e, i) => {
        if (!e.enabled) return;
        const o = e.origin === 'soul' ? s : e.origin === 'screen' ? { x: 0, y: 0 } : b;
        const x = o.x + e.x, y = o.y + e.y;
        const active = sim.t >= e.start && sim.t <= e.end;
        g.strokeStyle = active ? '#36c5f0' : 'rgba(54,197,240,0.35)';
        g.beginPath(); g.moveTo(x - 5, y); g.lineTo(x + 5, y); g.moveTo(x, y - 5); g.lineTo(x, y + 5); g.stroke();
        if (e.radius) { g.setLineDash([3, 3]); g.beginPath(); g.arc(x, y, Math.abs(e.radius), 0, Math.PI * 2); g.stroke(); g.setLineDash([]); }
        g.fillStyle = g.strokeStyle;
        g.fillText(`${i}`, x + 6, y - 6);
      });
    }
    // HUD in screen space
    g.fillStyle = '#fff';
    g.font = '12px monospace';
    g.fillText(`HP ${sim.hp}/${pat.soul.hp}`, 8, SCREEN_H - 24);
    g.fillText(`hits ${sim.hits}  grazes ${sim.grazes}`, 8, SCREEN_H - 8);
    if (test) { g.fillStyle = '#ffd400'; g.fillText('TEST: arrows move, X/Shift slow', SCREEN_W - 230, SCREEN_H - 8); }
    scrub.value = sim.t;
    frameLabel.textContent = `step ${sim.t}/${pat.duration} · ${(sim.t / 30).toFixed(2)} s`;
    stats.textContent = `bullets on screen ${sim.bullets.length} · made ${sim.spawned} · box ${Math.round(b.w)}×${Math.round(b.h)} at ${Math.round(b.x)},${Math.round(b.y)}`;
  }

  function frame(t) {
    if (!running) return;
    requestAnimationFrame(frame);
    const dt = Math.min(200, t - (last || t));
    last = t;
    if (playing) {
      acc += dt * speed;
      while (acc >= 1000 / 30) {
        acc -= 1000 / 30;
        if (sim.done) {
          if (loop) restart();
          else { setPlaying(false); setTest(false); break; }
        }
        sim.step(test ? input() : null);
      }
    }
    paint();
  }
  requestAnimationFrame(frame);

  // ---- Keys (only while the stage has focus, so typing in fields works) ----
  const KEY = { ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down', x: 'slow', X: 'slow', Shift: 'slow' };
  const onKey = (e) => {
    if (document.activeElement !== canvas) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const k = KEY[e.key];
    if (k) { if (e.type === 'keydown') keys.add(k); else keys.delete(k); e.preventDefault(); return; }
    if (e.type !== 'keydown') return;
    const lk = e.key.toLowerCase();
    if (lk === 't') setTest(!test);
    else if (lk === 'r') restart();
    else if (e.key === ' ') setPlaying(!playing);
    else if (e.key === '.') { setPlaying(false); stepOnce(); }
    else return;
    e.preventDefault();
  };
  canvas.addEventListener('keydown', onKey);
  canvas.addEventListener('keyup', onKey);
  canvas.addEventListener('blur', () => keys.clear());
  canvas.addEventListener('pointerdown', () => canvas.focus());
  if (name && !list.some((p) => p.name === name)) toast(`Pattern ${name} is not saved yet.`);

  return () => {
    running = false;
    document.removeEventListener('keydown', onSaveKey);
  };
}

