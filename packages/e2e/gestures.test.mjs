import assert from 'node:assert/strict'
import test from 'node:test'
import { panBesideInspector, exposeTarget, exposedOnCanvas } from './gestures.mjs'

// CI2 run-ftaZyy (DPR 2) and run-L6010X (DPR 1): same CSS geometry.
// The oracle's orders centre was covered by the inspector at x=1210.
for (const dpr of [1, 2]) {
  test(`expose the Shop orders class before measuring its DPR ${dpr} click`, async () => {
    const canvas = { x: 793, y: 83, width: 647, height: 917 }
    const inspector = { x: 1210, y: 83, width: 230, height: 917 }
    const orders = { x: 1286.5, y: 575.5 }
    let pointer
    let down
    let shift = 0
    let drags = 0
    const onCanvas = ([x, y]) => x >= canvas.x && x < inspector.x && y >= canvas.y && y < canvas.y + canvas.height
    const panel = {
      canvas: { async boundingBox() { return canvas } },
      panel: { getByRole() { return { async boundingBox() { return inspector } } } },
      page: { mouse: {
        async move(x, y) {
          pointer = [x, y]
          assert.ok(onCanvas(pointer), 'the real pan must stay on the exposed canvas')
        },
        async down() { down = pointer },
        async up() { shift += pointer[0] - down[0]; drags += 1 },
      } },
    }
    assert.equal(onCanvas([orders.x, orders.y]), false, 'the CI click was covered')
    await panBesideInspector(panel)
    assert.equal(drags, 1)
    assert.ok(onCanvas([orders.x + shift, orders.y]), 'the same world target is now exposed')
  })
}

// CI7 view-requests: the blanket pan put Deep :api at canvas-local x -141.47.
test('exposeTarget leaves an already exposed target in place and pans only covered ones', async () => {
  const canvas = { x: 793, y: 83, width: 647, height: 917 }
  const inspector = { x: 1210, y: 83, width: 230, height: 917 }
  assert.equal(exposedOnCanvas([-141.4726, 451.4593], canvas, inspector), false)
  assert.equal(exposedOnCanvas([88, 451], canvas, inspector), true)
  assert.equal(exposedOnCanvas([500, 451], canvas, inspector), false, 'under the inspector')
  let drags = 0
  const panel = {
    canvas: { async boundingBox() { return canvas } },
    panel: { getByRole() { return { async boundingBox() { return inspector } } } },
    page: { mouse: { async move() {}, async down() {}, async up() { drags += 1 } } },
  }
  await exposeTarget(panel, async () => ({ point: [88, 451] }))
  assert.equal(drags, 0, 'a visible left-column target is never panned away')
  let x = 500
  await exposeTarget(panel, async () => { const point = [x, 451]; x -= 230; return { point } })
  assert.equal(drags, 1, 'a covered target is panned once')
  await assert.rejects(exposeTarget(panel, async () => ({ point: [500, 451] })), /still not exposed/)
})
