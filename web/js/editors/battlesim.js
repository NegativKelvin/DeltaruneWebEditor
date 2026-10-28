// Bullet pattern simulator. Same rules and order as the GML that the server makes (server/src/patterns.rs):
// each step 1. box keyframes, 2. bullets of the wave (life, speed change, turn), 3. emitters fire,
// 4. GameMaker motion (gravity, then position). Screen pixels 640×480, camera at 0,0. No DOM: `node --test` checks it.

export const SCREEN_W = 640, SCREEN_H = 480;

export const DEFAULT_EMITTER = {
  name: 'ring', enabled: true, start: 10, end: 200, interval: 20, origin: 'box', x: 0, y: -140, jitter_x: 0, jitter_y: 0, radius: 0,
  count: 8, angle: 270, spread: 90, spin: 7, aim: false, jitter_angle: 0, speed: 3, speed_step: 0, accel: 0, gravity: 0, gravity_dir: 270,
  turn: 0, life: 0, sprite: '', scale: 1, rotate: false, size: 6, color: '#ffffff', damage: 0, on_spawn: '',
};
export const DEFAULT_BOX = { frame: 0, x: 320, y: 170, w: 150, h: 150 };
export const DEFAULT_SOUL = { speed: 4, slow: 2, radius: 4, graze: 24, iframes: 30, hp: 100 };
export const DEFAULT_PATTERN = {
  duration: 240, bullet_object: 'obj_collidebullet', box: DEFAULT_BOX, box_keys: [], box_code: false, soul: DEFAULT_SOUL,
  emitters: [DEFAULT_EMITTER], notes: '',
};

/** Fills missing fields with the defaults (the server does the same). */
export function normalize(p = {}) {
  return {
    ...DEFAULT_PATTERN,
    ...p,
    box: { ...DEFAULT_BOX, ...(p.box || {}) },
    box_keys: (p.box_keys || []).map((k) => ({ ...DEFAULT_BOX, ...k })),
    soul: { ...DEFAULT_SOUL, ...(p.soul || {}) },
    emitters: (p.emitters || DEFAULT_PATTERN.emitters).map((e) => ({ ...DEFAULT_EMITTER, ...e })),
  };
}

const RAD = Math.PI / 180;
export const lengthdirX = (r, a) => r * Math.cos(a * RAD);
export const lengthdirY = (r, a) => -r * Math.sin(a * RAD);
export function pointDirection(x1, y1, x2, y2) {
  const d = Math.atan2(-(y2 - y1), x2 - x1) / RAD;
  return d < 0 ? d + 360 : d + 0;
}

/** Seeded random numbers (mulberry32), so a test run can be played again the same way. */
export function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Box at step `t`: keys sorted, main box at frame 0, linear between keys (as the GML). */
export function boxAt(p, t) {
  const keys = [{ ...p.box, frame: 0 }, ...p.box_keys.filter((k) => k.frame > 0)].sort((a, b) => a.frame - b.frame);
  let b = keys[0];
  for (let i = 0; i + 1 < keys.length; i++) {
    const a = keys[i], c = keys[i + 1];
    if (t >= a.frame && t < c.frame) {
      const k = (t - a.frame) / (c.frame - a.frame);
      const l = (u, v) => u + (v - u) * k;
      return { x: l(a.x, c.x), y: l(a.y, c.y), w: l(a.w, c.w), h: l(a.h, c.h) };
    }
  }
  if (keys.length > 1 && t >= keys[keys.length - 1].frame) b = keys[keys.length - 1];
  return { x: b.x, y: b.y, w: b.w, h: b.h };
}

/** Angle offset of bullet i of `count` in a shot (as the GML). */
export function spreadOffset(e, i) {
  if (e.count <= 1) return 0;
  if (Math.abs(e.spread) >= 360) return (e.spread * i) / e.count;
  return -e.spread / 2 + (e.spread * i) / (e.count - 1);
}

/**
 * Makes a simulation. `pattern` must be normalized. Returns an object with `step(input)`:
 * input {left, right, up, down, slow}; without input the soul stays still.
 */
export function createSim(pattern, { seed = 1, soul: soulAt = null } = {}) {
  const p = pattern;
  const rand = rng(seed);
  const range = (v) => (v ? (rand() * 2 - 1) * v : 0);
  const b0 = boxAt(p, 0);
  const sim = {
    t: 0,
    bullets: [],
    box: b0,
    soul: { x: soulAt ? soulAt.x : b0.x, y: soulAt ? soulAt.y : b0.y, inv: 0 },
    hp: p.soul.hp,
    hits: 0,
    grazes: 0,
    spawned: 0,
    events: [], // {t, kind: 'hit'|'graze', emitter}
    get done() { return this.t > p.duration; },
    step,
  };

  function moveSoul(input) {
    const s = sim.soul;
    if (input) {
      const sp = input.slow ? p.soul.slow : p.soul.speed;
      s.x += ((input.right ? 1 : 0) - (input.left ? 1 : 0)) * sp;
      s.y += ((input.down ? 1 : 0) - (input.up ? 1 : 0)) * sp;
    }
    // The soul stays inside the box.
    const m = 8, bx = sim.box;
    const hw = Math.max(0, bx.w / 2 - m), hh = Math.max(0, bx.h / 2 - m);
    s.x = Math.max(bx.x - hw, Math.min(bx.x + hw, s.x));
    s.y = Math.max(bx.y - hh, Math.min(bx.y + hh, s.y));
    if (s.inv > 0) s.inv--;
  }

  function fire(e, ei) {
    const t = sim.t;
    if (!e.enabled || t < e.start || t > e.end || (t - e.start) % e.interval !== 0) return;
    const n = Math.floor((t - e.start) / e.interval);
    const o = e.origin === 'soul' ? sim.soul : e.origin === 'screen' ? { x: 0, y: 0 } : sim.box;
    const ox = o.x + e.x, oy = o.y + e.y;
    let base = e.angle + e.spin * n;
    if (e.aim) base += pointDirection(ox, oy, sim.soul.x, sim.soul.y);
    for (let i = 0; i < e.count; i++) {
      const a = base + spreadOffset(e, i) + range(e.jitter_angle);
      sim.bullets.push({
        em: ei,
        x: ox + lengthdirX(e.radius, a) + range(e.jitter_x),
        y: oy + lengthdirY(e.radius, a) + range(e.jitter_y),
        dir: a,
        speed: e.speed + e.speed_step * i,
        life: e.life === 0 ? -1 : e.life,
        angle: e.rotate ? a : 0,
        grazed: false,
      });
      sim.spawned++;
    }
  }

  function step(input) {
    if (sim.done) return;
    const t = sim.t;
    sim.box = boxAt(p, t);
    // Bullets of this wave.
    const keep = [];
    for (const b of sim.bullets) {
      if (b.life > 0) b.life--;
      if (b.life === 0 || b.x < -320 || b.x > SCREEN_W + 320 || b.y < -320 || b.y > SCREEN_H + 320) continue;
      const e = p.emitters[b.em];
      b.speed += e.accel;
      b.dir += e.turn;
      if (e.rotate) b.angle = b.dir;
      keep.push(b);
    }
    sim.bullets = keep;
    p.emitters.forEach(fire);
    // Motion: gravity changes hspeed/vspeed, then the position moves.
    for (const b of sim.bullets) {
      const e = p.emitters[b.em];
      if (e.gravity) {
        const vx = lengthdirX(b.speed, b.dir) + lengthdirX(e.gravity, e.gravity_dir);
        const vy = lengthdirY(b.speed, b.dir) + lengthdirY(e.gravity, e.gravity_dir);
        b.speed = Math.hypot(vx, vy);
        b.dir = pointDirection(0, 0, vx, vy);
        if (e.rotate) b.angle = b.dir;
      }
      b.x += lengthdirX(b.speed, b.dir);
      b.y += lengthdirY(b.speed, b.dir);
    }
    moveSoul(input);
    // Hits and grazes (circles; the game uses the sprite masks).
    const s = sim.soul;
    for (const b of sim.bullets) {
      const e = p.emitters[b.em];
      const d = Math.hypot(b.x - s.x, b.y - s.y);
      if (d < e.size + p.soul.radius) {
        if (s.inv === 0) {
          s.inv = p.soul.iframes;
          sim.hits++;
          sim.hp -= e.damage > 0 ? e.damage : 10;
          sim.events.push({ t, kind: 'hit', emitter: b.em });
        }
      } else if (!b.grazed && d < e.size + p.soul.graze) {
        b.grazed = true;
        sim.grazes++;
        sim.events.push({ t, kind: 'graze', emitter: b.em });
      }
    }
    sim.t++;
  }

  return sim;
}

/** Runs a simulation without input to step `t` (for the timeline). */
export function simulateTo(pattern, t, opts) {
  const sim = createSim(pattern, opts);
  while (sim.t < t && !sim.done) sim.step(null);
  return sim;
}
