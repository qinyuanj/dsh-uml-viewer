import { expect, it } from 'vitest'
import { IrGate } from '../src/gate.js'

// #84: `uml_display` answers the request the latest `uml_ir` named, so the IR
// record carries that id. The display ends exactly that request instead of
// falling back to the queue head; a later IR that names none clears it again.
it('remembers the panel request the latest IR named, and clears it on the next IR', () => {
  const gate = new IrGate()
  expect(gate.answered('s')).toBeNull()
  gate.record('s', { validResult: true, exitCode: 0, reason: null, requestId: 'b' })
  expect(gate.answered('s')).toBe('b')
  expect(gate.check('s')).toMatchObject({ ok: true, record: { requestId: 'b' } })
  // A hand-run IR names no request: the display that follows answers the head.
  gate.record('s', { validResult: true, exitCode: 0, reason: null, requestId: null })
  expect(gate.answered('s')).toBeNull()
  // Sessions stay independent.
  gate.record('t', { validResult: false, exitCode: 1, reason: 'bad policy', requestId: 'c' })
  expect(gate.answered('s')).toBeNull()
  expect(gate.answered('t')).toBe('c')
})