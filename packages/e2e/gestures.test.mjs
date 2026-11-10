import assert from 'node:assert/strict'
import test from 'node:test'
import { exposeTarget, exposedOnCanvas } from './gestures.mjs'

// CI7 view-requests: a blanket pan once put Deep :api at canvas-local x -141.47.
test('exposeTarget accepts a target on the canvas and refuses one off it, never panning', async () => {
  const canvas = { x: 793, y: 83, width: 647, height: 917 }
  assert.equal(exposedOnCanvas([-141.4726, 451.4593], canvas), false)
  assert.equal(exposedOnCanvas([88, 451], canvas), true)
  // The right column the floating inspector used to cover is plain canvas (#47).
  assert.equal(exposedOnCanvas([500, 451], canvas), true)
  assert.equal(exposedOnCanvas([643, 451], canvas), false, 'within the edge margin')
  let drags = 0
  const panel = {
    canvas: { async boundingBox() { return canvas } },
    page: { mouse: { async move() {}, async down() {}, async up() { drags += 1 } } },
  }
  assert.deepEqual((await exposeTarget(panel, async () => ({ point: [500, 451] }))).point, [500, 451])
  await assert.rejects(exposeTarget(panel, async () => ({ point: [-141, 451] })), /not on the canvas/)
  assert.equal(drags, 0)
})
