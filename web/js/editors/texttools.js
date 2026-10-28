// Text tools: find and replace in all dialogue, line length check against the text box (and translations),
// game font preview, and face (portrait) codes.
import { h, attempt, tabs, confirmDialog, toast } from '../ui.js';
import { textPreview } from './textpreview.js';
import { DEFAULT_LIMITS, fontMetrics } from './textcore.js';
import { fontSelect, fontCanvas, pickFace, codeButtons } from './textui.js';

export async function render(main, ctx) {
  const { capi, ch } = ctx;
  const c = encodeURIComponent(ch);
  const page = h('div', { class: 'page', style: { maxWidth: 'none' } });
  main.append(page);
  const where = (it) => h('a', { class: 'mono small', href: `#/${c}/assets/code/${encodeURIComponent(it.code)}?line=${it.line}` },
    `${it.code.replace(/^gml_(Object_|Script_|GlobalScript_)?/, '')}:${it.line}`);
  page.append(h('h2', null, 'Text tools'));
  const t = tabs([
    { id: 'replace', label: 'Find and replace', render: replaceTab },
    { id: 'check', label: 'Line length and translations', render: checkTab },
    { id: 'font', label: 'Font and faces', render: fontTab },
  ], ctx.query.tab);
  page.append(t.el);

  function replaceTab(el) {
    const v = { find: '', replace: '', regex: false, ignore_case: false, code: '', speaker: '' };
    const inp = (k, ph, w = 240) => h('input', { type: 'text', placeholder: ph, style: { width: `${w}px` }, oninput: (e) => { v[k] = e.target.value; } });
    const chk = (k, label) => h('label', { class: 'field check' }, h('input', { type: 'checkbox', onchange: (e) => { v[k] = e.target.checked; } }), h('span', null, label));
    const out = h('div');
    const run = async (dry) => {
      if (!v.find) { toast('Give the text to find.', 'error'); return; }
      if (!dry) {
        const pre = await attempt(() => capi.textReplace({ ...v, dry_run: true }));
        if (!pre) return;
        if (!pre.changes.length) { toast('No line matches.'); return; }
        if (!(await confirmDialog(`Change ${pre.changes.length} lines in ${pre.codes} code entries?`, 'Replace'))) return;
      }
      const r = await attempt(() => capi.textReplace({ ...v, dry_run: dry }));
      if (!r) return;
      const failed = r.failed?.length ? h('div', { class: 'error-box' }, r.failed.map((f) => h('div', null, `${f.code}: ${f.error}`))) : null;
      out.replaceChildren(h('p', null, r.applied ? `Changed ${r.changes.length} lines.` : `${r.changes.length} lines in ${r.codes} code entries will change (nothing written yet).`), failed,
        h('table', null, h('tr', null, h('th', null, 'Where'), h('th', null, 'Before'), h('th', null, 'After'), h('th', null, 'Preview')),
          r.changes.slice(0, 500).map((it) => h('tr', null, h('td', null, where(it), h('div', { class: 'dim small' }, it.func)),
            h('td', { class: 'mono small' }, it.before), h('td', { class: 'mono small' }, it.after), h('td', null, textPreview(it.after))))),
        r.changes.length > 500 ? h('p', { class: 'dim' }, `First 500 of ${r.changes.length} shown.`) : null);
    };
    el.append(h('div', { class: 'card' },
      h('p', { class: 'dim small' }, 'Changes the text of every dialogue line with a plain string (GML). Only the string literal changes. Translations in lang/*.json are not changed. ',
        'Regular expressions: Rust regex syntax, $1 in the replacement.'),
      h('div', { class: 'row' }, inp('find', 'Find'), inp('replace', 'Replace with'), chk('regex', 'Regular expression'), chk('ignore_case', 'Ignore case')),
      h('div', { class: 'row', style: { marginTop: '6px' } }, inp('code', 'Only code entries with this name part', 280), inp('speaker', 'Only this speaker', 160),
        h('button', { onclick: () => run(true) }, 'Preview'), h('button', { class: 'primary', onclick: () => run(false) }, 'Replace all'))), out);
  }

  function checkTab(el) {
    const lim = { ...DEFAULT_LIMITS };
    let font = null;
    const langs = new Set();
    const out = h('div');
    const num = (k, label) => h('label', { class: 'field' }, h('span', null, label),
      h('input', { type: 'number', value: lim[k], min: 0, oninput: (e) => { lim[k] = Number(e.target.value) || 0; } }));
    const pxInputs = h('div', { class: 'fields' });
    const drawPx = () => pxInputs.replaceChildren(...(font ? [num('px', 'Pixels per line'), num('face_px', 'Pixels with a face'),
      h('span', { class: 'dim small' }, `Advance of M: ${font.metrics.fallback}px. Start values: characters × that.`)] : []));
    const langBox = h('div', { class: 'row' });
    const fs = fontSelect(capi, (f) => {
      font = f && f.data.Glyphs?.length ? f : null;
      if (f && !font) toast('This font has no glyph data.', 'error');
      if (font) {
        lim.px = Math.round(lim.chars * fontMetrics(font.data).fallback);
        lim.face_px = Math.round(lim.face_chars * fontMetrics(font.data).fallback);
      }
      drawPx();
    });
    const run = async () => {
      const params = { chars: lim.chars, face_chars: lim.face_chars, lines: lim.lines, langs: [...langs].join(',') };
      if (font) Object.assign(params, { font: font.name, px: lim.px, face_px: lim.face_px });
      const r = await attempt(() => capi.textCheck(params));
      if (!r) return;
      if (!langBox.childElementCount) {
        for (const l of ['en', ...r.languages]) {
          langBox.append(h('label', { class: 'field check' }, h('input', { type: 'checkbox', onchange: (e) => { if (e.target.checked) langs.add(l); else langs.delete(l); } }), h('span', null, l)));
        }
      }
      const cov = Object.entries(r.coverage);
      out.replaceChildren(
        cov.length ? h('div', { class: 'card' }, h('h3', null, 'Translations (lang/*.json)'),
          h('table', null, h('tr', null, h('th', null, 'Language'), h('th', null, 'Keys in code'), h('th', null, 'Missing'), h('th', null, 'Same as English'), h('th', null, 'First missing keys')),
            cov.map(([l, x]) => h('tr', null, h('td', null, l), h('td', null, String(x.keys)), h('td', { style: { color: x.missing ? 'var(--danger)' : '' } }, String(x.missing)),
              h('td', null, String(x.same)), h('td', { class: 'mono small' }, x.missing_keys.slice(0, 20).join(', ')))))) : h('p', { class: 'dim small' }, 'No lang/*.json files in the chapter folder.'),
        h('p', null, `${r.total} texts over the limits${r.total > r.items.length ? ` (first ${r.items.length} shown)` : ''}.`),
        h('table', null, h('tr', null, h('th', null, 'Where'), h('th', null, 'Lang'), h('th', null, 'Problem'), h('th', null, 'Preview')),
          r.items.map((it) => h('tr', null,
            h('td', null, where(it), h('div', { class: 'dim small' }, it.func, it.speaker ? ` · ${it.speaker}` : ''),
              it.lang === 'en' ? h('a', { class: 'small', href: `#/${c}/dialogue?code=${encodeURIComponent(it.code)}` }, 'edit') : null),
            h('td', null, it.lang),
            h('td', { class: 'small' }, it.problems.map((p) => h('div', null, p.kind === 'lines' ? `${p.value} lines > ${p.limit}` : `line ${p.line + 1}: ${Math.round(p.value)} > ${p.limit}`,
              h('div', { class: 'mono dim' }, p.text)))),
            h('td', null, font ? fontCanvas(font, it.text, 1) : textPreview(it.text))))));
    };
    el.append(h('div', { class: 'card' },
      h('p', { class: 'dim small' }, 'Finds lines that are too long for the text box, and boxes with too many lines. Wide (CJK) characters count 2. ',
        'A face (speaker set, or \\E/\\F codes) leaves less room. The limits are estimates: check in the game, or pick the game font to measure in pixels.'),
      h('div', { class: 'fields' }, num('chars', 'Characters per line'), num('face_chars', 'With a face'), num('lines', 'Lines per box'), h('label', { class: 'field' }, h('span', null, 'Font'), fs)),
      pxInputs, h('div', { class: 'row', style: { marginTop: '6px' } }, h('span', { class: 'dim small' }, 'Languages (none: all):'), langBox),
      h('div', { class: 'row', style: { marginTop: '6px' } }, h('button', { class: 'primary', onclick: run }, 'Check'))), out);
    run();
  }

  function fontTab(el) {
    let font = null;
    const ta = h('textarea', { rows: 3, style: { width: '100%' }, value: '* Hello^1, \\cYKris\\cW!&* This is the game font./%' });
    const view = h('div');
    const draw = () => view.replaceChildren(font ? fontCanvas(font, ta.value, 2) : h('p', { class: 'dim' }, 'Pick a font.'), h('div', { style: { marginTop: '6px' } }, textPreview(ta.value)));
    ta.addEventListener('input', draw);
    const fs = fontSelect(capi, (f) => { font = f; draw(); });
    const faceOut = h('div', { class: 'row' });
    el.append(h('div', { class: 'card' }, h('h3', null, 'Font preview'), h('div', { class: 'row' }, fs), ta, codeButtons(capi, ta), view),
      h('div', { class: 'card' }, h('h3', null, 'Face codes'),
        h('p', { class: 'dim small' }, 'Pick a frame of a face sprite to get its \\E code. In the dialogue editor, the face… button puts the code at the cursor.'),
        h('button', { onclick: async () => {
          const code = await pickFace(capi);
          if (!code) return;
          faceOut.replaceChildren(h('span', { class: 'mono' }, code), h('button', { class: 'small', onclick: () => navigator.clipboard?.writeText(code).then(() => toast('Copied', 'ok')) }, 'Copy'));
        } }, 'Pick a face…'), faceOut));
    draw();
  }
}
