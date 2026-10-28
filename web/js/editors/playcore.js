// Room preview rules without DOM: collision boxes, solid objects, movement, door targets.
// Kept apart from roomplay.js so that `node --test` can check them.

const SOLID_RE = /^obj_(solid|solidblock|solidenemy|soliddark|solidcustom|collider)/i;
const TRIGGER_RE = /trig|door|marker|event|interact|readable|warp|entrance|savepoint|save_point|cutscene|area|zone|npc|chest|switch/i;

/** Walks the parent chain of an object (the object first). Stops on loops. */
export function ancestors(obj, objIdx) {
  const out = [];
  const seen = new Set();
  for (let o = obj; o && !seen.has(o); o = objIdx[o]?.ParentObject) {
    seen.add(o);
    out.push(o);
  }
  return out;
}

/** An object blocks the player if it (or a parent) is a Deltarune solid object or has the Solid flag. */
export function isSolidObject(obj, objIdx) {
  return ancestors(obj, objIdx).some((o) => SOLID_RE.test(o) || objIdx[o]?.Solid === true);
}

/** A trigger: door, marker, event, or invisible object, or an instance with creation code. Solids are not triggers. */
export function isTriggerInstance(inst, objIdx) {
  const o = inst.ObjectName || '';
  if (isSolidObject(o, objIdx)) return false;
  return ancestors(o, objIdx).some((a) => TRIGGER_RE.test(a)) || objIdx[o]?.Visible === false || !!inst.CreationCodeEntry;
}

/**
 * Collision box [x1, y1, x2, y2] of an instance from the sprite margins (mask sprite first).
 * UTMT leaves out fields with the default value, so a missing MarginRight/Bottom means the full sprite.
 */
export function instBBox(inst, objIdx, sprIdx) {
  const o = objIdx[inst.ObjectName] || {};
  const s = sprIdx[o.MaskSprite] || sprIdx[o.Sprite];
  const sx = inst.ScaleX ?? 1, sy = inst.ScaleY ?? 1;
  if (!s) return spriteBBox({ Width: 16, Height: 16 }, inst.X, inst.Y, sx, sy);
  return spriteBBox(s, inst.X, inst.Y, sx, sy);
}

export function spriteBBox(s, x, y, sx = 1, sy = 1) {
  const l = s.MarginLeft ?? 0, t = s.MarginTop ?? 0;
  const r = (s.MarginRight ?? (s.Width || 16) - 1) + 1, b = (s.MarginBottom ?? (s.Height || 16) - 1) + 1;
  const ox = s.OriginX ?? 0, oy = s.OriginY ?? 0;
  const xa = x + (l - ox) * sx, xb = x + (r - ox) * sx, ya = y + (t - oy) * sy, yb = y + (b - oy) * sy;
  return [Math.min(xa, xb), Math.min(ya, yb), Math.max(xa, xb), Math.max(ya, yb)];
}

export const overlaps = (a, b) => a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1];

/**
 * Moves box `b` by (dx, dy), one axis at a time, and stops at solid boxes (slides along walls).
 * Returns the distance moved [mx, my].
 */
export function moveBox(b, dx, dy, solids) {
  // Solids that already overlap the box do not block (so Kris can walk out of a wall he starts in).
  solids = solids.filter((s) => !overlaps(b, s));
  const hits = (x, y) => solids.some((s) => overlaps([b[0] + x, b[1] + y, b[2] + x, b[3] + y], s));
  if (!solids.length) { b[0] += dx; b[2] += dx; b[1] += dy; b[3] += dy; return [dx, dy]; }
  // Pixel by pixel, so that a fast move does not jump over a thin wall.
  const axis = (d, vert) => {
    let moved = 0;
    while (moved !== d) {
      const st = Math.sign(d - moved) * Math.min(1, Math.abs(d - moved));
      if (vert ? hits(0, moved + st) : hits(moved + st, 0)) break;
      moved += st;
    }
    return moved;
  };
  const mx = axis(dx, false);
  b[0] += mx; b[2] += mx;
  const my = axis(dy, true);
  b[1] += my; b[3] += my;
  return [mx, my];
}

/** Letter of a Deltarune door object: obj_doorA -> 'A'. The next room puts the player on obj_markerA. */
export function doorLetter(obj) {
  const m = /door_?([A-Z])(?![a-z])/.exec(obj || '');
  return m ? m[1] : null;
}

/**
 * Rooms that a code text can go to from `current`. Reads `room_goto(x)` and `room_x` names. When the code tests
 * `room == current`, only the rooms in that branch count. `rooms`: set of room names in the chapter.
 */
export function roomTargets(code, current, rooms) {
  if (!code) return [];
  const names = (text) => [...text.matchAll(/\b(room_[A-Za-z0-9_]+|[A-Za-z0-9_]+)\b/g)].map((m) => m[1]).filter((n) => n !== current && rooms.has(n));
  const tests = [...code.matchAll(new RegExp(`\\broom\\s*==\\s*${escapeRe(current)}\\b`, 'g'))];
  let found;
  if (tests.length) {
    found = [];
    for (const t of tests) {
      const rest = code.slice(t.index + t[0].length);
      const next = rest.search(/\broom\s*==/);
      found.push(...names(next >= 0 ? rest.slice(0, next) : rest));
    }
  } else {
    const gotos = [...code.matchAll(/room_goto\s*\(\s*([A-Za-z0-9_]+)\s*\)/g)].map((m) => m[1]).filter((n) => rooms.has(n) && n !== current);
    found = gotos.length ? gotos : names(code);
  }
  return [...new Set(found)];
}

function escapeRe(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** Picks the player sprites. Dark world uses the `_dark` set when it exists. */
export function playerSprites(sprIdx, dark) {
  const pick = (base) => (dark && sprIdx[`${base}_dark`] ? `${base}_dark` : sprIdx[base] ? base : null);
  return { down: pick('spr_krisd'), up: pick('spr_krisu'), left: pick('spr_krisl'), right: pick('spr_krisr') };
}

/** Rooms with these words are light world rooms (Kris is drawn at 1x). */
export function guessDark(roomName) {
  return !/town|school|light|house|hospital|diner|church|library|alley|torroom|krisroom|beach|lw_|_lw/i.test(roomName || '');
}
