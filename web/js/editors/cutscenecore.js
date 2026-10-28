// Cutscene rules without DOM: argument types of c_* functions, a GML call parser, the timeline model, and the state graph layout.

// Argument types: dir ("l" "r" "u" "d"), actor (kr su ra ...), frames, num, text (string), bool (0/1), and asset kinds.
// Hints only: any function and argument list is accepted.
export const CATALOG = {
  c_sel: ['actor'], c_walk: ['dir:dir', 'speed:num', 'frames:frames'], c_walk_wait: ['dir:dir', 'speed:num', 'frames:frames'],
  c_walkdirect: ['x:num', 'y:num', 'frames:frames'], c_walkdirect_wait: ['x:num', 'y:num', 'frames:frames'], c_wait: ['frames:frames'],
  c_facing: ['dir:dir'], c_sprite: ['sprite:sprites'], c_autowalk: ['on:bool'], c_imagespeed: ['speed:num'], c_imageindex: ['index:num'],
  c_speaker: ['name:text'], c_msgside: ['side:text'], c_msgsetloc: ['index:num', 'text:text', 'key:text'], c_msgnextloc: ['text:text', 'key:text'],
  c_msgsetsubloc: ['index:num', 'text:text', 'sub1', 'key:text'], c_msgnextsubloc: ['text:text', 'sub1', 'key:text'],
  c_fe: ['face:num'], c_talk: [], c_talk_wait: [], c_wait_talk: [], c_wait_box: ['box:num'], c_pan: ['x:num', 'y:num', 'frames:frames'],
  c_pan_wait: ['x:num', 'y:num', 'frames:frames'], c_panobj: ['object:objects', 'frames:frames'], c_pannable: ['on:bool'],
  c_soundplay: ['sound:sounds'], c_snd_play: ['sound:sounds'], c_mus: ['command:text'], c_mus2: ['command:text', 'arg'], c_shake: [],
  c_emote: ['type:text', 'frames:frames'], c_setxy: ['x:num', 'y:num'], c_addxy: ['x:num', 'y:num'], c_visible: ['on:bool'], c_depth: ['depth:num'],
  c_flip: ['axis:text'], c_halt: [], c_actortokris: [], c_actortocaterpillar: [], c_terminatekillactors: [],
  c_jump: ['x:num', 'y:num', 'height:num', 'frames:frames'], c_var_instance: ['instance', 'variable:text', 'value'],
  c_var_lerp_instance: ['instance', 'variable:text', 'from:num', 'to:num', 'frames:frames'], c_script_instance: ['instance', 'script', 'arg'],
  c_instance: ['x:num', 'y:num', 'object:objects'], c_waitcustom: [], c_waitcustom_end: [], c_customfunc: ['function'],
};

export const ACTORS = ['kr', 'su', 'ra', 'no', 'kr_actor', 'su_actor', 'ra_actor'];
export const DIRS = ['"r"', '"l"', '"u"', '"d"'];

/** [{name, type}] of a function's arguments. */
export function argSpec(func) {
  return (CATALOG[func] || []).map((s) => {
    const [name, type = name === 'actor' ? 'actor' : 'gml'] = s.split(':');
    return { name, type };
  });
}

/** Splits `a, f(b, c), "d, e"` at top-level commas. */
export function splitArgs(s) {
  const out = [];
  let depth = 0, cur = '', q = null;
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (q) {
      cur += c;
      if (c === '\\' && i + 1 < s.length) cur += s[++i];
      else if (c === q) q = null;
      continue;
    }
    if (c === '"' || c === "'") q = c;
    else if ('([{'.includes(c)) depth++;
    else if (')]}'.includes(c)) depth--;
    else if (c === ',' && depth === 0) { out.push(cur.trim()); cur = ''; continue; }
    cur += c;
  }
  if (cur.trim() || out.length) out.push(cur.trim());
  return out;
}

/** Parses one line `c_walk("r", 4, 10);` into {func, args}. Returns null for other text. */
export function parseCall(line) {
  const m = /^\s*(c_\w+)\s*\(([\s\S]*)\)\s*;?\s*$/.exec(line);
  return m ? { func: m[1], args: splitArgs(m[2]) } : null;
}

/** GML of one step, as the server writes it. */
export function stepGml(s) {
  return `${s.func}(${s.args.map((a) => a.trim()).join(', ')});`;
}

const BLOCKING = /^c_(wait|walk_wait|walkdirect_wait|pan_wait|talk_wait|wait_talk|wait_box|waitcustom)$/;
const TEXT = /^c_(msg|talk|wait_talk|speaker|fe$)/;
const CAMERA = /^c_pan/;
const SOUND = /^c_(sound|snd|mus)/;
/** Steps that wait for the text box: the time is not known, the timeline uses this many steps. */
export const TALK_FRAMES = 45;

function frameArg(s) {
  const spec = argSpec(s.func);
  const i = spec.findIndex((a) => a.type === 'frames');
  if (i < 0) return null;
  const v = Number(s.args[i]);
  return Number.isFinite(v) && v >= 0 ? v : null;
}

/**
 * Timeline of a list of steps. The cutscene master runs steps in order; only wait steps stop it
 * (`c_wait`, `*_wait`, `c_talk_wait`). Others start at once and can run for their frames (a walk).
 * Returns {items: [{i, start, dur, lane, blocking, known}], length, lanes}.
 */
export function timeline(steps) {
  let t = 0;
  let actor = 'kr';
  const items = [];
  const lanes = [];
  const lane = (n) => { if (!lanes.includes(n)) lanes.push(n); return n; };
  steps.forEach((s, i) => {
    if (s.func === 'c_sel' && s.args[0]) actor = s.args[0].trim();
    let f = frameArg(s);
    const blocking = BLOCKING.test(s.func);
    const known = f !== null || !/talk|box|custom/.test(s.func);
    if (f === null) f = /talk|box/.test(s.func) ? TALK_FRAMES : 0;
    const l = TEXT.test(s.func) ? 'text' : CAMERA.test(s.func) ? 'camera' : SOUND.test(s.func) ? 'sound' : s.func === 'c_wait' ? 'wait' : `actor ${actor}`;
    items.push({ i, start: t, dur: f, lane: lane(l), blocking, known });
    if (blocking) t += f;
  });
  const length = Math.max(t, ...items.map((x) => x.start + x.dur), 1);
  const order = ['text', 'camera', 'sound', 'wait'];
  lanes.sort((a, b) => (order.indexOf(a) + 1 || 99) - (order.indexOf(b) + 1 || 99) || a.localeCompare(b));
  return { items, length, lanes };
}

/** Moves of `c_walk(dir, speed, frames)` and `c_setxy`/`c_addxy` per actor, for the stage at frame `t`. Start: 0,0. */
export function actorPositions(steps, t) {
  const tl = timeline(steps);
  const pos = {};
  let actor = 'kr';
  steps.forEach((s, i) => {
    if (s.func === 'c_sel' && s.args[0]) actor = s.args[0].trim();
    const it = tl.items[i];
    if (it.start > t) return;
    const p = (pos[actor] ||= { x: 0, y: 0 });
    const num = (k) => Number(s.args[k]);
    if (s.func === 'c_setxy' && Number.isFinite(num(0)) && Number.isFinite(num(1))) { p.x = num(0); p.y = num(1); }
    if (s.func === 'c_addxy' && Number.isFinite(num(0)) && Number.isFinite(num(1))) { p.x += num(0); p.y += num(1); }
    if (/^c_walk(_wait)?$/.test(s.func)) {
      const d = (s.args[0] || '').replace(/["']/g, '');
      const sp = num(1), fr = num(2);
      if (!Number.isFinite(sp) || !Number.isFinite(fr)) return;
      const k = Math.min(t - it.start, fr) * sp;
      if (d === 'r') p.x += k; else if (d === 'l') p.x -= k; else if (d === 'd') p.y += k; else if (d === 'u') p.y -= k;
    }
  });
  return pos;
}

/** The text that shows at frame `t`: the last message set before `t`. */
export function textAt(steps, t) {
  const tl = timeline(steps);
  let text = null;
  steps.forEach((s, i) => {
    if (tl.items[i].start > t || !/^c_msg/.test(s.func)) return;
    const spec = argSpec(s.func);
    const k = spec.findIndex((a) => a.name === 'text');
    const raw = (s.args[k >= 0 ? k : 0] || '').trim();
    const m = /^"((?:[^"\\]|\\.)*)"$/.exec(raw);
    if (m) text = m[1].replace(/\\(["\\])/g, '$1');
  });
  return text;
}

/**
 * Layout of the `con` state graph: columns by order of the con value. Sections: [{con, next}].
 * Returns {nodes: [{con, x, y}], edges: [{from, to, back}]} in grid units.
 */
export function graphLayout(sections) {
  const cons = [...new Set(sections.map((s) => s.con))];
  const num = (c) => (Number.isFinite(Number(c)) ? Number(c) : Infinity);
  cons.sort((a, b) => num(a) - num(b) || String(a).localeCompare(String(b)));
  const nodes = cons.map((con, i) => ({ con, x: i, y: 0 }));
  const at = new Map(nodes.map((n) => [n.con, n]));
  const edges = [];
  for (const s of sections) {
    for (const to of s.next) {
      if (!at.has(to)) {
        const n = { con: to, x: nodes.length, y: 0, missing: true };
        nodes.push(n);
        at.set(to, n);
      }
      edges.push({ from: s.con, to, back: at.get(to).x <= at.get(s.con).x });
    }
  }
  return { nodes, edges };
}
