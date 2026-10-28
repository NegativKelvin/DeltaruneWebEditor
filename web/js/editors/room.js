// Room editor: draws backgrounds, tiles, assets, and instances; moves, adds, and deletes instances;
// paints tiles; edits instance and room creation code (room triggers); edits room, view, and layer properties.
// Works with GMS2 rooms (Layers) and older rooms (Backgrounds, Tiles, GameObjects without layers).
import { h, toast, numField, textField, boolField, colorField, assetDatalist, loadImage, add } from '../ui.js';
import { jsonForm } from './jsonform.js';
import { codeEditor } from './codeedit.js';
import { roomRenderer, layerKind } from './roomdraw.js';
import { openPlay } from './roomplay.js';

const TRIGGER_RE = /trig|door|marker|event|interact|readable|warp|entrance|savepoint|save_point|cutscene|area|zone|npc|chest|switch/i;

export async function open(el, ectx) {
  const { capi, ch, name, asset, setDirty } = ectx;
  const data = structuredClone(asset.data);
  data.GameObjects ||= [];
  const gms2 = Array.isArray(data.Layers) && data.Layers.length > 0;
  const [objIdx, sprIdx, bgIdx] = await Promise.all([
    capi.index('objects', ['Sprite', 'Visible', 'ParentObject', 'Depth']),
    capi.index('sprites', ['Width', 'Height', 'OriginX', 'OriginY', 'TextureCount']),
    capi.index('backgrounds', ['Tileset']),
  ]);
  const codeEdits = new Map(); // code name -> source (new or changed)
  const undoStack = [], redoStack = [];

  // ---- View state ----
  let zoom = 1, panX = 20, panY = 20;
  let tool = 'select';
  let selected = -1; // index into data.GameObjects
  let activeLayer = gms2 ? data.Layers.findIndex((l) => layerKind(l) === 'Instances') : -1;
  let placeObject = '';
  let tileBrush = 1;
  const hidden = new Set(); // editor-only hidden layers (index, or 'bg', 'tiles', 'inst', 'fg' in old rooms)
  const opts = { grid: true, snap: true, gridSize: 20, views: true, triggers: true };

  // ---- Helpers ----
  const R = roomRenderer(capi, data, { objIdx, sprIdx, bgIdx }, () => requestPaint());
  const { objSprite, objVisible, instBox, drawSprite, layerCache } = R;
  const isTrigger = (inst) => TRIGGER_RE.test(inst.ObjectName || '') || !objVisible(inst.ObjectName) || !!inst.CreationCodeEntry;
  const instLayer = (i) => (gms2 ? data.Layers.findIndex((l) => l.InstanceIndices?.includes(i)) : -1);
  const instVisible = (i) => (gms2 ? !hidden.has(instLayer(i)) : !hidden.has('inst'));

  function commit() {
    undoStack.push(JSON.stringify(data));
    if (undoStack.length > 30) undoStack.shift();
    redoStack.length = 0;
    setDirty(true);
  }
  function restore(json) {
    const v = JSON.parse(json);
    for (const k of Object.keys(data)) delete data[k];
    Object.assign(data, v);
    layerCache.clear();
    playing?.invalidate();
    selected = Math.min(selected, data.GameObjects.length - 1);
    setDirty(true);
    drawSide();
    requestPaint();
  }
  const undo = () => { if (undoStack.length) { redoStack.push(JSON.stringify(data)); restore(undoStack.pop()); } };
  const redo = () => { if (redoStack.length) { undoStack.push(JSON.stringify(data)); restore(redoStack.pop()); } };

  // ---- Layout ----
  const view = h('canvas');
  const wrap = h('div', { class: 'canvas-wrap' }, view);
  const status = h('div', { class: 'status' });
  const side = h('div', { class: 'side' });
  const toolBtns = {};
  const toolbar = h('div', { class: 'toolbar' },
    ...[['select', 'Select / move (V)'], ['place', 'Add instance (A)'], ['paint', 'Paint tiles (T)'], ['erase', 'Erase tiles (X)'], ['pan', 'Pan (Space)']]
      .map(([id, label]) => (toolBtns[id] = h('button', { class: 'small', title: label, onclick: () => setTool(id) }, label.split(' (')[0]))),
    h('span', { class: 'sep' }),
    h('input', { type: 'text', placeholder: 'object to add', list: assetDatalist(capi, ch, 'objects'), style: { width: '170px' }, oninput: (e) => { placeObject = e.target.value; if (placeObject) setTool('place'); } }),
    h('span', { class: 'sep' }),
    h('label', { class: 'field check small' }, h('input', { type: 'checkbox', checked: opts.grid, onchange: (e) => { opts.grid = e.target.checked; requestPaint(); } }), h('span', null, 'grid')),
    h('label', { class: 'field check small' }, h('input', { type: 'checkbox', checked: opts.snap, onchange: (e) => { opts.snap = e.target.checked; } }), h('span', null, 'snap')),
    h('input', { type: 'number', value: opts.gridSize, min: 1, style: { width: '56px' }, title: 'Grid size', oninput: (e) => { opts.gridSize = Math.max(1, +e.target.value || 20); requestPaint(); } }),
    h('label', { class: 'field check small' }, h('input', { type: 'checkbox', checked: opts.views, onchange: (e) => { opts.views = e.target.checked; requestPaint(); } }), h('span', null, 'views')),
    h('label', { class: 'field check small' }, h('input', { type: 'checkbox', checked: opts.triggers, onchange: (e) => { opts.triggers = e.target.checked; requestPaint(); } }), h('span', null, 'show triggers')),
    h('span', { class: 'sep' }),
    h('button', { class: 'small', onclick: () => setZoom(zoom / 2) }, '−'),
    h('button', { class: 'small', onclick: () => setZoom(zoom * 2) }, '+'),
    h('button', { class: 'small', onclick: () => fit() }, 'Fit'),
    h('button', { class: 'small', title: 'Ctrl+Z', onclick: undo }, 'Undo'),
    h('button', { class: 'small', title: 'Ctrl+Y', onclick: redo }, 'Redo'),
    h('span', { class: 'sep' }),
    h('button', { class: 'small primary', title: 'Play the room in the browser with the unsaved changes (P)', onclick: () => play() }, '▶ Play'),
  );
  let playing = null;
  const play = async () => { playing = await openPlay({ capi, name, data, codeEdits, onExit: () => { playing = null; requestPaint(); } }); };
  el.append(h('div', { class: 'full', style: { height: 'calc(100vh - 150px)' } }, toolbar,
    h('div', { class: 'row', style: { flex: 1, minHeight: 0, alignItems: 'stretch', flexWrap: 'nowrap', gap: 0 } },
      h('div', { class: 'col grow', style: { gap: 0, minHeight: 0 } }, wrap, status), side)));

  function setTool(t) {
    tool = t;
    for (const [k, b] of Object.entries(toolBtns)) b.classList.toggle('active', k === t);
    view.style.cursor = t === 'pan' ? 'grab' : t === 'select' ? 'default' : 'crosshair';
    if ((t === 'paint' || t === 'erase') && !(gms2 && layerKind(data.Layers[activeLayer] || {}) === 'Tiles')) toast('Select a tile layer in the Layers list first.');
  }
  function setZoom(z, cx = view.width / 2, cy = view.height / 2) {
    z = Math.max(0.125, Math.min(16, z));
    panX = cx - ((cx - panX) / zoom) * z;
    panY = cy - ((cy - panY) / zoom) * z;
    zoom = z;
    requestPaint();
  }
  function fit() {
    const r = wrap.getBoundingClientRect();
    const z = Math.min((r.width - 40) / data.Width, (r.height - 40) / data.Height);
    zoom = Math.pow(2, Math.floor(Math.log2(Math.max(0.125, z))));
    panX = (r.width - data.Width * zoom) / 2;
    panY = (r.height - data.Height * zoom) / 2;
    requestPaint();
  }

  // ---- Drawing ----
  const g = view.getContext('2d');
  let paintQueued = false;
  function requestPaint() {
    if (paintQueued) return;
    paintQueued = true;
    requestAnimationFrame(() => { paintQueued = false; paint(); });
  }

  function drawInstance(i) {
    const inst = data.GameObjects[i];
    const spr = objSprite(inst.ObjectName);
    const trig = isTrigger(inst);
    const invisible = !objVisible(inst.ObjectName);
    if (invisible && !opts.triggers) return;
    const drawn = spr && drawSprite(g, spr, inst.ImageIndex || 0, inst.X, inst.Y, inst.ScaleX, inst.ScaleY, inst.Rotation, inst.Color, invisible ? 0.35 : 1);
    if (!drawn || (trig && opts.triggers)) {
      const [x, y, w, hh] = instBox(inst);
      g.save();
      g.fillStyle = trig ? 'rgba(255,0,200,0.18)' : 'rgba(54,197,240,0.2)';
      g.strokeStyle = trig ? '#ff4fd8' : '#36c5f0';
      g.lineWidth = 1 / zoom;
      g.setLineDash(trig ? [4 / zoom, 3 / zoom] : []);
      if (!drawn) g.fillRect(x, y, w, hh);
      g.strokeRect(x, y, w, hh);
      if (zoom >= 0.75 && (!drawn || trig)) {
        g.fillStyle = trig ? '#ff9ff0' : '#9fe3ff';
        g.font = `${10 / zoom}px monospace`;
        g.fillText(inst.ObjectName || '?', x + 2 / zoom, y + 10 / zoom);
      }
      g.restore();
    }
  }

  function paint() {
    const r = wrap.getBoundingClientRect();
    if (view.width !== Math.floor(r.width) || view.height !== Math.floor(r.height)) { view.width = Math.floor(r.width); view.height = Math.floor(r.height); }
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.fillStyle = '#050508';
    g.fillRect(0, 0, view.width, view.height);
    g.setTransform(zoom, 0, 0, zoom, panX, panY);
    g.imageSmoothingEnabled = false;
    R.drawScene(g, hidden, (list) => { for (const i of list) drawInstance(i); });

    // Overlays
    g.lineWidth = 1 / zoom;
    if (opts.grid && opts.gridSize * zoom >= 6) {
      g.strokeStyle = 'rgba(255,255,255,0.07)';
      g.beginPath();
      for (let x = 0; x <= data.Width; x += opts.gridSize) { g.moveTo(x, 0); g.lineTo(x, data.Height); }
      for (let y = 0; y <= data.Height; y += opts.gridSize) { g.moveTo(0, y); g.lineTo(data.Width, y); }
      g.stroke();
    }
    g.strokeStyle = '#555';
    g.strokeRect(0, 0, data.Width, data.Height);
    if (opts.views) {
      for (const v of data.Views || []) {
        if (!v.Enabled) continue;
        g.strokeStyle = '#36c5f0';
        g.setLineDash([6 / zoom, 4 / zoom]);
        g.strokeRect(v.ViewX, v.ViewY, v.ViewWidth, v.ViewHeight);
        g.setLineDash([]);
      }
    }
    if (selected >= 0 && data.GameObjects[selected]) {
      const [x, y, w, hh] = instBox(data.GameObjects[selected]);
      g.strokeStyle = '#ffd400';
      g.lineWidth = 2 / zoom;
      g.strokeRect(x, y, w, hh);
    }
  }

  // ---- Hit test and mouse ----
  const toRoom = (e) => {
    const r = view.getBoundingClientRect();
    return [(e.clientX - r.left - panX) / zoom, (e.clientY - r.top - panY) / zoom];
  };
  function hit(x, y) {
    let order;
    if (gms2) {
      order = [];
      const ls = data.Layers.map((l, i) => i).filter((i) => layerKind(data.Layers[i]) === 'Instances' && !hidden.has(i)).sort((a, b) => data.Layers[a].Depth - data.Layers[b].Depth);
      for (const li of ls) order.push(...[...(data.Layers[li].InstanceIndices || [])].reverse());
    } else {
      order = hidden.has('inst') ? [] : data.GameObjects.map((_, i) => i).reverse();
    }
    for (const i of order) {
      const inst = data.GameObjects[i];
      if (!inst || (!opts.triggers && !objVisible(inst.ObjectName))) continue;
      const [bx, by, w, hh] = instBox(inst);
      if (x >= bx && y >= by && x < bx + w && y < by + hh) return i;
    }
    return -1;
  }
  const snap = (v) => (opts.snap ? Math.round(v / opts.gridSize) * opts.gridSize : Math.round(v));

  function addInstance(obj, x, y) {
    commit();
    const inst = { ObjectName: obj, X: x, Y: y, CreationCodeEntry: null, ScaleX: 1, ScaleY: 1, Color: 0xffffffff, Rotation: 0, PreCreateCodeEntry: null, ImageSpeed: 1, ImageIndex: 0 };
    data.GameObjects.push(inst);
    const i = data.GameObjects.length - 1;
    if (gms2) {
      let li = activeLayer >= 0 && layerKind(data.Layers[activeLayer]) === 'Instances' ? activeLayer : data.Layers.findIndex((l) => layerKind(l) === 'Instances');
      if (li < 0) { toast('This room has no instance layer.', 'error'); data.GameObjects.pop(); return; }
      data.Layers[li].InstanceIndices.push(i);
    }
    if (Array.isArray(data.InstanceCreationOrderIDs)) data.InstanceCreationOrderIDs.push(i);
    selected = i;
    drawSide();
    requestPaint();
  }

  function deleteInstance(i) {
    if (i < 0) return;
    commit();
    data.GameObjects.splice(i, 1);
    const fix = (arr) => arr.filter((x) => x !== i).map((x) => (x > i ? x - 1 : x));
    for (const l of data.Layers || []) if (l.InstanceIndices) l.InstanceIndices = fix(l.InstanceIndices);
    if (Array.isArray(data.InstanceCreationOrderIDs)) data.InstanceCreationOrderIDs = fix(data.InstanceCreationOrderIDs);
    selected = -1;
    drawSide();
    requestPaint();
  }

  function duplicateInstance(i) {
    if (i < 0) return;
    const src = data.GameObjects[i];
    addInstance(src.ObjectName, src.X + opts.gridSize, src.Y + opts.gridSize);
    const n = data.GameObjects[selected];
    Object.assign(n, structuredClone(src), { X: src.X + opts.gridSize, Y: src.Y + opts.gridSize, CreationCodeEntry: null, PreCreateCodeEntry: null });
    if (gms2) {
      const from = instLayer(i), to = instLayer(selected);
      if (from >= 0 && from !== to) { data.Layers[to].InstanceIndices.pop(); data.Layers[from].InstanceIndices.push(selected); }
    }
    drawSide();
    requestPaint();
  }

  function paintTile(x, y, erase) {
    const l = data.Layers[activeLayer];
    const ts = bgIdx[l?.Background]?.Tileset;
    if (!l || layerKind(l) !== 'Tiles' || !ts) return false;
    const tx = Math.floor((x - (l.XOffset || 0)) / ts.TileWidth), ty = Math.floor((y - (l.YOffset || 0)) / ts.TileHeight);
    if (tx < 0 || ty < 0 || tx >= l.TilesX || ty >= l.TilesY) return false;
    const k = ty * l.TilesX + tx;
    const v = erase ? 0 : tileBrush;
    if (l.TileData[k] === v) return false;
    l.TileData[k] = v;
    layerCache.delete(activeLayer);
    playing?.invalidate();
    return true;
  }

  let drag = null, spaceDown = false;
  view.addEventListener('pointerdown', (e) => {
    view.setPointerCapture(e.pointerId);
    const [x, y] = toRoom(e);
    if (e.button === 1 || tool === 'pan' || spaceDown) { drag = { pan: true, sx: e.clientX - panX, sy: e.clientY - panY }; return; }
    if (e.button !== 0) return;
    if (tool === 'place') {
      if (!placeObject) { toast('Type an object name in the box first.'); return; }
      if (!objIdx[placeObject]) { toast(`No object named ${placeObject}.`, 'error'); return; }
      addInstance(placeObject, snap(x), snap(y));
      return;
    }
    if (tool === 'paint' || tool === 'erase') {
      commit();
      if (paintTile(x, y, tool === 'erase')) requestPaint();
      drag = { tiles: true };
      return;
    }
    const i = hit(x, y);
    selected = i;
    drawSide();
    requestPaint();
    if (i >= 0) {
      const inst = data.GameObjects[i];
      drag = { inst: i, dx: x - inst.X, dy: y - inst.Y, moved: false };
    }
  });
  view.addEventListener('pointermove', (e) => {
    const [x, y] = toRoom(e);
    const over = hit(x, y);
    status.textContent = `${name}  ${data.Width}×${data.Height}  zoom ${zoom}×  x ${Math.floor(x)} y ${Math.floor(y)}${over >= 0 ? `  ${data.GameObjects[over].ObjectName}` : ''}`;
    if (!drag) return;
    if (drag.pan) { panX = e.clientX - drag.sx; panY = e.clientY - drag.sy; requestPaint(); return; }
    if (drag.tiles) { if (paintTile(x, y, tool === 'erase')) requestPaint(); return; }
    const inst = data.GameObjects[drag.inst];
    const nx = snap(x - drag.dx), ny = snap(y - drag.dy);
    if (nx === inst.X && ny === inst.Y) return;
    if (!drag.moved) { commit(); drag.moved = true; }
    inst.X = nx; inst.Y = ny;
    requestPaint();
  });
  view.addEventListener('pointerup', () => { if (drag?.moved) drawSide(); drag = null; });
  view.addEventListener('dblclick', () => {
    if (selected < 0) return;
    const ta = side.querySelector('.cc-open textarea');
    if (ta) ta.focus();
    else side.querySelector('.cc-open button')?.click();
  });
  view.addEventListener('wheel', (e) => {
    e.preventDefault();
    const r = view.getBoundingClientRect();
    setZoom(e.deltaY < 0 ? zoom * 2 : zoom / 2, e.clientX - r.left, e.clientY - r.top);
  }, { passive: false });
  const onKey = (e) => {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLSelectElement) return;
    if (e.key === ' ') { spaceDown = e.type === 'keydown'; e.preventDefault(); return; }
    if (e.type !== 'keydown') return;
    if ((e.ctrlKey || e.metaKey) && e.key === 'z') { e.preventDefault(); undo(); return; }
    if ((e.ctrlKey || e.metaKey) && e.key === 'y') { e.preventDefault(); redo(); return; }
    if ((e.ctrlKey || e.metaKey) && e.key === 'd') { e.preventDefault(); duplicateInstance(selected); return; }
    if (e.ctrlKey || e.metaKey) return;
    const k = e.key.toLowerCase();
    if (k === 'v') setTool('select');
    if (k === 'a') setTool('place');
    if (k === 't') setTool('paint');
    if (k === 'x') setTool('erase');
    if (k === 'p') { e.preventDefault(); play(); }
    if ((e.key === 'Delete' || e.key === 'Backspace') && selected >= 0) { e.preventDefault(); deleteInstance(selected); }
    if (selected >= 0 && e.key.startsWith('Arrow')) {
      e.preventDefault();
      commit();
      const st = e.shiftKey ? opts.gridSize : 1;
      const inst = data.GameObjects[selected];
      if (e.key === 'ArrowLeft') inst.X -= st;
      if (e.key === 'ArrowRight') inst.X += st;
      if (e.key === 'ArrowUp') inst.Y -= st;
      if (e.key === 'ArrowDown') inst.Y += st;
      drawSide();
      requestPaint();
    }
  };
  document.addEventListener('keydown', onKey);
  document.addEventListener('keyup', onKey);
  const ro = new ResizeObserver(requestPaint);
  ro.observe(wrap);

  // ---- Code editing (creation code = room triggers) ----
  async function codeBox(codeName, onCreate, label) {
    const box = h('div', { class: 'col' });
    if (!codeName) {
      box.append(h('button', { class: 'small', onclick: () => {
        const n = onCreate();
        codeEdits.set(n, '');
        setDirty(true);
        drawSide();
      } }, `+ Add ${label}`));
      return box;
    }
    const src = codeEdits.has(codeName) ? codeEdits.get(codeName) : (await capi.read('code', codeName).catch(() => ({ source: '' }))).source;
    const ed = codeEditor({ value: src, height: '200px', minHeight: '120px', onChange: (v) => { codeEdits.set(codeName, v); setDirty(true); }, onSave: ectx.save });
    box.append(h('div', { class: 'row small' }, h('a', { class: 'mono', href: `#/${encodeURIComponent(ch)}/assets/code/${encodeURIComponent(codeName)}` }, codeName)), ed.el);
    return box;
  }
  const newCodeName = (kind) => `gml_RoomCC_${name}_${kind}${Date.now().toString(36)}_Create`;

  // ---- Side panel ----
  const sections = { sel: true, layers: true, triggers: true, all: false, room: false };
  const section = (id, title, body) => {
    const d = h('details', { open: sections[id] }, h('summary', null, h('b', null, title)), body);
    d.addEventListener('toggle', () => { sections[id] = d.open; });
    return d;
  };

  function drawSide() {
    const scroll = side.scrollTop;
    side.replaceChildren();
    // Selected instance
    const selBox = h('div', { class: 'col' });
    if (selected >= 0 && data.GameObjects[selected]) {
      const inst = data.GameObjects[selected];
      const onC = () => { setDirty(true); requestPaint(); };
      const ccWrap = h('div');
      add(selBox,
        h('div', { class: 'row' }, h('label', { class: 'field grow' }, h('span', null, 'Object'),
          h('input', { type: 'text', value: inst.ObjectName, list: assetDatalist(capi, ch, 'objects'), onchange: (e) => { commit(); inst.ObjectName = e.target.value; requestPaint(); } })),
        h('a', { class: 'small', href: `#/${encodeURIComponent(ch)}/assets/objects/${encodeURIComponent(inst.ObjectName)}` }, 'open')),
        h('div', { class: 'fields' }, numField('X', inst, 'X', onC), numField('Y', inst, 'Y', onC), numField('Scale X', inst, 'ScaleX', onC), numField('Scale Y', inst, 'ScaleY', onC),
          numField('Rotation', inst, 'Rotation', onC), numField('Image index', inst, 'ImageIndex', onC, { step: 1 }), numField('Image speed', inst, 'ImageSpeed', onC), colorField('Blend', inst, 'Color', onC)),
        gms2 ? h('label', { class: 'field' }, h('span', null, 'Layer'), h('select', { onchange: (e) => {
          commit();
          const from = instLayer(selected), to = +e.target.value;
          if (from >= 0) data.Layers[from].InstanceIndices = data.Layers[from].InstanceIndices.filter((x) => x !== selected);
          data.Layers[to].InstanceIndices.push(selected);
          requestPaint();
        } }, data.Layers.map((l, li) => (layerKind(l) === 'Instances' ? h('option', { value: li, selected: li === instLayer(selected) }, l.Name) : null)))) : null,
        h('div', { class: 'row' },
          h('button', { class: 'small', onclick: () => duplicateInstance(selected) }, 'Duplicate (Ctrl+D)'),
          h('button', { class: 'small danger', onclick: () => deleteInstance(selected) }, 'Delete (Del)')),
        h('b', { class: 'small' }, 'Creation code (runs when the instance is made: trigger settings)'), ccWrap);
      codeBox(inst.CreationCodeEntry, () => { inst.CreationCodeEntry = newCodeName(''); return inst.CreationCodeEntry; }, 'creation code').then((b) => {
        b.classList.add('cc-open');
        ccWrap.append(b);
      });
    } else {
      selBox.append(h('p', { class: 'dim small' }, 'Click an instance. Drag to move. Arrow keys nudge (Shift: one grid step). Mouse wheel zooms, middle button or Space pans.'));
    }
    side.append(section('sel', 'Selected instance', selBox));

    // Layers
    const layersBox = h('div', { class: 'list-small' });
    const eye = (key) => h('input', { type: 'checkbox', checked: !hidden.has(key), title: 'Show in editor', onclick: (e) => e.stopPropagation(), onchange: (e) => { if (e.target.checked) hidden.delete(key); else hidden.add(key); requestPaint(); } });
    if (gms2) {
      [...data.Layers.keys()].sort((a, b) => data.Layers[a].Depth - data.Layers[b].Depth).forEach((li) => {
        const l = data.Layers[li];
        layersBox.append(h('div', { class: li === activeLayer ? 'sel' : '', onclick: () => { activeLayer = li; drawSide(); } },
          h('span', null, eye(li), ` ${l.Name}`), h('span', { class: 'dim' }, `${layerKind(l)} ${l.Depth}`)));
      });
    } else {
      for (const [k, label] of [['bg', 'Backgrounds'], ['tiles', `Tiles (${(data.Tiles || []).length})`], ['inst', `Instances (${data.GameObjects.length})`], ['fg', 'Foregrounds']]) {
        layersBox.append(h('div', null, h('span', null, eye(k), ` ${label}`)));
      }
    }
    const layerExtra = h('div');
    if (gms2 && data.Layers[activeLayer]) {
      const l = data.Layers[activeLayer];
      const kind = layerKind(l);
      if (kind === 'Tiles') layerExtra.append(tilePalette(l));
      const props = h('details', null, h('summary', null, `Layer properties: ${l.Name}`));
      props.append(jsonForm(l, () => { layerCache.delete(activeLayer); playing?.invalidate(); setDirty(true); requestPaint(); }, ectx));
      layerExtra.append(props);
    }
    side.append(section('layers', 'Layers', h('div', { class: 'col' }, layersBox, layerExtra)));

    // Triggers and events
    const trig = data.GameObjects.map((inst, i) => [inst, i]).filter(([inst]) => isTrigger(inst));
    side.append(section('triggers', `Triggers and events (${trig.length})`, h('div', { class: 'list-small' },
      trig.map(([inst, i]) => h('div', { class: i === selected ? 'sel' : '', onclick: () => { selected = i; centerOn(inst); drawSide(); requestPaint(); } },
        h('span', null, inst.ObjectName), h('span', { class: 'dim' }, `${inst.X},${inst.Y}${inst.CreationCodeEntry ? ' code' : ''}`))))));

    // All instances
    const filter = h('input', { type: 'search', placeholder: 'filter', oninput: () => fillAll() });
    const allBox = h('div', { class: 'list-small' });
    const fillAll = () => {
      allBox.replaceChildren();
      data.GameObjects.forEach((inst, i) => {
        if (filter.value && !inst.ObjectName.includes(filter.value)) return;
        allBox.append(h('div', { class: i === selected ? 'sel' : '', onclick: () => { selected = i; centerOn(inst); drawSide(); requestPaint(); } },
          h('span', null, `${i}: ${inst.ObjectName}`), h('span', { class: 'dim' }, `${inst.X},${inst.Y}`)));
      });
    };
    fillAll();
    side.append(section('all', `All instances (${data.GameObjects.length})`, h('div', { class: 'col' }, filter, allBox)));

    // Room properties
    const roomBox = h('div', { class: 'col' });
    const onR = () => { setDirty(true); requestPaint(); };
    const rcWrap = h('div');
    const rest = Object.fromEntries(Object.entries(data).filter(([k]) => !['GameObjects', 'Layers', 'Tiles', 'Views', 'InstanceCreationOrderIDs'].includes(k)));
    add(roomBox,
      h('div', { class: 'fields' }, numField('Width', data, 'Width', onR, { step: 1 }), numField('Height', data, 'Height', onR, { step: 1 }), numField('Speed', data, 'Speed', onR, { step: 1 })),
      h('div', { class: 'fields' }, textField('Caption', data, 'Caption', onR), boolField('Persistent', data, 'Persistent', onR), boolField('Draw bg colour', data, 'DrawBackgroundColor', onR), colorField('Bg colour', data, 'BackgroundColor', onR)),
      h('b', { class: 'small' }, 'Room creation code'), rcWrap,
      h('details', null, h('summary', null, 'Views'), jsonForm({ Views: data.Views || [] }, onR, ectx)),
      h('details', null, h('summary', null, 'All room fields'), jsonForm(rest, () => { Object.assign(data, rest); onR(); }, ectx)),
    );
    codeBox(data.CreationCodeEntry, () => { data.CreationCodeEntry = `gml_RoomCC_${name}_Create`; return data.CreationCodeEntry; }, 'room creation code').then((b) => rcWrap.append(b));
    side.append(section('room', 'Room properties', roomBox));
    side.scrollTop = scroll;
  }

  function centerOn(inst) {
    const r = wrap.getBoundingClientRect();
    panX = r.width / 2 - inst.X * zoom;
    panY = r.height / 2 - inst.Y * zoom;
  }

  function tilePalette(l) {
    const ts = bgIdx[l.Background]?.Tileset;
    if (!ts) return h('p', { class: 'dim small' }, `Tileset ${l.Background} has no tile data.`);
    const box = h('div', { class: 'col' }, h('span', { class: 'small dim' }, `Tile brush: ${tileBrush}. Click a tile, then use Paint (T) or Erase (X).`));
    const cv = h('canvas', { style: { imageRendering: 'pixelated', cursor: 'pointer', maxWidth: '100%', background: '#222' } });
    const cols = Math.min(ts.TileColumns || 8, 8);
    const scale = Math.max(1, Math.floor(32 / ts.TileWidth));
    const tw = ts.TileWidth, th = ts.TileHeight, bx = ts.OutputBorderX, by = ts.OutputBorderY, tc = ts.TileColumns || 1;
    loadImage(capi.file('backgrounds', l.Background, `${l.Background}.png`)).then((im) => {
      if (!im) return;
      const n = ts.TileCount || tc;
      cv.width = cols * tw * scale;
      cv.height = Math.ceil(n / cols) * th * scale;
      const cg = cv.getContext('2d');
      cg.imageSmoothingEnabled = false;
      for (let i = 0; i < n; i++) {
        const sx = (i % tc) * (tw + 2 * bx) + bx, sy = Math.floor(i / tc) * (th + 2 * by) + by;
        cg.drawImage(im, sx, sy, tw, th, (i % cols) * tw * scale, Math.floor(i / cols) * th * scale, tw * scale, th * scale);
      }
      cg.strokeStyle = '#ffd400';
      cg.lineWidth = 2;
      cg.strokeRect((tileBrush % cols) * tw * scale, Math.floor(tileBrush / cols) * th * scale, tw * scale, th * scale);
    });
    cv.addEventListener('click', (e) => {
      const r = cv.getBoundingClientRect();
      const x = Math.floor(((e.clientX - r.left) * (cv.width / r.width)) / (tw * scale)), y = Math.floor(((e.clientY - r.top) * (cv.height / r.height)) / (th * scale));
      tileBrush = y * cols + x;
      setTool('paint');
      drawSide();
    });
    box.append(cv);
    return box;
  }

  setTool('select');
  drawSide();
  requestAnimationFrame(fit);

  return {
    save: async () => {
      for (const [code, src] of codeEdits) await capi.write('code', code, { source: src });
      codeEdits.clear();
      const r = await capi.write('rooms', name, { data });
      asset.data = r.data;
    },
    dispose: () => {
      playing?.close();
      ro.disconnect();
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('keyup', onKey);
    },
  };
}

