// DOM helpers and small UI parts shared by all views.

/** Makes an element. `attrs`: class, style (string or object), on* handlers, dataset, other attributes/properties. */
export function h(tag, attrs, ...children) {
  const el = document.createElement(tag);
  if (attrs) {
    for (const [k, v] of Object.entries(attrs)) {
      if (v === undefined || v === null || v === false) continue;
      if (k === 'class') el.className = v;
      else if (k === 'style' && typeof v === 'object') Object.assign(el.style, v);
      else if (k === 'dataset') Object.assign(el.dataset, v);
      else if (k.startsWith('on') && typeof v === 'function') el.addEventListener(k.slice(2), v);
      else if (k in el && typeof v !== 'string') el[k] = v;
      else if (k === 'value' || k === 'checked' || k === 'selected') el[k] = v;
      else el.setAttribute(k, v === true ? '' : v);
    }
  }
  append(el, children);
  return el;
}

/** Like Element.append, but skips null/false and flattens arrays. */
export function add(el, ...children) {
  append(el, children);
  return el;
}

function append(el, children) {
  for (const c of children) {
    if (c === null || c === undefined || c === false) continue;
    if (Array.isArray(c)) append(el, c);
    else el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
}

export function clear(el) {
  while (el.firstChild) el.firstChild.remove();
  return el;
}

/** Short message at the bottom right. kind: info, ok, error. */
export function toast(msg, kind = 'info') {
  let box = document.getElementById('toasts');
  if (!box) {
    box = h('div', { id: 'toasts' });
    document.body.append(box);
  }
  const t = h('div', { class: `toast ${kind}` }, msg);
  box.append(t);
  setTimeout(() => t.remove(), kind === 'error' ? 8000 : 3000);
}

export function errMsg(e) {
  return e && e.message ? e.message : String(e);
}

/** Runs an async action and shows its error as a toast. Returns the result or undefined. */
export async function attempt(fn, okMsg) {
  try {
    const r = await fn();
    if (okMsg) toast(okMsg, 'ok');
    return r;
  } catch (e) {
    console.error(e);
    toast(errMsg(e), 'error');
    return undefined;
  }
}

/** Modal dialog. buttons: [{label, value, primary}]. Resolves with the value of the pressed button (null on Escape). */
export function modal(title, body, buttons = [{ label: 'OK', value: true, primary: true }]) {
  return new Promise((resolve) => {
    const close = (v) => {
      back.remove();
      document.removeEventListener('keydown', onKey, true);
      resolve(v);
    };
    const onKey = (e) => {
      if (e.key === 'Escape') { e.stopPropagation(); close(null); }
      if (e.key === 'Enter' && !(e.target instanceof HTMLTextAreaElement)) {
        const p = buttons.find((b) => b.primary);
        if (p) { e.preventDefault(); close(typeof p.value === 'function' ? p.value() : p.value); }
      }
    };
    const back = h('div', { class: 'modal-back' },
      h('div', { class: 'modal' },
        h('h3', null, title),
        h('div', { class: 'modal-body' }, body),
        h('div', { class: 'modal-buttons' }, buttons.map((b) =>
          h('button', { class: b.primary ? 'primary' : '', onclick: () => close(typeof b.value === 'function' ? b.value() : b.value) }, b.label)))));
    document.body.append(back);
    document.addEventListener('keydown', onKey, true);
    const first = back.querySelector('input,select,textarea');
    (first || back.querySelector('button.primary'))?.focus();
  });
}

export function confirmDialog(text, okLabel = 'OK') {
  return modal('Confirm', h('p', null, text), [
    { label: 'Cancel', value: false },
    { label: okLabel, value: true, primary: true },
  ]).then((v) => v === true);
}

/** Asks for one line of text. Resolves with the text, or null. */
export function promptDialog(title, value = '', placeholder = '', list = null) {
  const input = h('input', { type: 'text', value, placeholder, style: { width: '100%' }, list: list || undefined });
  return modal(title, input, [
    { label: 'Cancel', value: null },
    { label: 'OK', value: () => input.value, primary: true },
  ]);
}

export function debounce(fn, ms) {
  let t;
  return (...a) => {
    clearTimeout(t);
    t = setTimeout(() => fn(...a), ms);
  };
}

export function download(blob, name) {
  const a = h('a', { href: URL.createObjectURL(blob), download: name });
  document.body.append(a);
  a.click();
  setTimeout(() => { URL.revokeObjectURL(a.href); a.remove(); }, 1000);
}

/** Opens a file chooser. Resolves with the chosen files (may be empty). */
export function pickFiles(accept = '', multiple = false) {
  return new Promise((resolve) => {
    const input = h('input', { type: 'file', accept, multiple, style: { display: 'none' } });
    input.addEventListener('change', () => { resolve([...input.files]); input.remove(); });
    document.body.append(input);
    input.click();
  });
}

/** Tabs: [{id, label, render(el)}]. Returns {el, select(id)}. */
export function tabs(items, initial) {
  const head = h('div', { class: 'tabs' });
  const body = h('div', { class: 'tab-body' });
  const rendered = new Map();
  let current = null;
  const select = (id) => {
    current = id;
    for (const b of head.children) b.classList.toggle('active', b.dataset.id === id);
    for (const [k, el] of rendered) el.hidden = k !== id;
    if (!rendered.has(id)) {
      const el = h('div', { class: 'tab-pane' });
      rendered.set(id, el);
      body.append(el);
      items.find((t) => t.id === id).render(el);
    }
  };
  for (const t of items) head.append(h('button', { dataset: { id: t.id }, onclick: () => select(t.id) }, t.label));
  select(initial || items[0].id);
  return { el: h('div', { class: 'tabs-wrap' }, head, body), select, get current() { return current; } };
}

/** Number input bound to obj[key]. */
export function numField(label, obj, key, onChange, attrs = {}) {
  return h('label', { class: 'field' }, h('span', null, label),
    h('input', { type: 'number', value: obj[key] ?? 0, step: attrs.step ?? 'any', ...attrs, oninput: (e) => {
      const v = e.target.value === '' ? 0 : Number(e.target.value);
      if (!Number.isNaN(v)) { obj[key] = v; onChange?.(key, v); }
    } }));
}

export function textField(label, obj, key, onChange, attrs = {}) {
  return h('label', { class: 'field' }, h('span', null, label),
    h('input', { type: 'text', value: obj[key] ?? '', ...attrs, oninput: (e) => {
      obj[key] = e.target.value === '' && attrs.nullable ? null : e.target.value;
      onChange?.(key, obj[key]);
    } }));
}

export function boolField(label, obj, key, onChange) {
  return h('label', { class: 'field check' },
    h('input', { type: 'checkbox', checked: !!obj[key], onchange: (e) => { obj[key] = e.target.checked; onChange?.(key, obj[key]); } }),
    h('span', null, label));
}

export function selectField(label, obj, key, options, onChange) {
  return h('label', { class: 'field' }, h('span', null, label),
    h('select', { onchange: (e) => { obj[key] = e.target.value; onChange?.(key, obj[key]); } },
      options.map((o) => h('option', { value: o, selected: o === obj[key] }, o))));
}

/** GameMaker colours are 0xAABBGGRR. */
export function gmToCss(c, alpha = true) {
  c = c >>> 0;
  const r = c & 255, g = (c >>> 8) & 255, b = (c >>> 16) & 255, a = (c >>> 24) & 255;
  return alpha ? `rgba(${r},${g},${b},${(a / 255).toFixed(3)})` : `rgb(${r},${g},${b})`;
}
export function gmToHex(c) {
  c = c >>> 0;
  const r = c & 255, g = (c >>> 8) & 255, b = (c >>> 16) & 255;
  return '#' + [r, g, b].map((x) => x.toString(16).padStart(2, '0')).join('');
}
export function hexToGm(hex, alpha = 255) {
  const n = parseInt(hex.slice(1), 16);
  const r = (n >> 16) & 255, g = (n >> 8) & 255, b = n & 255;
  return ((alpha << 24) | (b << 16) | (g << 8) | r) >>> 0;
}

/** Colour field for a GameMaker colour number (keeps alpha). */
export function colorField(label, obj, key, onChange) {
  const alpha = ((obj[key] ?? 0xffffffff) >>> 24) & 255;
  return h('label', { class: 'field' }, h('span', null, label),
    h('input', { type: 'color', value: gmToHex(obj[key] ?? 0xffffffff), oninput: (e) => {
      obj[key] = hexToGm(e.target.value, alpha);
      onChange?.(key, obj[key]);
    } }));
}

/** Loads an image; resolves to null on failure. */
const imgCache = new Map();
export function loadImage(url, fresh = false) {
  if (!fresh && imgCache.has(url)) return imgCache.get(url);
  const p = new Promise((resolve) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => resolve(null);
    img.src = fresh ? `${url}${url.includes('?') ? '&' : '?'}t=${Date.now()}` : url;
  });
  imgCache.set(url, p);
  return p;
}
export function forgetImages(prefix) {
  for (const k of [...imgCache.keys()]) if (k.startsWith(prefix)) imgCache.delete(k);
}

/** Datalist with asset names of one type, made once per chapter and type. */
const lists = new Map();
export function assetDatalist(chapterApi, ch, type) {
  const id = `dl-${ch}-${type}`;
  if (!lists.has(id)) {
    const dl = h('datalist', { id });
    document.body.append(dl);
    lists.set(id, dl);
    chapterApi.list(type).then((r) => {
      for (const it of r.items) dl.append(h('option', { value: it.name }));
    }).catch(() => {});
  }
  return id;
}
export function forgetDatalists() {
  for (const dl of lists.values()) dl.remove();
  lists.clear();
}
