export function exposedOnCanvas([x, y], canvas, margin = 8) {
  return x >= margin && y >= margin && x < canvas.width - margin && y < canvas.height - margin
}

export async function exposeTarget(panel, measure) {
  const canvas = await panel.canvas.boundingBox()
  if (!canvas) throw new Error('the canvas must be visible before measuring a target')
  const measured = await measure()
  if (!exposedOnCanvas(measured.point, canvas)) {
    throw new Error(`target is not on the canvas: ${measured.point.join(',')}`)
  }
  return measured
}
