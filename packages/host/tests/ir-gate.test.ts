import { expect, it } from 'vitest'
import { IrGate } from '../src/gate.js'

it('remembers the panel request the latest IR named, and clears it on the next IR', () => {
  const gate = new IrGate()
  expect(gate.answered('s')).toBeNull()
  gate.record('s', { validResult: true, exitCode: 0, reason: null, requestId: 'b' })
  expect(gate.answered('s')).toBe('b')
  expect(gate.check('s')).toMatchObject({ ok: true, record: { requestId: 'b' } })
  gate.record('s', { validResult: true, exitCode: 0, reason: null, requestId: null })
  expect(gate.answered('s')).toBeNull()
  gate.record('t', { validResult: false, exitCode: 1, reason: 'bad policy', requestId: 'c' })
  expect(gate.answered('s')).toBeNull()
  expect(gate.answered('t')).toBe('c')
})