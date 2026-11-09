import { test } from "node:test";
import assert from "node:assert/strict";
import { Counter, clamp, half, shift, sign } from "../src/calc.ts";

test("counter", () => {
  const counter = new Counter();
  assert.equal(counter.isZero(), true);
  assert.equal(counter.increment(2), 2);
  assert.equal(counter.isPositive(), true);
});

test("clamp", () => {
  assert.equal(clamp(5, 0, 3), 3);
  assert.equal(clamp(-1, 0, 3), 0);
});

test("sign", () => {
  assert.equal(sign(4), 1);
});

test("half", () => {
  assert.equal(half(8), 4);
});

test("shift", () => {
  assert.equal(shift(8, 1), 4);
});
