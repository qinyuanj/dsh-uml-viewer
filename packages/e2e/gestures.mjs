/** Whether a canvas-local point is clickable: inside the canvas, off its edges. */
export function exposedOnCanvas([x, y], canvas, margin = 8) {
  return x >= margin && y >= margin && x < canvas.width - margin && y < canvas.height - margin
}

/**
 * Measure a target and check it is on the canvas before a click. Nothing
 * floats over the canvas since the inspector was deleted (#47), so a fitted
 * view's target is exposed as measured; one that is not is a real layout
 * failure, reported instead of panned around. `measure` returns
 * { frame, point } with a canvas-local point.
 */
export async function exposeTarget(panel, measure) {
  const canvas = await panel.canvas.boundingBox()
  if (!canvas) throw new Error('the canvas must be visible before measuring a target')
  const measured = await measure()
  if (!exposedOnCanvas(measured.point, canvas)) {
    throw new Error(`target is not on the canvas: ${measured.point.join(',')}`)
  }
  return measured
}
