// Generic form for any asset JSON (UTMT project format). Changes the value in place.
import { h, assetDatalist } from '../ui.js';

/** Field names that hold the name of another asset, and its type. */
export const REF_FIELDS = {
  Sprite: 'sprites', MaskSprite: 'sprites', ExportedSprite: 'sprites',
  ObjectName: 'objects', ParentObject: 'objects', FollowsObject: 'objects',
  CodeEntry: 'code', CreationCodeEntry: 'code', PreCreateCodeEntry: 'code', Code: 'code',
  BackgroundAsset: 'backgrounds', Background: 'backgrounds', Asset: 'backgrounds',
  Font: 'fonts', Sequence: 'sequences', Path: 'paths', Sound: 'sounds',
};

const BIG_PRIMITIVE_ARRAY = 40;

/**
 * value: object to edit. onChange(): called after each change.
 * ctx: {capi, ch} for asset links and name lists.
 */
export function jsonForm(value, onChange, ctx) {
  const root = h('div', { class: 'jf' });
  renderObject(root, value, onChange, ctx, 0);
  return root;
}

function renderObject(el, obj, onChange, ctx, depth) {
  for (const key of Object.keys(obj)) {
    if (key === '$type') continue;
    el.append(renderField(obj, key, onChange, ctx, depth));
  }
}

function link(ctx, type, name) {
  if (!name || !ctx?.ch) return null;
  return h('a', { href: `#/${encodeURIComponent(ctx.ch)}/assets/${type}/${encodeURIComponent(name)}`, class: 'small', title: `Open ${type}/${name}` }, 'open');
}

function renderField(obj, key, onChange, ctx, depth) {
  const v = obj[key];
  const set = (nv) => { obj[key] = nv; onChange(); };
  const refType = REF_FIELDS[key];
  if (v !== null && typeof v === 'object') return renderComposite(obj, key, onChange, ctx, depth);
  let input;
  if (typeof v === 'boolean') {
    input = h('input', { type: 'checkbox', checked: v, onchange: (e) => set(e.target.checked) });
  } else if (typeof v === 'number') {
    input = h('input', { type: 'number', step: 'any', value: v, oninput: (e) => { if (e.target.value !== '' && !Number.isNaN(+e.target.value)) set(+e.target.value); } });
  } else {
    // string or null: empty text means null.
    const listId = refType && ctx?.capi ? assetDatalist(ctx.capi, ctx.ch, refType) : undefined;
    input = h('input', { type: 'text', value: v ?? '', placeholder: v === null ? 'null' : '', list: listId, oninput: (e) => set(e.target.value === '' && (v === null || refType) ? null : e.target.value) });
  }
  return h('div', { class: 'jf-row' }, h('span', { class: 'jf-key', title: key }, key),
    h('span', { class: 'row' }, input, refType ? link(ctx, refType, v) : null));
}

function renderComposite(obj, key, onChange, ctx, depth) {
  const v = obj[key];
  const isArr = Array.isArray(v);
  const summary = h('summary', null, `${key} `, h('span', { class: 'dim' }, isArr ? `[${v.length}]` : (v.$type || '{…}')));
  const d = h('details', { open: depth < 1 && (!isArr || v.length <= 8) }, summary);
  let filled = false;
  const fill = () => {
    if (filled) return;
    filled = true;
    if (isArr && v.length > BIG_PRIMITIVE_ARRAY && v.every((x) => x === null || typeof x !== 'object')) {
      // Large arrays of numbers (tile data, ...) as one JSON text field.
      const ta = h('textarea', { style: { width: '100%', height: '120px' }, value: JSON.stringify(v) });
      ta.addEventListener('change', () => {
        try {
          const nv = JSON.parse(ta.value);
          if (!Array.isArray(nv)) throw new Error('not an array');
          v.splice(0, v.length, ...nv);
          ta.style.borderColor = '';
          onChange();
        } catch { ta.style.borderColor = 'var(--danger)'; }
      });
      d.append(ta);
      return;
    }
    if (isArr) {
      const body = h('div');
      const draw = () => {
        body.replaceChildren();
        summary.lastChild.textContent = `[${v.length}]`;
        const shown = v.slice(0, 200);
        shown.forEach((item, i) => {
          const row = h('div', { class: 'row' });
          const holder = h('div', { class: 'grow' });
          holder.append(item !== null && typeof item === 'object' ? renderComposite(v, i, onChange, ctx, depth + 1) : renderField(v, i, onChange, ctx, depth + 1));
          row.append(holder, h('button', { class: 'small danger', title: 'Remove item', onclick: () => { v.splice(i, 1); onChange(); draw(); } }, '×'));
          body.append(row);
        });
        if (v.length > 200) body.append(h('div', { class: 'dim small' }, `${v.length - 200} more items: use Raw JSON.`));
        body.append(h('button', { class: 'small', onclick: () => {
          const last = v[v.length - 1];
          v.push(last === undefined ? null : structuredClone(last));
          onChange();
          draw();
        } }, '+ add item (copy of last)'));
      };
      draw();
      d.append(body);
    } else {
      renderObject(d, v, onChange, ctx, depth + 1);
    }
  };
  d.addEventListener('toggle', () => { if (d.open) fill(); });
  if (d.open) fill();
  return d;
}
