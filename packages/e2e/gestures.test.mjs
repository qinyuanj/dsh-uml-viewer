import assert from 'node:assert/strict'
import test from 'node:test'
import { panBesideInspector } from './gestures.mjs'

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
