// Room preview: walks Kris through a room in the browser, without the game.
// Solid objects block, triggers show what they run, doors go to the next room, Z reads an object's dialogue.
// The preview reads the same room data as the editor, so unsaved edits show at once.
import { h, toast, errMsg, add } from '../ui.js';
import { roomRenderer } from './roomdraw.js';
import { textPreview } from './textpreview.js';
import {
  isSolidObject, isTriggerInstance, instBBox, spriteBBox, overlaps, moveBox, doorLetter, roomTargets, playerSprites, guessDark,
} from './playcore.js';

const OBJ_FIELDS = ['Sprite', 'Visible', 'ParentObject', 'Depth', 'Solid', 'MaskSprite'];
const SPR_FIELDS = ['Width', 'Height', 'OriginX', 'OriginY', 'TextureCount', 'MarginLeft', 'MarginRight', 'MarginTop', 'MarginBottom', 'Extra'];
const KEYS = {
  ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down', a: 'left', d: 'right', w: 'up', s: 'down',
  x: 'run', Shift: 'run', z: 'act', Enter: 'act', c: 'menu',
};

const indexCache = new WeakMap();
async function playIndex(capi) {
  if (!indexCache.has(capi)) {
    indexCache.set(capi, Promise.all([
      capi.index('objects', OBJ_FIELDS),
      capi.index('sprites', SPR_FIELDS),
      capi.index('backgrounds', ['Tileset']),
      capi.list('rooms', { limit: 100000 }),
    ]).then(([objIdx, sprIdx, bgIdx, rooms]) => ({ objIdx, sprIdx, bgIdx, rooms: new Set(rooms.items.map((r) => r.name)) })));
  }
  return indexCache.get(capi);
}

/**
 * Makes the preview in `host`. `room`: {name, data} (data can be the editor's live object).
 * `codeEdits`: unsaved code from the editor (name -> source). Returns {el, dispose, focus}.
 */
export async function createPlayer(host, { capi, room, codeEdits = new Map(), onRoomChange }) {
  const idx = await playIndex(capi);
  const { objIdx, sprIdx } = idx;
  const editorRoom = room;
  const codeCache = new Map();
  const readCode = async (name) => {
    if (!name) return '';
    if (codeEdits.has(name)) return codeEdits.get(name);
    if (!codeCache.has(name)) codeCache.set(name, capi.read('code', name).then((c) => c.source || '', () => ''));
    return codeCache.get(name);
  };
  const objCache = new Map();
  const readObject = (name) => {
    if (!objCache.has(name)) objCache.set(name, capi.read('objects', name).then((o) => o.data, () => null));
    return objCache.get(name);
  };

  // ---- State ----
  let cur = null; // {name, data, R, solids, triggers}
  const opts = { dark: guessDark(room.name), walk: 0, run: 0, showTriggers: true, showSolids: false, followDoors: true, animate: false, noclip: false };
  const player = { x: 0, y: 0, dir: 'down', frame: 0, box: [0, 0, 0, 0] };
  const keys = new Set();
  let touching = new Set();
  let paused = false, running = true, last = 0, acc = 0, fpsT = 0, fpsN = 0, fps = 0, steps = 0;
  let dialogue = null; // {lines, i}

  // ---- Layout ----
  const view = h('canvas', { tabindex: 0 });
  const wrap = h('div', { class: 'canvas-wrap play-canvas' }, view);
  const hud = h('div', { class: 'status' });
  const log = h('div', { class: 'play-log' });
  const dlg = h('div', { class: 'play-dlg', hidden: true });
  wrap.append(dlg);
  const num = (label, key, title) => h('label', { class: 'field small', title }, h('span', null, label),
    h('input', { type: 'number', min: 0, step: 0.5, value: opts[key], style: { width: '56px' }, oninput: (e) => { opts[key] = +e.target.value || 0; } }));
  const chk = (label, key, title, onC) => h('label', { class: 'field check small', title }, h('input', { type: 'checkbox', checked: opts[key], onchange: (e) => { opts[key] = e.target.checked; onC?.(); } }), h('span', null, label));
  const pauseBtn = h('button', { class: 'small', onclick: () => setPaused(!paused) }, 'Pause');
  const roomLabel = h('b', { class: 'mono' });
  const toolbar = h('div', { class: 'toolbar' },
    roomLabel,
    h('button', { class: 'small', title: 'Back to the first room and spawn point (R)', onclick: () => restart() }, 'Restart'),
    pauseBtn,
    h('span', { class: 'sep' }),
    chk('dark world', 'dark', 'Kris drawn at 2x with the _dark sprites', () => setPlayerScale()),
    num('walk', 'walk', 'Pixels per step. 0 = default (dark 4, light 2)'),
    num('run', 'run', 'Pixels per step with X or Shift. 0 = default (dark 8, light 4)'),
    chk('triggers', 'showTriggers', 'Show trigger boxes'),
    chk('solids', 'showSolids', 'Show solid boxes'),
    chk('follow doors', 'followDoors', 'Go to the next room when Kris touches a door'),
    chk('animate', 'animate', 'Play sprite animations at the sprite speed (the game often sets image_speed in code)'),
    chk('no clip', 'noclip', 'Walk through solids'),
  );
  const el = h('div', { class: 'play' }, toolbar,
    h('div', { class: 'row', style: { flex: 1, minHeight: 0, alignItems: 'stretch', flexWrap: 'nowrap', gap: 0 } },
      h('div', { class: 'col grow', style: { gap: 0, minHeight: 0 } }, wrap, hud),
      h('div', { class: 'side play-side' },
        h('p', { class: 'dim small' }, 'Arrows or WASD walk, X or Shift runs, Z or Enter talks, click to put Kris there, R restarts.'),
        h('h3', null, 'Events'), log)));
  host.append(el);

  const g = view.getContext('2d');

  // ---- Room loading ----
  function speedWalk() { return opts.walk || (opts.dark ? 4 : 2); }
  function speedRun() { return opts.run || (opts.dark ? 8 : 4); }
  const scale = () => (opts.dark ? 2 : 1);
  const sprites = () => playerSprites(sprIdx, opts.dark);

  function playerBox(x, y) {
    // Kris collides with the feet: the lower part of the sprite box, as in obj_mainchara.
    const s = sprIdx[sprites().down];
    const k = scale();
    const b = s ? spriteBBox(s, x, y, k, k) : [x, y, x + 16 * k, y + 32 * k];
    const hgt = b[3] - b[1];
    return [b[0] + 2 * k, b[3] - Math.max(6 * k, hgt * 0.3), b[2] - 2 * k, b[3]];
  }
  function setPlayerScale() {
    player.box = playerBox(player.x, player.y);
  }

  function enterRoom(name, data, entrance) {
    const R = name === editorRoom.name && data === editorRoom.data ? editorRenderer() : roomRenderer(capi, data, idx, () => {});
    cur = { name, data, R };
    rebuild();
    const insts = data.GameObjects || [];
    let spot = insts.find((i) => /^obj_mainchara$/.test(i.ObjectName));
    if (!spot && entrance) spot = insts.find((i) => i.ObjectName === `obj_marker${entrance}`);
    if (!spot) spot = insts.find((i) => /^obj_marker/.test(i.ObjectName));
    const v = mainView(data);
    player.x = spot ? spot.X : v.x + v.w / 2;
    player.y = spot ? spot.Y : v.y + v.h / 2;
    player.dir = 'down';
    setPlayerScale();
    // Do not start on a door: wait until Kris leaves the boxes under the spawn point.
    touching = new Set(cur.triggers.filter((t) => overlaps(t.box, player.box)).map((t) => t.i));
    roomLabel.textContent = name;
    logLine(`Room ${name}${entrance ? ` (entrance ${entrance})` : ''}`, 'room');
    onRoomChange?.(name);
    if (data.CreationCodeEntry) readCode(data.CreationCodeEntry).then((src) => src && logCode('Room creation code', data.CreationCodeEntry, src));
  }
  let editorR = null;
  const editorRenderer = () => (editorR ||= roomRenderer(capi, editorRoom.data, idx, () => {}));

  /** Solid and trigger boxes. Made again each step from the live data, so moved instances count at once. */
  function rebuild() {
    const insts = cur.data.GameObjects || [];
    cur.solids = [];
    cur.triggers = [];
    insts.forEach((inst, i) => {
      if (!inst) return;
      if (isSolidObject(inst.ObjectName, objIdx)) cur.solids.push(instBBox(inst, objIdx, sprIdx));
      else if (isTriggerInstance(inst, objIdx)) cur.triggers.push({ i, inst, box: instBBox(inst, objIdx, sprIdx) });
    });
  }

  function mainView(data) {
    const v = (data.Views || []).find((x) => x.Enabled && x.ViewWidth > 0);
    if (v) return { x: v.ViewX, y: v.ViewY, w: v.ViewWidth, h: v.ViewHeight };
    return { x: 0, y: 0, w: Math.min(data.Width, 640), h: Math.min(data.Height, 480) };
  }

  async function goRoom(target, entrance) {
    if (target === editorRoom.name) { enterRoom(editorRoom.name, editorRoom.data, entrance); return; }
    try {
      const a = await capi.read('rooms', target);
      a.data.GameObjects ||= [];
      enterRoom(target, a.data, entrance);
    } catch (e) {
      toast(`${target}: ${errMsg(e)}`, 'error');
    }
  }

  function restart() {
    dialogue = null;
    dlg.hidden = true;
    enterRoom(editorRoom.name, editorRoom.data, null);
  }

  // ---- Events ----
  function logLine(text, kind = '', extra = null) {
    const row = h('div', { class: `play-ev ${kind}` }, h('span', { class: 'dim mono' }, `${String(steps).padStart(5)} `), text, extra);
    log.prepend(row);
    while (log.childElementCount > 80) log.lastChild.remove();
    return row;
  }
  function logCode(title, name, src) {
    logLine(title, 'code', h('details', null, h('summary', { class: 'mono small' }, name), h('pre', { class: 'mono small' }, src.trim().slice(0, 4000))));
  }

  async function onTouch(t) {
    const { inst } = t;
    const o = inst.ObjectName;
    const src = await readCode(inst.CreationCodeEntry);
    const objData = await readObject(o);
    const objCode = (await Promise.all((objData?.Events || []).filter((e) => /Collision|Step|Create|Other/.test(e.Category)).map((e) => readCode(e.CodeEntry)))).join('\n');
    const targets = roomTargets(`${src}\n${objCode}`, cur.name, idx.rooms);
    const letter = doorLetter(o);
    const go = (r) => h('button', { class: 'small', onclick: () => goRoom(r, letter) }, `Go to ${r}`);
    logLine(`Touched ${o} at ${inst.X},${inst.Y}`, 'trigger', h('span', null, ' ', targets.map(go)));
    if (src) logCode('Creation code', inst.CreationCodeEntry, src);
    if (letter && targets.length === 1 && opts.followDoors) goRoom(targets[0], letter);
    else if (letter && !targets.length) logLine(`No target room found in the code of ${o}. The game sets it at run time.`, 'dim');
  }

  async function interact() {
    if (dialogue) { nextLine(); return; }
    // Check box: the sprite box of Kris, made longer in the facing direction.
    const reach = 16 * scale();
    const s = sprIdx[sprites().down];
    const [x1, y1, x2, y2] = s ? spriteBBox(s, player.x, player.y, scale(), scale()) : player.box;
    const d = { left: [-reach, 0], right: [reach, 0], up: [0, -reach], down: [0, reach] }[player.dir];
    const probe = [x1 + Math.min(0, d[0]), y1 + Math.min(0, d[1]), x2 + Math.max(0, d[0]), y2 + Math.max(0, d[1])];
    const insts = cur.data.GameObjects || [];
    const hit = insts.map((inst, i) => ({ inst, i })).find(({ inst }) => inst && overlaps(instBBox(inst, objIdx, sprIdx), probe));
    if (!hit) return;
    const o = hit.inst.ObjectName;
    const objData = await readObject(o);
    const codes = [hit.inst.CreationCodeEntry, ...(objData?.Events || []).map((e) => e.CodeEntry)].filter(Boolean);
    const lines = [];
    for (const code of codes) {
      const r = await capi.dialogue({ code, limit: 50 }).catch(() => ({ items: [] }));
      lines.push(...r.items.filter((l) => l.text));
    }
    logLine(`Talked to ${o}: ${lines.length} text line(s)`, 'talk');
    if (!lines.length) return;
    dialogue = { lines, i: 0 };
    showLine();
  }
  function showLine() {
    const l = dialogue.lines[dialogue.i];
    dlg.replaceChildren(h('div', { class: 'dim small' }, `${l.speaker ? `${l.speaker} · ` : ''}${dialogue.i + 1}/${dialogue.lines.length} · ${l.code}:${l.line}`), textPreview(l.text));
    dlg.hidden = false;
  }
  function nextLine() {
    dialogue.i++;
    if (dialogue.i >= dialogue.lines.length) { dialogue = null; dlg.hidden = true; return; }
    showLine();
  }

  // ---- Step (30 per second, as the game) ----
  function step() {
    steps++;
    rebuild();
    if (dialogue) return;
    let dx = 0, dy = 0;
    if (keys.has('left')) dx -= 1;
    if (keys.has('right')) dx += 1;
    if (keys.has('up')) dy -= 1;
    if (keys.has('down')) dy += 1;
    const sp = keys.has('run') ? speedRun() : speedWalk();
    if (dx || dy) {
      // Keep the facing while it is one of the pressed directions (diagonal walk), else face the new one.
      const h1 = dx ? (dx < 0 ? 'left' : 'right') : null, v1 = dy ? (dy < 0 ? 'up' : 'down') : null;
      if (player.dir !== h1 && player.dir !== v1) player.dir = v1 || h1;
      const b = playerBox(player.x, player.y);
      const [mx, my] = opts.noclip ? [dx * sp, dy * sp] : moveBox(b, dx * sp, dy * sp, cur.solids);
      player.x += mx;
      player.y += my;
      // Stay in the room.
      const nb = playerBox(player.x, player.y);
      player.x += Math.max(0, -nb[0]) - Math.max(0, nb[2] - cur.data.Width);
      player.y += Math.max(0, -nb[1]) - Math.max(0, nb[3] - cur.data.Height);
      player.frame += keys.has('run') ? 0.33 : 0.2;
    } else {
      player.frame = 0;
    }
    player.box = playerBox(player.x, player.y);
    const now = new Set();
    for (const t of cur.triggers) {
      if (!overlaps(t.box, player.box)) continue;
      now.add(t.i);
      if (!touching.has(t.i)) onTouch(t);
    }
    touching = now;
  }

  // ---- Drawing ----
  function instFrame(inst) {
    if (!opts.animate) return inst.ImageIndex || 0;
    const spr = objIdx[inst.ObjectName]?.Sprite;
    const ex = sprIdx[spr]?.Extra;
    const roomSpeed = cur.data.Speed || 30;
    const perStep = ex ? (ex.PlaybackSpeedType === 1 ? ex.PlaybackSpeed ?? 1 : (ex.PlaybackSpeed ?? 15) / roomSpeed) : 1;
    return (inst.ImageIndex || 0) + steps * perStep * (inst.ImageSpeed ?? 1);
  }

  function drawPlayer() {
    const spr = sprites()[player.dir] || sprites().down;
    const k = scale();
    if (!spr || !cur.R.drawSprite(g, spr, Math.floor(player.frame) % Math.max(1, sprIdx[spr]?.TextureCount || 1), player.x, player.y, k, k)) {
      g.fillStyle = '#36c5f0';
      g.fillRect(player.box[0], player.box[1] - 24 * k, player.box[2] - player.box[0], player.box[3] - player.box[1] + 24 * k);
    }
  }

  function paint() {
    const r = wrap.getBoundingClientRect();
    if (view.width !== Math.floor(r.width) || view.height !== Math.floor(r.height)) { view.width = Math.floor(r.width); view.height = Math.floor(r.height); }
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.fillStyle = '#000';
    g.fillRect(0, 0, view.width, view.height);
    if (!cur) return;
    const data = cur.data;
    const v = mainView(data);
    const z0 = Math.min(view.width / v.w, view.height / v.h);
    const z = z0 >= 1 ? Math.floor(z0) : z0;
    // Camera on Kris, kept in the room.
    const cx = Math.max(0, Math.min(data.Width - v.w, player.x - v.w / 2));
    const cy = Math.max(0, Math.min(data.Height - v.h, player.y - v.h / 2));
    const ox = (view.width - v.w * z) / 2, oy = (view.height - v.h * z) / 2;
    g.setTransform(z, 0, 0, z, ox - cx * z, oy - cy * z);
    g.imageSmoothingEnabled = false;
    g.save();
    g.beginPath();
    g.rect(cx, cy, v.w, v.h);
    g.clip();
    let drew = false;
    // Kris goes on the instance layer with the most instances (old rooms: the one instance pass).
    let kLayer = -1;
    (data.Layers || []).forEach((l, li) => { if (l.InstanceIndices && (kLayer < 0 || l.InstanceIndices.length > data.Layers[kLayer].InstanceIndices.length)) kLayer = li; });
    cur.R.drawScene(g, new Set(), (list, li) => {
      // Instances by depth, then by y (Deltarune sets depth from y for most objects), Kris among them.
      const items = list.map((i) => ({ i, y: data.GameObjects[i].Y, d: objIdx[data.GameObjects[i].ObjectName]?.Depth || 0 }));
      if (!drew && li === kLayer) { items.push({ i: -1, y: player.y, d: 0 }); drew = true; }
      items.sort((a, b) => b.d - a.d || a.y - b.y);
      for (const it of items) {
        if (it.i < 0) { drawPlayer(); continue; }
        const inst = data.GameObjects[it.i];
        if (!cur.R.objVisible(inst.ObjectName)) continue;
        const spr = cur.R.objSprite(inst.ObjectName);
        if (spr) cur.R.drawSprite(g, spr, instFrame(inst), inst.X, inst.Y, inst.ScaleX, inst.ScaleY, inst.Rotation, inst.Color);
      }
    });
    if (!drew) drawPlayer();
    g.lineWidth = 1 / z;
    if (opts.showSolids) {
      g.strokeStyle = 'rgba(255,90,90,0.8)';
      for (const s of cur.solids) g.strokeRect(s[0], s[1], s[2] - s[0], s[3] - s[1]);
      g.strokeStyle = '#4ade80';
      g.strokeRect(player.box[0], player.box[1], player.box[2] - player.box[0], player.box[3] - player.box[1]);
    }
    if (opts.showTriggers) {
      for (const t of cur.triggers) {
        const on = touching.has(t.i);
        g.fillStyle = on ? 'rgba(255,212,0,0.25)' : 'rgba(255,0,200,0.12)';
        g.strokeStyle = on ? '#ffd400' : '#ff4fd8';
        g.fillRect(t.box[0], t.box[1], t.box[2] - t.box[0], t.box[3] - t.box[1]);
        g.strokeRect(t.box[0], t.box[1], t.box[2] - t.box[0], t.box[3] - t.box[1]);
      }
    }
    g.restore();
    hud.textContent = `${cur.name}  x ${Math.round(player.x)} y ${Math.round(player.y)}  view ${v.w}×${v.h} at ${z.toFixed(2)}×  ${fps} fps${paused ? '  PAUSED' : ''}`;
  }

  function frame(t) {
    if (!running) return;
    requestAnimationFrame(frame);
    const dt = Math.min(250, t - (last || t));
    last = t;
    fpsT += dt; fpsN++;
    if (fpsT >= 1000) { fps = Math.round((fpsN * 1000) / fpsT); fpsT = 0; fpsN = 0; }
    if (!paused && cur) {
      acc += dt;
      const ms = 1000 / (cur.data.Speed || 30);
      while (acc >= ms) { step(); acc -= ms; }
    }
    paint();
  }

  function setPaused(p) {
    paused = p;
    pauseBtn.textContent = p ? 'Resume' : 'Pause';
  }

  // ---- Input ----
  const isField = (t) => t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement || t instanceof HTMLSelectElement;
  let active = true;
  const onDown = (e) => { active = el.contains(e.target); };
  const onKey = (e) => {
    if (!active || isField(e.target)) return;
    const k = KEYS[e.key] || KEYS[e.key.toLowerCase()];
    if (e.ctrlKey || e.metaKey || e.altKey || e.key === 'Escape') return;
    if (e.type === 'keydown' && e.key.toLowerCase() === 'r') restart();
    else if (k === 'act') { if (e.type === 'keydown' && !e.repeat) interact(); }
    else if (k) {
      if (e.type === 'keydown') keys.add(k); else keys.delete(k);
      // Turn at once on a key press, also when the press is shorter than one step.
      if (e.type === 'keydown' && !dialogue && ['left', 'right', 'up', 'down'].includes(k)) player.dir = k;
    }
    // While the preview has focus, editor shortcuts (Delete, V, A, ...) do not run.
    e.preventDefault();
    e.stopPropagation();
  };
  window.addEventListener('keydown', onKey, true);
  window.addEventListener('keyup', onKey, true);
  document.addEventListener('pointerdown', onDown, true);
  const onBlur = () => keys.clear();
  window.addEventListener('blur', onBlur);
  view.addEventListener('pointerdown', (e) => {
    if (!cur) return;
    const r = view.getBoundingClientRect();
    const v = mainView(cur.data);
    const z0 = Math.min(view.width / v.w, view.height / v.h);
    const z = z0 >= 1 ? Math.floor(z0) : z0;
    const cx = Math.max(0, Math.min(cur.data.Width - v.w, player.x - v.w / 2));
    const cy = Math.max(0, Math.min(cur.data.Height - v.h, player.y - v.h / 2));
    const ox = (view.width - v.w * z) / 2, oy = (view.height - v.h * z) / 2;
    player.x = Math.round((e.clientX - r.left - ox) / z + cx);
    player.y = Math.round((e.clientY - r.top - oy) / z + cy);
    setPlayerScale();
    view.focus();
  });

  enterRoom(room.name, room.data, null);
  requestAnimationFrame(frame);
  view.focus();

  return {
    el,
    /** Call after the editor changes tile data, so that the preview draws it again. */
    invalidate: () => editorR?.layerCache.clear(),
    dispose: () => {
      running = false;
      window.removeEventListener('keydown', onKey, true);
      window.removeEventListener('keyup', onKey, true);
      window.removeEventListener('blur', onBlur);
      document.removeEventListener('pointerdown', onDown, true);
      el.remove();
    },
  };
}

/** Opens the preview in a window over the room editor. The editor stays usable: edits show in the preview at once. */
export async function openPlay({ capi, name, data, codeEdits, onExit }) {
  document.querySelector('.play-window')?.dispatchEvent(new Event('close'));
  const body = h('div', { class: 'play-body' });
  const title = h('div', { class: 'play-title' }, h('b', null, 'Room preview'), h('span', { class: 'dim small' }, ' drag to move, drag the corner to resize'),
    h('div', { class: 'grow' }),
    h('button', { class: 'small', title: 'Full size', onclick: () => win.classList.toggle('max') }, '⤢'),
    h('button', { class: 'small', title: 'Close (Esc)', onclick: () => close() }, '✕'));
  const win = h('div', { class: 'play-window' }, title, body);
  document.body.append(win);
  let player;
  const close = () => {
    player?.dispose();
    win.remove();
    window.removeEventListener('keydown', onEsc, true);
    onExit?.();
  };
  const onEsc = (e) => { if (e.key === 'Escape' && win.contains(document.activeElement)) { e.stopPropagation(); close(); } };
  window.addEventListener('keydown', onEsc, true);
  win.addEventListener('close', close);
  let drag = null;
  title.addEventListener('pointerdown', (e) => {
    if (e.target.closest('button')) return;
    const r = win.getBoundingClientRect();
    drag = { dx: e.clientX - r.left, dy: e.clientY - r.top };
    title.setPointerCapture(e.pointerId);
  });
  title.addEventListener('pointermove', (e) => {
    if (!drag) return;
    win.style.left = `${Math.max(0, e.clientX - drag.dx)}px`;
    win.style.top = `${Math.max(0, e.clientY - drag.dy)}px`;
    win.style.right = 'auto';
    win.style.bottom = 'auto';
  });
  title.addEventListener('pointerup', () => { drag = null; });
  try {
    player = await createPlayer(body, { capi, room: { name, data }, codeEdits });
  } catch (e) {
    add(body, h('div', { class: 'error-box' }, errMsg(e)));
  }
  return { close, invalidate: () => player?.invalidate() };
}
