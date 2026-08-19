import { defineConfig, devices } from '@playwright/test'

const stage = process.env.AGENTX_E2E_STAGE ?? 'local'
const runId = process.env.AGENTX_E2E_RUN_ID ?? new Date().toISOString().replaceAll(':', '-').replaceAll('.', '-')
const suite = process.env.AGENTX_E2E_SUITE
const resultRoot = `test-results/${stage}/${runId}${suite ? `/${suite}` : ''}`
const hostResolverRules = process.env.AGENTX_E2E_HOST_RESOLVER_RULES

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
    launchOptions: hostResolverRules ? { args: [`--host-resolver-rules=${hostResolverRules}`] } : undefined,
    screenshot: 'only-on-failure',
    trace: 'on',
    video: 'retain-on-failure',
  },
  expect: { timeout: 15_000 },
  outputDir: `${resultRoot}/artifacts`,
  reporter: [
    ['list'],
    ['html', { outputFolder: `${resultRoot}/playwright-report`, open: 'never' }],
    ['junit', { outputFile: `${resultRoot}/junit.xml` }],
  ],
})
