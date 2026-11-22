if (process.env.UML_VIEWER_E2E_LIFELINE === '1') {
  delete process.env.UML_VIEWER_E2E_LIFELINE
  process.stdin.on('end', () => process.exit(0))
  process.stdin.resume()
}
