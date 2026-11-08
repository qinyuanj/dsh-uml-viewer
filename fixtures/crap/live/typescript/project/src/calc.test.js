import assert from 'node:assert/strict';
import test from 'node:test';
import { choose } from './calc.js';

test('positive', () => {
  assert.equal(choose(3), 6);
});
