// Code highlighter: every line of the HTML has whole spans (the line highlight splits it at '\n').
import test from 'node:test';
import assert from 'node:assert/strict';
import { highlight } from '../js/editors/codeedit.js';

test('multi-line comments and strings close their spans on each line', () => {
  const html = highlight('a = 1; /* x\n<y> */ b(@"s\nt");');
  for (const line of html.split('\n')) {
    assert.equal((line.match(/<span/g) || []).length, (line.match(/<\/span>/g) || []).length, line);
  }
  assert.ok(html.includes('&lt;y&gt;'));
});
