// Sprite editor: frames, pixel tools, origin, bounding box, collision masks, animation preview.
import { h, attempt, modal, toast, pickFiles, download, forgetImages, numField, selectField, boolField, add } from '../ui.js';

const TOOLS = [
  ['pencil', 'Pencil (B)', 'b'], ['eraser', 'Eraser (E)', 'e'], ['fill', 'Fill (G)', 'g'], ['picker', 'Pick colour (I)', 'i'], ['origin', 'Set origin (O)', 'o'], ['pan', 'Pan (Space)', ' '],
];

function newCanvas(w, hgt) {
  const c = document.createElement('canvas');
  c.width = w;
  c.height = hgt;
  return c;
}

function cloneCanvas(src, w = src.width, hgt = src.height, dx = 0, dy = 0) {
  const c = newCanvas(w, hgt);
  c.getContext('2d').drawImage(src, dx, dy);
  return c;
}

const toBlob = (c) => new Promise((r) => c.toBlob(r, 'image/png'));

export async function open(el, ectx) {
  const { capi, name, asset, setDirty } = ectx;
  const data = structuredClone(asset.data);
  let W = data.Width, H = data.Height;
  let oldW = W, oldH = H, oldCount = data.TextureCount;
  let structureChanged = false;
  let pixelsChanged = false;

  // ---- Load frames ----
  const frames = [];
  for (let i = 0; i < data.TextureCount; i++) {
    const c = newCanvas(W, H);
    const f = `${name}_${i}.png`;
    if (asset.files.includes(f)) {
      const blob = await (await capi.readFile('sprites', name, f)).blob();
      const bmp = await createImageBitmap(blob);
      c.getContext('2d').drawImage(bmp, 0, 0);
    }
    frames.push({ canvas: c, undo: [], redo: [], dirty: false });
  }
  if (!frames.length) frames.push({ canvas: newCanvas(W, H), undo: [], redo: [], dirty: true });
  let maskDims = null;
  if (asset.files.includes(`${name}_0.mask.png`)) {
    const bmp = await createImageBitmap(await (await capi.readFile('sprites', name, `${name}_0.mask.png`)).blob());
    maskDims = [bmp.width, bmp.height];
  }

  // ---- State ----
  let cur = 0, tool = 'pencil', zoom = Math.max(1, Math.min(16, Math.floor(480 / Math.max(W, H)))), panX = 20, panY = 20;
  let color = '#ffffff', alpha = 255, brush = 1;
  const opts = { grid: true, onion: false, bbox: true, origin: true };
  const changed = () => { setDirty(true); };

  // ---- Layout ----
  const view = h('canvas');
  const wrap = h('div', { class: 'canvas-wrap' }, view);
  const status = h('div', { class: 'status' });
  const strip = h('div', { class: 'frames' });
  const side = h('div', { class: 'side' });
  const toolBtns = {};
  const colorIn = h('input', { type: 'color', value: color, oninput: (e) => { color = e.target.value; } });
  const alphaIn = h('input', { type: 'range', min: 0, max: 255, value: alpha, title: 'Alpha', oninput: (e) => { alpha = +e.target.value; } });
  const toolbar = h('div', { class: 'toolbar' },
    TOOLS.map(([id, label]) => (toolBtns[id] = h('button', { class: 'small', title: label, onclick: () => setTool(id) }, label.split(' (')[0]))),
    h('span', { class: 'sep' }), colorIn, alphaIn,
    h('label', { class: 'small dim' }, 'Size ', h('input', { type: 'number', min: 1, max: 16, value: 1, style: { width: '50px' }, oninput: (e) => { brush = Math.max(1, +e.target.value || 1); } })),
    h('span', { class: 'sep' }),
    h('button', { class: 'small', title: 'Zoom out (-)', onclick: () => setZoom(zoom / 2) }, '−'),
    h('button', { class: 'small', title: 'Zoom in (+)', onclick: () => setZoom(zoom * 2) }, '+'),
    ...['grid', 'onion', 'bbox', 'origin'].map((k) => h('label', { class: 'field check small' }, h('input', { type: 'checkbox', checked: opts[k], onchange: (e) => { opts[k] = e.target.checked; paint(); } }), h('span', null, k))),
    h('span', { class: 'sep' }),
    h('button', { class: 'small', title: 'Undo (Ctrl+Z)', onclick: () => undo() }, 'Undo'),
    h('button', { class: 'small', title: 'Redo (Ctrl+Y)', onclick: () => redo() }, 'Redo'),
    h('button', { class: 'small', onclick: () => flip(true) }, 'Flip H'),
    h('button', { class: 'small', onclick: () => flip(false) }, 'Flip V'),
    h('button', { class: 'small', onclick: () => resizeDialog() }, 'Resize…'),
    h('button', { class: 'small', onclick: () => importFrames() }, 'Import PNG…'),
    h('button', { class: 'small', onclick: () => exportStrip() }, 'Export strip'),
  );
  el.append(h('div', { class: 'full', style: { height: 'calc(100vh - 150px)' } }, toolbar,
    h('div', { class: 'row', style: { flex: 1, minHeight: 0, alignItems: 'stretch', flexWrap: 'nowrap', gap: 0 } },
      h('div', { class: 'col grow', style: { gap: 0, minHeight: 0 } }, wrap, status, strip), side)));

  // ---- Painting the view ----
  const vg = view.getContext('2d');
  const checker = (() => {
    const c = newCanvas(16, 16), g = c.getContext('2d');
    g.fillStyle = '#2a2a2a'; g.fillRect(0, 0, 16, 16);
    g.fillStyle = '#3a3a3a'; g.fillRect(0, 0, 8, 8); g.fillRect(8, 8, 8, 8);
    return vg.createPattern(c, 'repeat');
  })();
  function paint() {
    const r = wrap.getBoundingClientRect();
    if (view.width !== Math.floor(r.width) || view.height !== Math.floor(r.height)) { view.width = Math.floor(r.width); view.height = Math.floor(r.height); }
    vg.imageSmoothingEnabled = false;
    vg.fillStyle = '#050508';
    vg.fillRect(0, 0, view.width, view.height);
    vg.fillStyle = checker;
    vg.fillRect(panX, panY, W * zoom, H * zoom);
    if (opts.onion && cur > 0) {
      vg.globalAlpha = 0.3;
      vg.drawImage(frames[cur - 1].canvas, panX, panY, W * zoom, H * zoom);
      vg.globalAlpha = 1;
    }
    vg.drawImage(frames[cur].canvas, panX, panY, W * zoom, H * zoom);
    if (opts.grid && zoom >= 6) {
      vg.strokeStyle = 'rgba(255,255,255,0.08)';
      vg.beginPath();
      for (let x = 0; x <= W; x++) { vg.moveTo(panX + x * zoom + 0.5, panY); vg.lineTo(panX + x * zoom + 0.5, panY + H * zoom); }
      for (let y = 0; y <= H; y++) { vg.moveTo(panX, panY + y * zoom + 0.5); vg.lineTo(panX + W * zoom, panY + y * zoom + 0.5); }
      vg.stroke();
    }
    if (opts.bbox) {
      vg.strokeStyle = '#4ade80';
      vg.setLineDash([4, 3]);
      vg.strokeRect(panX + data.MarginLeft * zoom + 0.5, panY + data.MarginTop * zoom + 0.5, (data.MarginRight - data.MarginLeft + 1) * zoom, (data.MarginBottom - data.MarginTop + 1) * zoom);
      vg.setLineDash([]);
    }
    if (opts.origin) {
      const ox = panX + data.OriginX * zoom, oy = panY + data.OriginY * zoom;
      vg.strokeStyle = '#ff5a5a';
      vg.beginPath(); vg.moveTo(ox - 8, oy + 0.5); vg.lineTo(ox + 8, oy + 0.5); vg.moveTo(ox + 0.5, oy - 8); vg.lineTo(ox + 0.5, oy + 8); vg.stroke();
    }
    vg.strokeStyle = '#555';
    vg.strokeRect(panX - 0.5, panY - 0.5, W * zoom + 1, H * zoom + 1);
  }
  const ro = new ResizeObserver(paint);
  ro.observe(wrap);

  function setZoom(z, cx = view.width / 2, cy = view.height / 2) {
    z = Math.max(1, Math.min(64, Math.round(z)));
    panX = cx - ((cx - panX) / zoom) * z;
    panY = cy - ((cy - panY) / zoom) * z;
    zoom = z;
    paint();
  }
  function setTool(t) {
    tool = t;
    for (const [k, b] of Object.entries(toolBtns)) b.classList.toggle('active', k === t);
    view.style.cursor = t === 'pan' ? 'grab' : 'crosshair';
  }
  setTool('pencil');

  // ---- Frame strip ----
  function drawStrip() {
    strip.replaceChildren();
    frames.forEach((f, i) => {
      const thumb = cloneCanvas(f.canvas);
      strip.append(h('div', { class: `frame ${i === cur ? 'sel' : ''}`, onclick: () => { cur = i; drawStrip(); paint(); } }, thumb, h('span', null, `${i}${f.dirty ? ' *' : ''}`)));
    });
    strip.append(h('div', { class: 'col', style: { justifyContent: 'center' } },
      h('button', { class: 'small', onclick: () => addFrame(false) }, '+ Blank'),
      h('button', { class: 'small', onclick: () => addFrame(true) }, '+ Copy'),
      h('button', { class: 'small danger', disabled: frames.length < 2, onclick: () => delFrame() }, 'Delete')),
    h('div', { class: 'col', style: { justifyContent: 'center' } },
      h('button', { class: 'small', disabled: cur === 0, onclick: () => moveFrame(-1) }, '← Move'),
      h('button', { class: 'small', disabled: cur === frames.length - 1, onclick: () => moveFrame(1) }, 'Move →')));
  }
  function addFrame(copy) {
    const c = copy ? cloneCanvas(frames[cur].canvas) : newCanvas(W, H);
    frames.splice(cur + 1, 0, { canvas: c, undo: [], redo: [], dirty: true });
    cur++;
    structureChanged = true;
    changed(); drawStrip(); paint();
  }
  function delFrame() {
    frames.splice(cur, 1);
    cur = Math.min(cur, frames.length - 1);
    structureChanged = true;
    changed(); drawStrip(); paint();
  }
  function moveFrame(d) {
    const [f] = frames.splice(cur, 1);
    cur += d;
    frames.splice(cur, 0, f);
    structureChanged = true;
    changed(); drawStrip(); paint();
  }

  // ---- Pixel tools ----
  const fg = () => frames[cur].canvas.getContext('2d', { willReadFrequently: true });
  const snapshot = () => {
    const f = frames[cur];
    f.undo.push(fg().getImageData(0, 0, W, H));
    if (f.undo.length > 100) f.undo.shift();
    f.redo = [];
  };
  function undo() {
    const f = frames[cur];
    if (!f.undo.length) return;
    f.redo.push(fg().getImageData(0, 0, W, H));
    fg().putImageData(f.undo.pop(), 0, 0);
    markPixels(); paint();
  }
  function redo() {
    const f = frames[cur];
    if (!f.redo.length) return;
    f.undo.push(fg().getImageData(0, 0, W, H));
    fg().putImageData(f.redo.pop(), 0, 0);
    markPixels(); paint();
  }
  function markPixels() {
    frames[cur].dirty = true;
    pixelsChanged = true;
    changed();
    drawStrip();
  }
  const rgba = () => {
    const n = parseInt(color.slice(1), 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255, alpha];
  };
  function plot(x, y) {
    const g = fg();
    const o = Math.floor((brush - 1) / 2);
    g.clearRect(x - o, y - o, brush, brush);
    if (tool === 'pencil') {
      const [r, gg, b, a] = rgba();
      g.fillStyle = `rgba(${r},${gg},${b},${a / 255})`;
      g.fillRect(x - o, y - o, brush, brush);
    }
  }
  function line(x0, y0, x1, y1) {
    const dx = Math.abs(x1 - x0), dy = -Math.abs(y1 - y0), sx = x0 < x1 ? 1 : -1, sy = y0 < y1 ? 1 : -1;
    let err = dx + dy;
    for (;;) {
      plot(x0, y0);
      if (x0 === x1 && y0 === y1) break;
      const e2 = 2 * err;
      if (e2 >= dy) { err += dy; x0 += sx; }
      if (e2 <= dx) { err += dx; y0 += sy; }
    }
  }
  function fill(x, y) {
    const g = fg();
    const img = g.getImageData(0, 0, W, H);
    const d = new Uint32Array(img.data.buffer);
    const target = d[y * W + x];
    const [r, gg, b, a] = rgba();
    const repl = new Uint32Array(new Uint8Array([r, gg, b, a]).buffer)[0];
    if (target === repl) return;
    const stack = [[x, y]];
    while (stack.length) {
      const [px, py] = stack.pop();
      if (px < 0 || py < 0 || px >= W || py >= H || d[py * W + px] !== target) continue;
      d[py * W + px] = repl;
      stack.push([px + 1, py], [px - 1, py], [px, py + 1], [px, py - 1]);
    }
    g.putImageData(img, 0, 0);
  }
  function pick(x, y) {
    const p = fg().getImageData(x, y, 1, 1).data;
    color = '#' + [p[0], p[1], p[2]].map((v) => v.toString(16).padStart(2, '0')).join('');
    alpha = p[3];
    colorIn.value = color;
    alphaIn.value = alpha;
  }
  function flip(horiz) {
    snapshot();
    const src = cloneCanvas(frames[cur].canvas);
    const g = fg();
    g.clearRect(0, 0, W, H);
    g.save();
    g.translate(horiz ? W : 0, horiz ? 0 : H);
    g.scale(horiz ? -1 : 1, horiz ? 1 : -1);
    g.drawImage(src, 0, 0);
    g.restore();
    markPixels(); paint();
  }

  const toPixel = (e) => {
    const r = view.getBoundingClientRect();
    return [Math.floor((e.clientX - r.left - panX) / zoom), Math.floor((e.clientY - r.top - panY) / zoom)];
  };
  let drag = null;
  let spaceDown = false;
  view.addEventListener('pointerdown', (e) => {
    view.setPointerCapture(e.pointerId);
    const [x, y] = toPixel(e);
    if (e.button === 1 || tool === 'pan' || spaceDown) { drag = { pan: true, sx: e.clientX - panX, sy: e.clientY - panY }; return; }
    if (e.button !== 0) return;
    const inside = x >= 0 && y >= 0 && x < W && y < H;
    if (tool === 'origin') {
      data.OriginX = x; data.OriginY = y; changed(); drawSide(); paint(); return;
    }
    if (!inside) return;
    if (tool === 'picker') { pick(x, y); return; }
    snapshot();
    if (tool === 'fill') { fill(x, y); markPixels(); paint(); return; }
    plot(x, y);
    drag = { x, y };
    markPixels(); paint();
  });
  view.addEventListener('pointermove', (e) => {
    const [x, y] = toPixel(e);
    const p = x >= 0 && y >= 0 && x < W && y < H ? fg().getImageData(x, y, 1, 1).data : null;
    status.textContent = `frame ${cur}/${frames.length}  ${W}×${H}  zoom ${zoom}×  ${p ? `x ${x} y ${y}  rgba(${p.join(',')})` : ''}`;
    if (!drag) return;
    if (drag.pan) { panX = e.clientX - drag.sx; panY = e.clientY - drag.sy; paint(); return; }
    if (x === drag.x && y === drag.y) return;
    line(drag.x, drag.y, x, y);
    drag.x = x; drag.y = y;
    paint();
  });
  view.addEventListener('pointerup', () => { if (drag && !drag.pan) drawStrip(); drag = null; });
  view.addEventListener('wheel', (e) => {
    e.preventDefault();
    const r = view.getBoundingClientRect();
    setZoom(e.deltaY < 0 ? zoom * 2 : zoom / 2, e.clientX - r.left, e.clientY - r.top);
  }, { passive: false });
  const onKey = (e) => {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLSelectElement) return;
    if ((e.ctrlKey || e.metaKey) && e.key === 'z') { e.preventDefault(); undo(); return; }
    if ((e.ctrlKey || e.metaKey) && e.key === 'y') { e.preventDefault(); redo(); return; }
    if (e.key === ' ') { spaceDown = e.type === 'keydown'; e.preventDefault(); return; }
    if (e.type !== 'keydown' || e.ctrlKey || e.metaKey) return;
    const t = TOOLS.find((x) => x[2] === e.key.toLowerCase());
    if (t && t[0] !== 'pan') setTool(t[0]);
    if (e.key === '+' || e.key === '=') setZoom(zoom * 2);
    if (e.key === '-') setZoom(zoom / 2);
    if (e.key === 'ArrowRight') { cur = (cur + 1) % frames.length; drawStrip(); paint(); }
    if (e.key === 'ArrowLeft') { cur = (cur - 1 + frames.length) % frames.length; drawStrip(); paint(); }
  };
  document.addEventListener('keydown', onKey);
  document.addEventListener('keyup', onKey);

  // ---- Resize / import / export ----
  function resizeAll(nw, nh, mode, anchor) {
    for (const f of frames) {
      const src = f.canvas;
      const c = newCanvas(nw, nh);
      const g = c.getContext('2d');
      g.imageSmoothingEnabled = false;
      if (mode === 'scale') g.drawImage(src, 0, 0, nw, nh);
      else g.drawImage(src, anchor === 'center' ? Math.floor((nw - W) / 2) : 0, anchor === 'center' ? Math.floor((nh - H) / 2) : 0);
      f.canvas = c; f.undo = []; f.redo = []; f.dirty = true;
    }
    if (mode === 'scale') { data.OriginX = Math.round(data.OriginX * nw / W); data.OriginY = Math.round(data.OriginY * nh / H); }
    else if (anchor === 'center') { data.OriginX += Math.floor((nw - W) / 2); data.OriginY += Math.floor((nh - H) / 2); }
    W = nw; H = nh;
    data.Width = W; data.Height = H;
    data.MarginLeft = 0; data.MarginTop = 0; data.MarginRight = W - 1; data.MarginBottom = H - 1;
    structureChanged = true; pixelsChanged = true;
    changed(); drawStrip(); drawSide(); paint();
  }
  async function resizeDialog() {
    const v = { w: W, h: H, mode: 'canvas', anchor: 'top-left' };
    const ok = await modal('Resize sprite', h('div', { class: 'fields' },
      numField('Width', v, 'w', null, { min: 1, step: 1 }), numField('Height', v, 'h', null, { min: 1, step: 1 }),
      selectField('Mode', v, 'mode', ['canvas', 'scale']), selectField('Anchor (canvas mode)', v, 'anchor', ['top-left', 'center'])),
    [{ label: 'Cancel', value: false }, { label: 'Resize', value: true, primary: true }]);
    if (ok && v.w > 0 && v.h > 0) resizeAll(Math.round(v.w), Math.round(v.h), v.mode, v.anchor);
  }
  async function importFrames() {
    const files = await pickFiles('image/png,image/*', true);
    if (!files.length) return;
    for (const file of files) {
      const bmp = await createImageBitmap(file);
      let parts = [[0, 0]];
      if (bmp.width !== W || bmp.height !== H) {
        const strip = bmp.height === H && bmp.width % W === 0 && bmp.width > W;
        const choice = await modal('Image size differs',
          h('p', null, `${file.name} is ${bmp.width}×${bmp.height}. The sprite is ${W}×${H}.`),
          [{ label: 'Cancel', value: null },
            ...(strip ? [{ label: `Split into ${bmp.width / W} frames`, value: 'split' }] : []),
            { label: 'Place at top-left', value: 'place' },
            { label: `Resize sprite to ${bmp.width}×${bmp.height}`, value: 'resize', primary: true }]);
        if (!choice) continue;
        if (choice === 'resize') resizeAll(bmp.width, bmp.height, 'canvas', 'top-left');
        if (choice === 'split') parts = Array.from({ length: bmp.width / W }, (_, i) => [i * W, 0]);
      }
      for (const [sx, sy] of parts) {
        const c = newCanvas(W, H);
        c.getContext('2d').drawImage(bmp, sx, sy, W, H, 0, 0, W, H);
        frames.splice(++cur, 0, { canvas: c, undo: [], redo: [], dirty: true });
      }
    }
    structureChanged = true; pixelsChanged = true;
    changed(); drawStrip(); paint();
    toast('Imported. Delete the old frames if you want to replace them.');
  }
  async function exportStrip() {
    const c = newCanvas(W * frames.length, H);
    frames.forEach((f, i) => c.getContext('2d').drawImage(f.canvas, i * W, 0));
    download(await toBlob(c), `${name}_strip${frames.length}.png`);
  }

  // ---- Side panel: properties, palette, preview ----
  const preview = newCanvas(W, H);
  let fps = 8;
  function drawSide() {
    side.replaceChildren();
    const onP = () => { changed(); paint(); };
    add(side,
      h('h3', null, 'Properties'),
      h('div', { class: 'fields' },
        numField('Origin X', data, 'OriginX', onP, { step: 1 }), numField('Origin Y', data, 'OriginY', onP, { step: 1 })),
      h('div', { class: 'fields' },
        selectField('Bounding box', { v: ['Automatic', 'Full image', 'Manual'][data.BBoxMode] || 'Automatic' }, 'v', ['Automatic', 'Full image', 'Manual'],
          (k, v) => { data.BBoxMode = ['Automatic', 'Full image', 'Manual'].indexOf(v); if (data.BBoxMode === 1) Object.assign(data, { MarginLeft: 0, MarginTop: 0, MarginRight: W - 1, MarginBottom: H - 1 }); changed(); drawSide(); paint(); }),
        selectField('Collision', data, 'CollisionKind', ['AxisAlignedRect', 'Precise', 'RotatedRect'], onP)),
      h('div', { class: 'fields' },
        numField('Left', data, 'MarginLeft', onP, { step: 1 }), numField('Right', data, 'MarginRight', onP, { step: 1 }),
        numField('Top', data, 'MarginTop', onP, { step: 1 }), numField('Bottom', data, 'MarginBottom', onP, { step: 1 })),
      h('div', { class: 'fields' }, boolField('Transparent', data, 'Transparent', onP), boolField('Smooth', data, 'Smooth', onP), boolField('Preload', data, 'Preload', onP)),
      data.Extra ? h('div', { class: 'fields' }, numField('Playback speed', data.Extra, 'PlaybackSpeed', onP),
        selectField('Speed type', data.Extra, 'PlaybackSpeedType', ['FramesPerSecond', 'FramesPerGameFrame'], onP)) : null,
      h('p', { class: 'dim small' }, `${W}×${H}, ${frames.length} frames, ${data.MaskCount} collision masks. Automatic box and masks are made on save.`),
      h('hr'),
      h('h3', null, 'Palette'),
      palette(),
      h('hr'),
      h('h3', null, 'Animation'),
      h('div', { class: 'row' }, preview, h('label', { class: 'small dim' }, 'fps ', h('input', { type: 'number', value: fps, min: 1, max: 60, style: { width: '56px' }, oninput: (e) => { fps = Math.max(1, +e.target.value || 1); } }))),
    );
    preview.style.cssText = `width:${Math.min(160, W * 2)}px;image-rendering:pixelated;background:#222`;
  }
  function palette() {
    const seen = new Map();
    for (const f of frames) {
      const d = f.canvas.getContext('2d', { willReadFrequently: true }).getImageData(0, 0, W, H).data;
      for (let i = 0; i < d.length && seen.size < 48; i += 4) {
        if (d[i + 3] === 0) continue;
        const k = `#${[d[i], d[i + 1], d[i + 2]].map((v) => v.toString(16).padStart(2, '0')).join('')}`;
        seen.set(k, (seen.get(k) || 0) + 1);
      }
    }
    return h('div', { class: 'swatches' }, [...seen.keys()].map((k) => h('span', { title: k, style: { background: k }, onclick: () => { color = k; alpha = 255; colorIn.value = k; alphaIn.value = 255; } })));
  }
  let anim = 0, animT = 0;
  const tick = (t) => {
    anim = requestAnimationFrame(tick);
    if (t - animT < 1000 / fps) return;
    animT = t;
    const i = Math.floor(t / (1000 / fps)) % frames.length;
    const g = preview.getContext('2d');
    if (preview.width !== W || preview.height !== H) { preview.width = W; preview.height = H; }
    g.clearRect(0, 0, W, H);
    g.drawImage(frames[i].canvas, 0, 0);
  };
  anim = requestAnimationFrame(tick);

  drawStrip();
  drawSide();
  paint();

  // ---- Save ----
  function alphaUnion(list) {
    const m = new Uint8Array(W * H);
    for (const f of list) {
      const d = f.canvas.getContext('2d', { willReadFrequently: true }).getImageData(0, 0, W, H).data;
      for (let i = 0; i < W * H; i++) if (d[i * 4 + 3] > 0) m[i] = 1;
    }
    return m;
  }
  function autoBox() {
    const m = alphaUnion(frames);
    let l = W, r = -1, t = H, b = -1;
    for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) if (m[y * W + x]) { l = Math.min(l, x); r = Math.max(r, x); t = Math.min(t, y); b = Math.max(b, y); }
    if (r < 0) { l = 0; t = 0; r = W - 1; b = H - 1; }
    Object.assign(data, { MarginLeft: l, MarginRight: r, MarginTop: t, MarginBottom: b });
  }
  async function maskBlob(list) {
    const c = newCanvas(W, H), g = c.getContext('2d');
    g.fillStyle = '#000'; g.fillRect(0, 0, W, H);
    g.fillStyle = '#fff';
    if (data.CollisionKind === 'Precise') {
      const m = alphaUnion(list);
      for (let i = 0; i < W * H; i++) if (m[i]) g.fillRect(i % W, Math.floor(i / W), 1, 1);
    } else {
      g.fillRect(data.MarginLeft, data.MarginTop, data.MarginRight - data.MarginLeft + 1, data.MarginBottom - data.MarginTop + 1);
    }
    return toBlob(c);
  }

  return {
    save: async () => {
      if (data.BBoxMode === 0) autoBox();
      const writeAll = structureChanged;
      for (let i = 0; i < frames.length; i++) {
        if (!writeAll && !frames[i].dirty) continue;
        await capi.writeFile('sprites', name, `${name}_${i}.png`, await toBlob(frames[i].canvas));
      }
      for (let i = frames.length; i < oldCount; i++) {
        if (asset.files.includes(`${name}_${i}.png`)) await capi.deleteFile('sprites', name, `${name}_${i}.png`).catch(() => {});
      }
      // Collision masks: made again when the pixels change, if they use the sprite size.
      const sameDims = !maskDims || (maskDims[0] === oldW && maskDims[1] === oldH);
      if ((pixelsChanged || structureChanged) && sameDims) {
        const perFrame = data.MaskCount > 1 && data.MaskCount === oldCount;
        const n = perFrame ? frames.length : 1;
        for (let i = 0; i < n; i++) await capi.writeFile('sprites', name, `${name}_${i}.mask.png`, await maskBlob(perFrame ? [frames[i]] : frames));
        for (let i = n; i < data.MaskCount; i++) await capi.deleteFile('sprites', name, `${name}_${i}.mask.png`).catch(() => {});
        data.MaskCount = n;
        maskDims = [W, H];
      } else if (!sameDims && frames.length !== oldCount && data.MaskCount > 1) {
        toast('Collision masks use a size other than the sprite size. Check the masks in the Files tab.', 'error');
      }
      data.TextureCount = frames.length;
      data.Width = W;
      data.Height = H;
      const r = await capi.write('sprites', name, { data });
      asset.data = r.data;
      asset.files = r.files;
      for (const f of frames) f.dirty = false;
      structureChanged = false;
      pixelsChanged = false;
      oldW = W; oldH = H; oldCount = frames.length;
      forgetImages(capi.asset('sprites', name));
      drawStrip();
      drawSide();
    },
    dispose: () => {
      cancelAnimationFrame(anim);
      ro.disconnect();
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('keyup', onKey);
    },
  };
}
