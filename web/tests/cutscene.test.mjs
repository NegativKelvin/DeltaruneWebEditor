// node --test web/tests
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { splitArgs, parseCall, stepGml, timeline, actorPositions, textAt, graphLayout, argSpec } from '../js/editors/cutscenecore.js';
import { visibleText, checkText, DEFAULT_LIMITS } from '../js/editors/textcore.js';

test('split and parse calls', () => {
  assert.deepEqual(splitArgs('"r", f(1, 2), "a, b", \'x\''), ['"r"', 'f(1, 2)', '"a, b"', "'x'"]);
  assert.deepEqual(splitArgs(''), []);
  assert.deepEqual(splitArgs('"q\\", x", 1'), ['"q\\", x"', '1']);
  assert.deepEqual(parseCall('  c_walk("r", 4, 10);'), { func: 'c_walk', args: ['"r"', '4', '10'] });
  assert.deepEqual(parseCall('c_talk()'), { func: 'c_talk', args: [] });
  assert.equal(parseCall('x = 1;'), null);
  assert.equal(stepGml({ func: 'c_wait', args: [' 30 '] }), 'c_wait(30);');
  assert.deepEqual(argSpec('c_walk').map((a) => a.type), ['dir', 'num', 'frames']);
  assert.deepEqual(argSpec('c_sel'), [{ name: 'actor', type: 'actor' }]);
});

const STEPS = [
  { func: 'c_sel', args: ['kr'] },
  { func: 'c_walk', args: ['"r"', '2', '20'] },
  { func: 'c_sel', args: ['su'] },
  { func: 'c_walk_wait', args: ['"d"', '1', '10'] },
  { func: 'c_msgsetloc', args: ['0', '"* Hey./%"', '"k"'] },
  { func: 'c_talk_wait', args: [] },
  { func: 'c_wait', args: ['30'] },
];

test('timeline: only wait steps move time', () => {
  const tl = timeline(STEPS);
  assert.deepEqual(tl.items.map((x) => x.start), [0, 0, 0, 0, 10, 10, 55]);
  assert.equal(tl.items[1].dur, 20);
  assert.equal(tl.items[1].lane, 'actor kr');
  assert.equal(tl.items[3].lane, 'actor su');
  assert.equal(tl.items[5].known, false);
  assert.equal(tl.length, 85);
  assert.deepEqual(tl.lanes, ['text', 'wait', 'actor kr', 'actor su']);
});

test('stage positions and text', () => {
  assert.deepEqual(actorPositions(STEPS, 5), { kr: { x: 10, y: 0 }, su: { x: 0, y: 5 } });
  assert.deepEqual(actorPositions(STEPS, 100), { kr: { x: 40, y: 0 }, su: { x: 0, y: 10 } });
  assert.equal(textAt(STEPS, 0), null);
  assert.equal(textAt(STEPS, 12), '* Hey./%');
});

test('state graph layout', () => {
  const g = graphLayout([{ con: '1', next: ['2'] }, { con: '2', next: ['3', '1'] }, { con: '10', next: [] }]);
  assert.deepEqual(g.nodes.map((n) => n.con), ['1', '2', '10', '3']);
  assert.equal(g.nodes[3].missing, true);
  assert.deepEqual(g.edges.map((e) => e.back), [false, false, true]);
});

test('text measure matches the server', () => {
  const cases = JSON.parse(readFileSync(new URL('../../server/tests/fixtures/text_cases.json', import.meta.url)));
  for (const c of cases) {
    assert.deepEqual(visibleText(c.text), c.visible, c.text);
    assert.equal(checkText(c.text, !!c.face, DEFAULT_LIMITS).length, c.problems, c.text);
  }
});
