import { defineConfig, devices } from '@playwright/test'

export default defineConfig({
  testDir: './tests',
  fullyParallel: false,
  workers: 1,
  timeout: 600_000,
  use: {
    ...devices['Desktop Chrome'],
    actionTimeout: 20_000,
    baseURL: process.env.AGENTX_E2E_BASE_URL ?? 'http://127.0.0.1:18081',
    locale: 'zh-CN',
    viewport: { width: 1440, height: 900 },
    screenshot: 'only-on-failure',
    trace: 'on',
    video: 'retain-on-failure',
  },
  expect: { timeout: 15_000 },
  outputDir: 'test-results/artifacts',
  reporter: [
    ['list'],
    ['html', { outputFolder: 'playwright-report', open: 'never' }],
    ['junit', { outputFile: 'test-results/junit.xml' }],
  ],
})
