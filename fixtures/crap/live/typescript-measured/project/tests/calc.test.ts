import assert from "node:assert/strict";
import test from "node:test";

import { choose } from "../src/calc.ts";

test("positive", () => {
  assert.equal(choose(3), 6);
});
