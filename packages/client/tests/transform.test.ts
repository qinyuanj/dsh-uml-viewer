import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import type { SceneFrame } from '@dsh-uml-viewer/contracts'
import { viewTransform, worldToScreen } from '../src/fit.js'

const framesDir = path.resolve(fileURLToPath(new URL('../../../crates/frame/tests/fixtures/frames', import.meta.url)))

function golden(name: string): SceneFrame {
  return JSON.parse(readFileSync(path.join(framesDir, name), 'utf8')) as SceneFrame
}

function box(frame: SceneFrame, kind: 'package' | 'class', id: string) {
  const item = frame.world.find(candidate => candidate.kind === kind && candidate.id === id)
  if (!item || (item.kind !== 'package' && item.kind !== 'class')) {
    throw new Error(`missing ${kind} ${id}`)
  }
  return item.rect
}

describe('scene frame viewport transform', () => {
  it.each([
    {
      name: 'root.frame.json',
      boxes: [
        { kind: 'package', id: ':view', screen: [474.7874, 82.7402] },
        { kind: 'class', id: ':util', screen: [550.9565, 119.4646] },
        { kind: 'class', id: ':api', screen: [484.8031, 363.1811] },
      ],
    },
    {
      name: 'core-selected.frame.json',
      boxes: [
        { kind: 'package', id: ':view', screen: [458, 223] },
        { kind: 'class', id: ':core.order', screen: [470, 307] },
        { kind: 'class', id: ':core', screen: [630, 267] },
      ],
    },
  ] as const)('maps $name world boxes to the native screen coordinates', ({ name, boxes }) => {
    const frame = golden(name)
    const view = viewTransform(frame, frame.viewport.width, frame.viewport.height)
    for (const expected of boxes) {
      const rect = box(frame, expected.kind, expected.id)
      const actual = worldToScreen(view, rect.x, rect.y)
      expect(actual[0], `${expected.id} screen x`).toBeCloseTo(expected.screen[0], 4)
      expect(actual[1], `${expected.id} screen y`).toBeCloseTo(expected.screen[1], 4)
    }
  })
})
