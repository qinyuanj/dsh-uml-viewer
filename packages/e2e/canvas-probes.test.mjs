import assert from 'node:assert/strict'
import test from 'node:test'
import { panDeltaBounds, letterInkCount, edgeHoverHit, dotPanX } from './canvas-probes.mjs'

const within = (value, [min, max]) => value >= min && value <= max
const rgba = pixels => pixels.flatMap(p => [...p, 255])

test('sample every dot, including the CI6 observer beyond the canvas right edge', () => {
  // All 16 class dots remain asserted; an out-of-canvas read is transparent.
  const delta = dotPanX(647, 685.8002, 4.6)
  assert.ok(685.8002 + delta + 4.6 <= 647 - 12)
  assert.equal(dotPanX(647, 373.0002, 4.6), 0, 'already visible aggregation stays in place')
  assert.ok(2 + dotPanX(647, 2, 4.6) - 4.6 >= 12, 'left clipping is also exposed')
})

test('100px drag matches the three-decimal camera summary from CI6', () => {
  // CI6 navigation: zoomed x=25.514, panned x=157.030, zoom=0.760.
  assert.ok(within(131.516, panDeltaBounds(100, 0.760)))
  assert.ok(!within(99 / 0.76036, panDeltaBounds(100, 0.760)), 'a missing screen pixel still fails')
  assert.ok(!within(101 / 0.76036, panDeltaBounds(100, 0.760)), 'an extra screen pixel still fails')
})

test('C/M probe recognizes CI6 antialiased ink while rejecting the disc and ring', () => {
  // CI6 :aggregation M, Sarasa at 8px, zoom .920, centre y=308.59136.
  // These are actual interior pixels in the failing screenshot, not a glyph
  // painted by the implementation under test. Host primary ink is #f9fafb.
  const pixels = rgba([[211, 207, 204], [214, 209, 205], [207, 202, 198], [136, 119, 107]])
  const ink = [249, 250, 251], fill = [71, 44, 24]
  assert.ok(letterInkCount(pixels, ink, fill) > 0)
  assert.equal(letterInkCount(rgba([fill, [222, 136, 77]]), ink, fill), 0, 'plain quality fill/ring cannot pass as a letter')
  assert.equal(letterInkCount(rgba([[211, 100, 204]]), ink, fill), 0, 'a different hue cannot pass as antialiased ink')
  assert.equal(letterInkCount(rgba([[136, 119, 107]]), ink, fill), 0, 'less than half-covered ink cannot satisfy the probe')
})

test('producer edge hover includes the dependencies carried by a real hit', () => {
  // --hover consumes a complete hit, not a request to resolve its endpoints.
  assert.equal(edgeHoverHit(':hub', ':association', [{ from: ':hub', to: ':association', violating: false }]),
    '{:kind :edge :from :hub :to :association :deps [{:from :hub :to :association :violating false}]}')
})
