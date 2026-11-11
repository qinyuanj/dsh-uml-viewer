#!/usr/bin/env node
// Diagnostic-only executable for packaged resident attachment acceptance.
// It reuses the real host protocol fixture without building or running Rust.
import { existsSync, writeFileSync } from 'node:fs'
if (process.argv[2] === '--version') {
  process.stdout.write('uml-viewer 1.0.0\n')
} else {
  const fixture = process.env.UML_VIEWER_PACKAGED_FIXTURE
  const frame = process.env.UML_VIEWER_PACKAGED_FRAME
  if (!fixture || !frame) throw new Error('Set the diagnostic fixture and frame paths')
  if (process.argv[2] === 'resident' && !existsSync('resident.json')) {
    writeFileSync('resident.json', JSON.stringify({ framePath: frame }))
  }
  if (process.argv[2] !== 'resident' && !existsSync('fake-binary.json')) {
    writeFileSync('fake-binary.json', JSON.stringify({ exitCode: 0 }))
  }
  await import(fixture)
}
