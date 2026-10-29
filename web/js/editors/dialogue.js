// Dialogue editor: every text line found in GML (msgsetloc, c_msgsetloc, stringsetloc, ...).
// A change replaces only the string literal in the code.
import { h, attempt, debounce, toast, confirmDialog, add } from '../ui.js';
import { setDirtyCheck } from '../app.js';
import { textPreview } from './textpreview.js';
import { DEFAULT_LIMITS, hasFace } from './textcore.js';
import { codeButtons, lengthInfo, fontSelect, fontCanvas } from './textui.js';

const PAGE = 100;

export async function render(main, ctx) {
  const { capi, ch } = ctx;
  const page = h('div', { class: 'page', style: { maxWidth: 'none' } });
  main.append(page);
  const q = h('input', { type: 'search', placeholder: 'Find text or key', value: ctx.query.q || '', style: { width: '280px' } });
  const code = h('input', { type: 'search', placeholder: 'Code entry (exact)', value: ctx.query.code || '', style: { width: '280px' } });
  const info = h('span', { class: 'dim' });
  const saveAll = h('button', { class: 'primary', disabled: true, onclick: () => save() }, 'Save changes');
  const table = h('div');
  let offset = 0;
  let data = null;
  const edits = new Map(); // row index -> new text
  const trEdits = new Map(); // `${lang}\n${key}` -> {lang, key, text}
  const dirty = () => edits.size + trEdits.size;
  const showDirty = () => {
    saveAll.disabled = dirty() === 0;
    saveAll.textContent = dirty() ? `Save changes (${dirty()})` : 'Save changes';
  };
  setDirtyCheck(() => dirty() > 0);
  let font = null;
  const fontSel = fontSelect(capi, (f) => { font = f && f.data.Glyphs?.length ? f : null; if (data) redrawPreviews(); });
  const previews = [];
  const redrawPreviews = () => previews.forEach((f) => f());

  add(page,
    h('h2', null, 'Dialogue'),
    h('p', { class: 'dim small' }, 'Text lines from all code. English text is in the GML; other languages come from lang/*.json in the chapter folder. Translation changes are kept in the workspace (lang store) and go into the mod. ',
      'Codes: & or newline = new line, / = wait for key, % = close box, ^1 = pause, \\E1 = face, \\cY = colour.'),
    h('div', { class: 'row' }, q, code, h('button', { onclick: () => { offset = 0; load(); } }, 'Find'), info, h('div', { class: 'grow' }),
      fontSel, h('a', { class: 'button', href: `#/${encodeURIComponent(ch)}/text` }, 'Text tools'), saveAll),
    table);
  q.addEventListener('input', debounce(() => { offset = 0; load(); }, 300));
  code.addEventListener('change', () => { offset = 0; load(); });

  async function load() {
    if (dirty() && !(await confirmDialog('Discard unsaved dialogue changes?', 'Discard'))) return;
    edits.clear();
    trEdits.clear();
    showDirty();
    const r = await attempt(() => capi.dialogue({ q: q.value, code: code.value, offset, limit: PAGE }));
    if (!r) return;
    data = r;
    previews.length = 0;
    info.textContent = `${r.total} lines${r.total ? `, ${offset + 1}-${Math.min(offset + PAGE, r.total)}` : ''}`;
    table.replaceChildren(
      h('table', null,
        h('tr', null, h('th', null, 'Where'), h('th', null, 'Speaker'), h('th', { style: { width: '42%' } }, 'Text'), h('th', null, 'Preview'), r.languages.length ? h('th', null, r.languages.join(', ')) : null),
        r.items.map((it, i) => row(it, i, r.languages))),
      h('div', { class: 'row', style: { marginTop: '8px' } },
        h('button', { disabled: offset === 0, onclick: () => { offset = Math.max(0, offset - PAGE); load(); } }, 'Previous'),
        h('button', { disabled: offset + PAGE >= r.total, onclick: () => { offset += PAGE; load(); } }, 'Next')));
  }

  function row(it, i, langs) {
    const c = encodeURIComponent(ch);
    const pv = h('div');
    const len = h('div');
    // Preview and line widths (default box limits; the game font measures in pixels when it is picked).
    const show = (text) => {
      const lim = font ? { ...DEFAULT_LIMITS, px: DEFAULT_LIMITS.chars * font.metrics.fallback, face_px: DEFAULT_LIMITS.face_chars * font.metrics.fallback } : DEFAULT_LIMITS;
      pv.replaceChildren(font && text !== null ? fontCanvas(font, text, 1) : textPreview(text));
      len.replaceChildren(text !== null ? lengthInfo(text, hasFace(text, it.speaker), lim, font) : '');
    };
    let input;
    if (it.text !== null && it.span) {
      input = h('textarea', { class: 'dlg-text', value: it.text, rows: Math.min(6, 1 + (it.text.match(/\n|&/g) || []).length) });
      previews.push(() => show(input.value));
      input.addEventListener('input', () => {
        if (input.value === it.text) edits.delete(i); else edits.set(i, input.value);
        input.style.borderColor = edits.has(i) ? 'var(--accent)' : '';
        show(input.value);
        showDirty();
      });
    } else {
      input = h('span', { class: 'dim small' }, it.key ? `Key only: ${it.key}` : '(not a plain string)');
      previews.push(() => show(it.text));
    }
    show(input instanceof HTMLTextAreaElement ? input.value : it.text);
    return h('tr', null,
      h('td', { class: 'small' },
        h('a', { class: 'mono', href: `#/${c}/assets/code/${encodeURIComponent(it.code)}?line=${it.line}` }, `${it.code.replace(/^gml_(Object_|Script_|GlobalScript_)?/, '')}:${it.line}`),
        h('div', { class: 'dim' }, it.func),
        /c_/.test(it.func) ? h('a', { href: `#/${c}/cutscenes/${encodeURIComponent(it.code)}` }, 'cutscene') : null),
      h('td', null, it.speaker || h('span', { class: 'dim' }, '-')),
      h('td', null, input, input instanceof HTMLTextAreaElement ? codeButtons(capi, input) : null, len, it.key ? h('div', { class: 'dim small mono' }, it.key) : null),
      h('td', null, pv),
      langs.length ? h('td', { class: 'small' }, langs.map((l) => trCell(it, l))) : null);
  }

  // One translation of a line with a key: editable; a changed text can go back to the game text.
  function trCell(it, l) {
    const old = it.translations[l];
    if (!it.key) return old ? h('div', null, h('span', { class: 'dim' }, `${l}: `), old) : null;
    const id = `${l}\n${it.key}`;
    const ta = h('textarea', { class: 'dlg-text', rows: 1, value: old ?? '', placeholder: '(missing)' });
    const len = h('div');
    const show = () => len.replaceChildren(ta.value ? lengthInfo(ta.value, hasFace(ta.value, it.speaker), DEFAULT_LIMITS, null) : '');
    ta.addEventListener('input', () => {
      if (ta.value === (old ?? '')) trEdits.delete(id); else trEdits.set(id, { lang: l, key: it.key, text: ta.value });
      ta.style.borderColor = trEdits.has(id) ? 'var(--accent)' : '';
      show();
      showDirty();
    });
    show();
    const changed = (it.translations_changed || []).includes(l);
    const revert = changed ? h('button', { class: 'small', title: 'Remove the mod change: use the game text again', onclick: async () => {
      if (!(await confirmDialog(`Use the game text for ${l} ${it.key} again?`, 'Revert'))) return;
      if (!(await attempt(() => capi.putLang(data.lang_files[l], { [it.key]: null })))) return;
      toast('Reverted', 'ok');
      trEdits.delete(id);
      showDirty();
      load();
    } }, 'revert') : null;
    return h('div', { class: 'dlg-tr' }, h('span', { class: 'dim' }, `${l}: `), changed ? h('span', { class: 'badge mod' }, 'changed') : null, revert, ta, len);
  }

  async function save() {
    // Translations: one write per lang file. They do not change the GML, so they go first.
    const byFile = new Map();
    for (const [id, e] of trEdits) {
      const file = data.lang_files[e.lang];
      if (!byFile.has(file)) byFile.set(file, { changes: {}, ids: [] });
      byFile.get(file).changes[e.key] = e.text;
      byFile.get(file).ids.push(id);
    }
    let tn = 0;
    for (const [file, p] of byFile) {
      if (!(await attempt(() => capi.putLang(file, p.changes)))) continue;
      tn += p.ids.length;
      for (const id of p.ids) trEdits.delete(id);
    }
    if (tn) toast(`Saved ${tn} translations`, 'ok');
    // Group by code entry: one patch per code.
    const byCode = new Map();
    for (const [i, text] of edits) {
      const it = data.items[i];
      if (!byCode.has(it.code)) byCode.set(it.code, { rev: it.rev, edits: [], rows: [] });
      byCode.get(it.code).edits.push({ start: it.span[0], end: it.span[1], string: text });
      byCode.get(it.code).rows.push(i);
    }
    let n = 0, saved = 0;
    for (const [codeName, p] of byCode) {
      if (!(await attempt(() => capi.patchCode(codeName, p.rev, p.edits)))) continue;
      n += p.edits.length;
      saved++;
      for (const i of p.rows) edits.delete(i);
    }
    if (dirty()) {
      // Keep the lines that were not saved, so the text is not lost. Saved rows now have a new rev: reload before you change them again.
      toast(`Saved ${n} lines in ${saved} code entries and ${tn} translations. ${dirty()} changes are not saved.`, 'error');
      showDirty();
      return;
    }
    if (byCode.size) toast(`Saved ${n} lines in ${byCode.size} code entries`, 'ok');
    load();
  }

  load();
}
