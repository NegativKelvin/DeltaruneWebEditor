// Text box rules without DOM. Same rules as server/src/text.rs (shared cases: server/tests/fixtures/text_cases.json).

/** Deltarune dialogue box with the default font. */
export const DEFAULT_LIMITS = { chars: 33, face_chars: 26, lines: 3, px: 0, face_px: 0 };

const WIDE = [[0x1100, 0x115f], [0x2e80, 0xa4cf], [0xac00, 0xd7a3], [0xf900, 0xfaff], [0xfe30, 0xfe4f], [0xff00, 0xff60], [0xffe0, 0xffe6]];

/** East Asian wide characters take two cells. */
export function cells(s) {
  let n = 0;
  for (const ch of s) {
    const u = ch.codePointAt(0);
    n += WIDE.some(([a, b]) => u >= a && u <= b) ? 2 : 1;
  }
  return n;
}

/** Visible text of each line, per box: control codes removed, `&` and newlines split lines. */
export function visibleText(text) {
  const chars = [...(text ?? '')];
  const boxes = [['']];
  const isAlpha = (c) => c !== undefined && /^[A-Za-z]$/.test(c);
  const isAlnum = (c) => c !== undefined && /^[A-Za-z0-9]$/.test(c);
  const isDigit = (c) => c !== undefined && /^[0-9]$/.test(c);
  for (let i = 0; i < chars.length; i++) {
    const c = chars[i], next = chars[i + 1];
    const lines = boxes[boxes.length - 1];
    if (c === '\\' && isAlpha(next) && isAlnum(chars[i + 2])) { i += 2; continue; }
    if (c === '^' && isDigit(next)) { i += 1; continue; }
    if (c === '&' || c === '\n') lines.push('');
    else if ((c === '/' || c === '%') && chars.slice(i).every((x) => x === '/' || x === '%')) break;
    else if (c === '~' && isDigit(next)) { lines[lines.length - 1] += `[${next}]`; i += 1; }
    else lines[lines.length - 1] += c;
  }
  return boxes;
}

/** A face shows: the speaker is a character, or the text changes the face. */
export function hasFace(text, speaker) {
  return (!!speaker && speaker !== 'no_name') || /\\[EF]/.test(text || '');
}

/** Advances of a UTMT font JSON (`Glyphs[].Shift`). */
export function fontMetrics(fontData) {
  const advance = new Map((fontData?.Glyphs || []).map((g) => [g.Character, g.Shift]));
  const fallback = advance.get(77) ?? Math.max(0, ...advance.values());
  return { advance, fallback, width: (s) => [...s].reduce((w, ch) => w + (advance.get(ch.codePointAt(0)) ?? fallback), 0) };
}

/** Problems of one text: [{box, line, kind: 'width'|'lines', value, limit, text}]. */
export function checkText(text, face, lim = DEFAULT_LIMITS, font = null) {
  const chars = face ? lim.face_chars : lim.chars;
  const px = face ? lim.face_px : lim.px;
  const out = [];
  visibleText(text).forEach((lines, bi) => {
    lines.forEach((l, li) => {
      const [value, limit] = font && px > 0 ? [font.width(l), px] : [cells(l), chars];
      if (value > limit) out.push({ box: bi, line: li, kind: 'width', value, limit, text: l });
    });
    if (lines.length > lim.lines) out.push({ box: bi, line: lim.lines, kind: 'lines', value: lines.length, limit: lim.lines, text: lines.slice(lim.lines).join(' / ') });
  });
  return out;
}

/** Face code for frame `n` of the face sprite: `\E0`..`\E9`, then `\EA`... */
export function faceCode(n) {
  return `\\E${n < 10 ? n : String.fromCharCode(55 + n)}`;
}
