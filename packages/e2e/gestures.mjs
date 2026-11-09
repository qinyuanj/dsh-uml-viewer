/** Whether a canvas-local point is clickable: on the canvas and left of the inspector. */
export function exposedOnCanvas([x, y], canvas, inspector, margin = 8) {
  const exposed = inspector.x - canvas.x
  return x >= margin && y >= margin && x < exposed - margin && y < canvas.height - margin
}

/**
 * Measure a target and pan only when the inspector covers it. A blanket pan
 * moves left-column targets off the canvas (CI7: :api at local x -141).
 * `measure` returns { frame, point } with a canvas-local point.
 */
export async function exposeTarget(panel, measure) {
  const boxes = async () => [await panel.canvas.boundingBox(),
    await panel.panel.getByRole('complementary', { name: '检查器' }).boundingBox()]
  let [canvas, inspector] = await boxes()
  let measured = await measure()
  if (exposedOnCanvas(measured.point, canvas, inspector)) return measured
  await panBesideInspector(panel)
  ;[canvas, inspector] = await boxes()
  measured = await measure()
  if (!exposedOnCanvas(measured.point, canvas, inspector)) {
    throw new Error(`target still not exposed after one pan: ${measured.point.join(',')}`)
  }
  return measured
}

/** Prepare a root view for geometry checks beside the floating inspector. */
export async function panBesideInspector(panel) {
  const canvas = await panel.canvas.boundingBox()
  const inspector = await panel.panel.getByRole('complementary', { name: '检查器' }).boundingBox()
  if (!canvas || !inspector) throw new Error('canvas and inspector must be visible before panning')
  const exposed = inspector.x - canvas.x
  const distance = inspector.width
  if (exposed <= distance) throw new Error('the panel is too narrow for an unobscured inspector pan')
  // Keep both ends in the exposed canvas. This is a normal user gesture;
  // measure a fresh producer frame afterwards, since the camera has moved.
  const start = canvas.x + (exposed + distance) / 2
  const y = canvas.y + canvas.height / 2
  await panel.page.mouse.move(start, y)
  await panel.page.mouse.down()
  await panel.page.mouse.move(start - distance, y, { steps: 6 })
  await panel.page.mouse.up()
}
