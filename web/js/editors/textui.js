// Text parts shared by the dialogue editor and the text tools: game font drawing, face (portrait) picker, code buttons.
import { h, loadImage, modal, debounce } from '../ui.js';
import { visibleText, fontMetrics, faceCode, cells, checkText } from './textcore.js';

const COLORS = { R: '#ff3030', B: '#5050ff', Y: '#ffff00', G: '#00ff00', W: '#ffffff', O: '#ff9f00', P: '#ff60ff', A: '#80ffff', L: '#40a0ff', S: '#ff80c0' };

/** Loads a font asset: {name, data, img, metrics}. The atlas is `<name>.png` (UTMT font export). */
export async function loadFont(capi, name) {
  const a = await capi.read('fonts', name);
  const png = a.files.find((f) => f.endsWith('.png'));
  const img = png ? await loadImage(capi.file('fonts', name, png)) : null;
  return { name, data: a.data, img, metrics: fontMetrics(a.data) };
}

/** Draws text with the font glyphs, 2x, with \cX colours. Lines: `&` and newlines. */
export function fontCanvas(font, text, scale = 2) {
  const glyphs = new Map((font.data.Glyphs || []).map((g) => [g.Character, g]));
  const lineH = font.data.LineHeight || Math.max(8, ...[...glyphs.values()].map((g) => g.SourceHeight));
  // Colour runs: split on \cX and keep the other codes out, line by line.
  const runs = [];
  let color = '#ffffff';
  const raw = (text || '').replace(/[/%]+$/, '');
  for (let i = 0; i < raw.length; i++) {
    if (raw[i] === '\\' && /[A-Za-z]/.test(raw[i + 1] || '') && /[A-Za-z0-9]/.test(raw[i + 2] || '')) {
      if (raw[i + 1] === 'c') color = COLORS[raw[i + 2]] || '#ffffff';
      i += 2;
      continue;
    }
    if (raw[i] === '^' && /\d/.test(raw[i + 1] || '')) { i += 1; continue; }
    runs.push({ ch: raw[i] === '&' ? '\n' : raw[i], color });
  }
  const lines = [[]];
  for (const r of runs) (r.ch === '\n' ? lines.push([]) : lines[lines.length - 1].push(r));
  const width = Math.max(1, ...lines.map((l) => font.metrics.width(l.map((r) => r.ch).join(''))));
  const cv = h('canvas', { class: 'font-canvas', width: Math.ceil((width + 8) * scale), height: Math.ceil((lines.length * lineH + 8) * scale) });
  const g = cv.getContext('2d');
  g.imageSmoothingEnabled = false;
  g.fillStyle = '#000';
  g.fillRect(0, 0, cv.width, cv.height);
  if (!font.img) return cv;
  const tmp = document.createElement('canvas');
  const tg = tmp.getContext('2d');
  lines.forEach((l, li) => {
    let x = 4;
    for (const r of l) {
      const gl = glyphs.get(r.ch.codePointAt(0));
      if (gl && gl.SourceWidth && gl.SourceHeight) {
        tmp.width = gl.SourceWidth;
        tmp.height = gl.SourceHeight;
        tg.globalCompositeOperation = 'source-over';
        tg.drawImage(font.img, gl.SourceX, gl.SourceY, gl.SourceWidth, gl.SourceHeight, 0, 0, gl.SourceWidth, gl.SourceHeight);
        tg.globalCompositeOperation = 'source-in';
        tg.fillStyle = r.color;
        tg.fillRect(0, 0, tmp.width, tmp.height);
        g.drawImage(tmp, (x + (gl.Offset || 0)) * scale, (4 + li * lineH) * scale, gl.SourceWidth * scale, gl.SourceHeight * scale);
      }
      x += gl ? gl.Shift : font.metrics.fallback;
    }
  });
  return cv;
}

/** Font select (fonts of the chapter). `onPick(font|null)`. */
export function fontSelect(capi, onPick, initial = '') {
  const sel = h('select', { title: 'Game font for the preview and pixel widths' }, h('option', { value: '' }, 'Font: box preview'));
  capi.list('fonts').then((r) => {
    for (const it of r.items) sel.append(h('option', { value: it.name, selected: it.name === initial }, it.name));
    if (initial) sel.dispatchEvent(new Event('change'));
  }).catch(() => {});
  sel.addEventListener('change', async () => onPick(sel.value ? await loadFont(capi, sel.value).catch(() => null) : null));
  return sel;
}

/** Face sprites (portraits): names with "face". Resolves with the face code (`\E3`) of a picked frame, or null. */
export async function pickFace(capi) {
  const list = await capi.list('sprites', { q: 'face', limit: 2000 }).catch(() => ({ items: [] }));
  const idx = await capi.index('sprites', ['TextureCount']).catch(() => ({}));
  const q = h('input', { type: 'search', placeholder: 'Filter face sprites' });
  const sprites = h('div', { class: 'list-small', style: { maxHeight: '180px' } });
  const frames = h('div', { class: 'faces' });
  let done = () => {};
  const showFrames = (name) => {
    const n = Math.min(idx[name]?.TextureCount ?? 1, 36);
    frames.replaceChildren(...Array.from({ length: n }, (_, i) => h('div', { class: 'face', title: `${name} frame ${i}`, onclick: () => done(faceCode(i)) },
      h('img', { src: capi.file('sprites', name, `${name}_${i}.png`), loading: 'lazy' }), faceCode(i))));
  };
  const drawList = () => sprites.replaceChildren(...list.items.filter((s) => s.name.includes(q.value)).map((s) => h('div', { onclick: (e) => {
    sprites.querySelectorAll('.sel').forEach((x) => x.classList.remove('sel'));
    e.currentTarget.classList.add('sel');
    showFrames(s.name);
  } }, s.name, h('span', { class: 'dim' }, String(idx[s.name]?.TextureCount ?? '')))));
  q.addEventListener('input', debounce(drawList, 150));
  drawList();
  const body = h('div', { class: 'col', style: { width: 'min(720px, 80vw)' } },
    h('p', { class: 'dim small' }, 'The face code picks a frame of the face sprite of the speaker (scr_speaker / c_speaker). \\E0 to \\E9, then \\EA ... Check the result in the game.'),
    q, sprites, frames);
  return modal('Pick a face', body, [{ label: 'Cancel', value: null }], (close) => { done = close; });
}

/** Inserts text at the cursor of a textarea and sends an input event. */
export function insertAt(ta, text) {
  const s = ta.selectionStart ?? ta.value.length, e = ta.selectionEnd ?? s;
  ta.value = ta.value.slice(0, s) + text + ta.value.slice(e);
  ta.selectionStart = ta.selectionEnd = s + text.length;
  ta.focus();
  ta.dispatchEvent(new Event('input'));
}

/** Code buttons for a dialogue textarea: new line, pause, colours, face, wait, close. */
export function codeButtons(capi, ta) {
  const b = (label, text, title) => h('button', { type: 'button', title, onmousedown: (e) => e.preventDefault(), onclick: () => insertAt(ta, text) }, label);
  const color = h('select', { class: 'small', title: 'Colour', onchange: (e) => { if (e.target.value) insertAt(ta, `\\c${e.target.value}`); e.target.value = ''; } },
    h('option', { value: '' }, 'colour'), Object.keys(COLORS).map((k) => h('option', { value: k, style: { color: COLORS[k] } }, `\\c${k}`)));
  return h('div', { class: 'dlg-tools' }, b('&', '&', 'New line'), b('^1', '^1', 'Pause'), b('/', '/', 'Wait for key'), b('%', '%', 'Close box'), color,
    h('button', { type: 'button', title: 'Face (portrait)', onclick: async () => { const code = await pickFace(capi); if (code) insertAt(ta, code); } }, 'face…'));
}

/** Line widths under a textarea: `33/33 ✓` per line; red when over the limit. */
export function lengthInfo(text, face, lim, font) {
  const probs = checkText(text, face, lim, font);
  const lines = visibleText(text)[0] || [];
  const limit = font && (face ? lim.face_px : lim.px) > 0 ? (face ? lim.face_px : lim.px) : (face ? lim.face_chars : lim.chars);
  const unit = font && (face ? lim.face_px : lim.px) > 0 ? 'px' : '';
  const w = (l) => (unit ? Math.round(font.width(l)) : cells(l));
  return h('div', { class: `dlg-len${probs.length ? ' bad' : ''}` },
    `${lines.map((l) => `${w(l)}${unit}`).join(' | ')} / ${limit}${unit}${face ? ' (face)' : ''}`,
    probs.some((p) => p.kind === 'lines') ? `  ${lines.length} lines > ${lim.lines}` : '');
}
