// node --test web/tests
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import {
  DEFAULT_PATTERN, normalize, createSim, simulateTo, boxAt, spreadOffset, pointDirection, lengthdirX, lengthdirY,
} from '../js/editors/battlesim.js';
import {
  isSolidObject, isTriggerInstance, spriteBBox, moveBox, doorLetter, roomTargets, playerSprites, guessDark,
} from '../js/editors/playcore.js';

test('defaults match the server', () => {
  const f = JSON.parse(readFileSync(new URL('../../server/tests/fixtures/default_pattern.json', import.meta.url)));
  assert.deepEqual(DEFAULT_PATTERN, f);
});

test('GameMaker angles', () => {
  assert.equal(pointDirection(0, 0, 10, 0), 0);
  assert.equal(pointDirection(0, 0, 0, -10), 90);
  assert.equal(pointDirection(0, 0, 0, 10), 270);
  assert.ok(Math.abs(lengthdirY(5, 90) + 5) < 1e-9);
  assert.ok(Math.abs(lengthdirX(5, 180) + 5) < 1e-9);
});

test('spread offsets as the GML', () => {
  const e = { count: 5, spread: 90 };
  assert.deepEqual([0, 1, 2, 3, 4].map((i) => spreadOffset(e, i)), [-45, -22.5, 0, 22.5, 45]);
  assert.deepEqual([0, 1, 2, 3].map((i) => spreadOffset({ count: 4, spread: 360 }, i)), [0, 90, 180, 270]);
  assert.equal(spreadOffset({ count: 1, spread: 90 }, 0), 0);
});

test('emitters fire on the interval with spin', () => {
  const p = normalize({ duration: 100, emitters: [{ start: 0, end: 40, interval: 10, count: 2, spread: 10, spin: 5, angle: 0, speed: 2 }] });
  const sim = createSim(p);
  sim.step(null);
  assert.equal(sim.bullets.length, 2);
  // Moved one step after the shot.
  assert.ok(Math.abs(sim.bullets[0].x - (320 + lengthdirX(2, -5))) < 1e-9);
  while (sim.t < 11) sim.step(null);
  assert.equal(sim.spawned, 4);
  assert.equal(sim.bullets[2].dir, 5 - 5);
  const end = simulateTo(p, 101);
  assert.equal(end.spawned, 10);
  assert.ok(end.done);
});

test('life, acceleration, gravity', () => {
  const p = normalize({ emitters: [{ start: 0, end: 0, interval: 1, count: 1, angle: 0, speed: 1, accel: 1, life: 3, x: 0, y: 0 }] });
  const sim = createSim(p);
  sim.step(null); // made, moved 1
  sim.step(null); // speed 2
  sim.step(null); // speed 3
  assert.equal(sim.bullets.length, 1);
  assert.equal(sim.bullets[0].x, 320 + 1 + 2 + 3);
  sim.step(null); // life 0: gone
  assert.equal(sim.bullets.length, 0);
  const g = normalize({ emitters: [{ start: 0, end: 0, count: 1, angle: 0, speed: 0, gravity: 0.5, gravity_dir: 270, x: 0, y: 0 }] });
  const s2 = createSim(g);
  s2.step(null);
  s2.step(null);
  assert.ok(Math.abs(s2.bullets[0].y - (170 + 0.5 + 1)) < 1e-9);
});

test('aim, hits, i-frames, grazes, soul stays in box', () => {
  const p = normalize({ soul: { iframes: 5 }, emitters: [{ start: 0, end: 0, count: 1, origin: 'box', x: 100, y: 0, angle: 0, aim: true, speed: 10, size: 4 }] });
  const sim = createSim(p);
  for (let i = 0; i < 12; i++) sim.step(null);
  assert.equal(sim.hits, 1);
  assert.equal(sim.hp, 90);
  const g = normalize({ emitters: [{ start: 0, end: 0, count: 1, x: -100, y: 15, angle: 0, speed: 5, size: 2 }] });
  const s2 = createSim(g);
  for (let i = 0; i < 50; i++) s2.step(null);
  assert.equal(s2.hits, 0);
  assert.equal(s2.grazes, 1);
  const s3 = createSim(normalize({ emitters: [] }));
  for (let i = 0; i < 100; i++) s3.step({ right: true });
  assert.equal(s3.soul.x, 320 + 75 - 8);
});

test('box keyframes', () => {
  const p = normalize({ box_keys: [{ frame: 10, x: 320, y: 170, w: 250, h: 50 }] });
  assert.deepEqual(boxAt(p, 0), { x: 320, y: 170, w: 150, h: 150 });
  assert.deepEqual(boxAt(p, 5), { x: 320, y: 170, w: 200, h: 100 });
  assert.deepEqual(boxAt(p, 99), { x: 320, y: 170, w: 250, h: 50 });
});

test('same seed, same run', () => {
  const p = normalize({ emitters: [{ jitter_x: 30, jitter_angle: 20 }] });
  const a = simulateTo(p, 120, { seed: 7 }), b = simulateTo(p, 120, { seed: 7 });
  assert.deepEqual(a.bullets, b.bullets);
});

test('room preview: solids, triggers, boxes', () => {
  const objIdx = { obj_solidblock: {}, obj_wall: { ParentObject: 'obj_solidblock' }, obj_doorA: { Visible: false }, obj_tree: {}, obj_loopA: { ParentObject: 'obj_loopB' }, obj_loopB: { ParentObject: 'obj_loopA' } };
  assert.ok(isSolidObject('obj_wall', objIdx));
  assert.ok(!isSolidObject('obj_tree', objIdx));
  assert.ok(!isSolidObject('obj_loopA', objIdx));
  assert.ok(isTriggerInstance({ ObjectName: 'obj_doorA' }, objIdx));
  assert.ok(isTriggerInstance({ ObjectName: 'obj_tree', CreationCodeEntry: 'x' }, objIdx));
  assert.ok(!isTriggerInstance({ ObjectName: 'obj_wall', CreationCodeEntry: 'x' }, objIdx));
  assert.deepEqual(spriteBBox({ Width: 20, Height: 20, OriginX: 10, OriginY: 20 }, 100, 100, 2, 2), [80, 60, 120, 100]);
  assert.deepEqual(spriteBBox({ Width: 20, Height: 20, MarginLeft: 2, MarginRight: 17, MarginTop: 4, MarginBottom: 19 }, 0, 0), [2, 4, 18, 20]);
});

test('room preview: movement stops at walls and slides', () => {
  const walls = [[50, 0, 60, 100]];
  const b = [0, 0, 10, 10];
  assert.deepEqual(moveBox(b, 100, 0, walls), [40, 0]);
  assert.deepEqual(b, [40, 0, 50, 10]);
  assert.deepEqual(moveBox(b, 4, 4, walls), [0, 4]);
  assert.deepEqual(moveBox(b, -3, 0, walls), [-3, 0]);
});

test('room preview: doors', () => {
  assert.equal(doorLetter('obj_doorA'), 'A');
  assert.equal(doorLetter('obj_doorB_musfade'), 'B');
  assert.equal(doorLetter('obj_door'), null);
  assert.equal(doorLetter('obj_doorway'), null);
  const rooms = new Set(['room_a', 'room_b', 'room_c']);
  const code = 'if (room == room_a)\n    room_goto(room_b);\nif (room == room_b)\n    room_goto(room_c);\n';
  assert.deepEqual(roomTargets(code, 'room_a', rooms), ['room_b']);
  assert.deepEqual(roomTargets(code, 'room_b', rooms), ['room_c']);
  assert.deepEqual(roomTargets('nextroom = room_c;', 'room_a', rooms), ['room_c']);
  assert.deepEqual(roomTargets('room_goto(room_b); x = room_c;', 'room_a', rooms), ['room_b']);
  assert.deepEqual(roomTargets('', 'room_a', rooms), []);
});

test('room preview: player sprites', () => {
  const spr = { spr_krisd: {}, spr_krisd_dark: {}, spr_krisu: {} };
  assert.deepEqual(playerSprites(spr, true), { down: 'spr_krisd_dark', up: 'spr_krisu', left: null, right: null });
  assert.equal(playerSprites(spr, false).down, 'spr_krisd');
  assert.ok(guessDark('room_field1'));
  assert.ok(!guessDark('room_town_krisyard'));
});
