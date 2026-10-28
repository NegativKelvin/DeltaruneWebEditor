// Code text editor with GML / GLSL highlighting, line numbers, tab indent, and go-to-line.
import { h } from '../ui.js';

const GML_KW = new Set(('if else while do until for repeat switch case default break continue return exit with var globalvar ' +
  'function new delete throw try catch finally enum static and or not xor div mod true false undefined self other all noone global ' +
  'argument argument0 argument1 argument2 argument3 argument4 argument5 argument_count instance_destroy').split(' '));
const GLSL_KW = new Set(('attribute uniform varying const void float int bool vec2 vec3 vec4 mat2 mat3 mat4 sampler2D ' +
  'if else for while return discard precision lowp mediump highp in out inout struct true false break continue ' +
  '#define #ifdef #ifndef #endif #else #if #version').split(' '));

const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const TOKEN = /(\/\/[^\n]*|\/\*[\s\S]*?(?:\*\/|$))|(@?"(?:[^"\\\n]|\\.)*"?|'(?:[^'\\\n]|\\.)*'?)|(\$[0-9a-fA-F]+|\b\d+(?:\.\d+)?\b)|(#?[A-Za-z_]\w*)(\s*\()?/g;

export function highlight(src, lang = 'gml') {
  const kw = lang === 'glsl' ? GLSL_KW : GML_KW;
  let out = '';
  let last = 0;
  src.replace(TOKEN, (m, com, str, num, id, call, at) => {
    out += esc(src.slice(last, at));
    last = at + m.length;
    if (com) out += `<span class="tok-com">${esc(com)}</span>`;
    else if (str) out += `<span class="tok-str">${esc(str)}</span>`;
    else if (num) out += `<span class="tok-num">${num}</span>`;
    else if (id) {
      let cls = '';
      if (kw.has(id)) cls = 'tok-kw';
      else if (call) cls = 'tok-fn';
      else if (id.startsWith('gm_') || id.startsWith('obj_') || id.startsWith('spr_') || id.startsWith('snd_') || id.startsWith('room_')) cls = 'tok-glob';
      out += cls ? `<span class="${cls}">${id}</span>` : id;
      if (call) out += esc(call);
    }
    return m;
  });
  return out + esc(src.slice(last));
}

/**
 * opts: {value, lang, readOnly, onChange(value), onSave(), minHeight}
 * Returns {el, value (get/set), goto(line), focus()}.
 */
export function codeEditor(opts) {
  const lang = opts.lang || 'gml';
  const gutter = h('div', { class: 'gutter' });
  const pre = h('pre', { 'aria-hidden': 'true' });
  const ta = h('textarea', { spellcheck: false, wrap: 'off', readOnly: !!opts.readOnly, autocapitalize: 'off', autocomplete: 'off' });
  const scroller = h('div', { class: 'scroller' }, pre, ta);
  const el = h('div', { class: 'code-edit', style: { height: opts.height || '60vh', minHeight: opts.minHeight || '200px' } }, gutter, scroller);
  ta.value = opts.value || '';
  let markLine = 0;
  let queued = false;

  const paint = () => {
    queued = false;
    const v = ta.value;
    let html = highlight(v, lang);
    if (markLine) {
      const lines = html.split('\n');
      if (lines[markLine - 1] !== undefined) lines[markLine - 1] = `<span class="hl-line">${lines[markLine - 1] || ' '}</span>`;
      html = lines.join('\n');
    }
    pre.innerHTML = html + '\n';
    const n = v.split('\n').length;
    if (gutter.childElementCount !== n) gutter.textContent = Array.from({ length: n }, (_, i) => i + 1).join('\n') + '\n';
    ta.style.height = 'auto';
    ta.style.width = 'auto';
    ta.style.height = `${Math.max(ta.scrollHeight, scroller.clientHeight)}px`;
    ta.style.width = `${Math.max(ta.scrollWidth, scroller.clientWidth)}px`;
  };
  const queue = () => { if (!queued) { queued = true; requestAnimationFrame(paint); } };

  ta.addEventListener('input', () => { markLine = 0; queue(); opts.onChange?.(ta.value); });
  scroller.addEventListener('scroll', () => { gutter.scrollTop = scroller.scrollTop; });
  ta.addEventListener('keydown', (e) => {
    if (e.key === 'Tab' && !opts.readOnly) {
      e.preventDefault();
      const { selectionStart: s, selectionEnd: t } = ta;
      if (s === t) {
        ta.setRangeText('    ', s, t, 'end');
      } else {
        // Indent or outdent the selected lines.
        const ls = ta.value.lastIndexOf('\n', s - 1) + 1;
        const block = ta.value.slice(ls, t);
        const out = e.shiftKey ? block.replace(/^ {1,4}/gm, '') : block.replace(/^/gm, '    ');
        ta.setRangeText(out, ls, t, 'select');
      }
      ta.dispatchEvent(new Event('input'));
    } else if (e.key === 'Enter' && !opts.readOnly) {
      e.preventDefault();
      const s = ta.selectionStart;
      const ls = ta.value.lastIndexOf('\n', s - 1) + 1;
      const indent = ta.value.slice(ls, s).match(/^\s*/)[0];
      const extra = /[{(]\s*$/.test(ta.value.slice(ls, s)) ? '    ' : '';
      ta.setRangeText('\n' + indent + extra, s, ta.selectionEnd, 'end');
      ta.dispatchEvent(new Event('input'));
    } else if ((e.ctrlKey || e.metaKey) && e.key === 's') {
      e.preventDefault();
      opts.onSave?.();
    }
  });
  new ResizeObserver(queue).observe(scroller);
  paint();

  return {
    el,
    get value() { return ta.value; },
    set value(v) { ta.value = v; queue(); },
    goto(line) {
      markLine = line;
      paint();
      const y = (line - 1) * 18;
      scroller.scrollTop = Math.max(0, y - scroller.clientHeight / 3);
      const lines = ta.value.split('\n');
      const pos = lines.slice(0, line - 1).reduce((a, l) => a + l.length + 1, 0);
      ta.setSelectionRange(pos, pos);
    },
    focus() { ta.focus(); },
  };
}
