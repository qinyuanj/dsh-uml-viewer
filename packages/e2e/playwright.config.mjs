import { defineConfig } from '@playwright/test'
import path from 'node:path'

const root = path.resolve(import.meta.dirname, '../..')
const scratch = process.env.UML_VIEWER_E2E_SCRATCH ?? path.join(root, '.scratch', 'e2e')
process.env.PLAYWRIGHT_BROWSERS_PATH ??= path.join(scratch, 'browsers')

export default defineConfig({
  testDir: import.meta.dirname,
  testMatch: '**/*.spec.mjs',
  timeout: 180_000,
  expect: { timeout: 30_000 },
  workers: 1,
  reporter: 'list',
  outputDir: path.join(scratch, 'results'),
  use: {
    headless: true,
    viewport: { width: 1440, height: 1000 },
    launchOptions: {
      args: ['--disable-background-timer-throttling', '--disable-backgrounding-occluded-windows', '--disable-renderer-backgrounding'],
    },
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
})
