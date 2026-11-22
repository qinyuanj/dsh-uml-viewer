export function panDeltaBounds(screenDelta, roundedZoom) {
  const halfUnit = 0.0005
  const deltas = [screenDelta / (roundedZoom - halfUnit), screenDelta / (roundedZoom + halfUnit)]
  return [Math.min(...deltas) - 2 * halfUnit, Math.max(...deltas) + 2 * halfUnit]
}

export function letterInkCount(pixels, ink, fill) {
  const direction = ink.slice(0, 3).map((v, k) => v - fill[k])
  const length2 = direction.reduce((sum, v) => sum + v * v, 0)
  if (length2 === 0) throw new Error('marker ink must contrast with the quality disc')
  let count = 0
  for (let i = 0; i < pixels.length; i += 4) {
    const coverage = direction.reduce((sum, v, k) => sum + (pixels[i + k] - fill[k]) * v, 0) / length2
    if (coverage >= 0.5 && coverage <= 1.01
      && direction.every((v, k) => Math.abs(pixels[i + k] - (fill[k] + coverage * v)) <= 3)) count++
  }
  return count
}

export function dotPanX(width, centerX, radius) {
  if (centerX - radius < 12) return Math.ceil(12 - centerX + radius)
  if (centerX + radius > width - 12) return -Math.ceil(centerX + radius - width + 12)
  return 0
}

export function edgeHoverHit(from, to, deps) {
  const dependencies = deps.map(dep => `{:from ${dep.from} :to ${dep.to} :violating ${dep.violating}}`).join(' ')
  return `{:kind :edge :from ${from} :to ${to} :deps [${dependencies}]}`
}
