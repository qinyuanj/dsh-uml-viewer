// Preloaded into the isolated host only. The runner owns its stdin pipe; EOF
// stops the host even if the runner is interrupted or force-terminated.
if (process.env.UML_VIEWER_E2E_LIFELINE === '1') {
  delete process.env.UML_VIEWER_E2E_LIFELINE
  process.stdin.on('end', () => process.exit(0))
  process.stdin.resume()
}
