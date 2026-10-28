// Draws Deltarune dialogue text the way the text box shows it, with control codes marked.
import { h } from '../ui.js';

const COLORS = { R: '#ff3030', B: '#5050ff', Y: '#ffff00', G: '#00ff00', W: '#ffffff', X: '#000000', O: '#ff9f00', P: '#ff60ff', A: '#80ffff', L: '#40a0ff', S: '#ff80c0', M: '#a03050', '0': '#ffffff' };

/** Returns an element that shows `text` as in the game text box. */
export function textPreview(text) {
  const box = h('div', { class: 'dlg-box' });
  if (text === null || text === undefined) return box;
  let color = '#ffffff';
  let span = null;
  const put = (s) => {
    if (!span || span.style.color !== color) {
      span = h('span', { style: { color } });
      box.append(span);
    }
    span.textContent += s;
  };
  const code = (s) => { box.append(h('span', { class: 'cc' }, s)); span = null; };
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    const rest = text.slice(i);
    if (c === '\\' && /^\\[A-Za-z][A-Za-z0-9]/.test(rest)) {
      const k = text[i + 1], v = text[i + 2];
      if (k === 'c') color = COLORS[v] || '#ffffff';
      code(`\\${k}${v}`);
      i += 2;
    } else if (c === '^' && /^\^\d/.test(rest)) {
      code(`^${text[i + 1]}`);
      i += 1;
    } else if (c === '&' || c === '\n') {
      put('\n');
    } else if ((c === '/' || c === '%') && /^[/%]*$/.test(rest)) {
      code(rest);
      break;
    } else if (c === '~' && /^~\d/.test(rest)) {
      put(`[${text[i + 1]}]`);
      i += 1;
    } else {
      put(c);
    }
  }
  return box;
}

/** Short display text for a GML value: string literals and stringsetloc("...") show their text. */
export function displayExpr(expr) {
  if (!expr) return '';
  let m = expr.match(/^(?:stringsetloc|stringset|stringsetsubloc)\s*\(\s*"((?:[^"\\]|\\.)*)"/);
  if (m) return m[1];
  m = expr.match(/^"((?:[^"\\]|\\.)*)"$/);
  if (m) return m[1];
  m = expr.match(/^scr_84_get_lang_string\s*\(\s*"([^"]*)"/);
  if (m) return `[${m[1]}]`;
  return expr;
}
