import { readFile } from 'node:fs/promises'

import { expect, type Page, test } from '@playwright/test'

const password = 'agentx-e2e-admin-password'

type RuntimeContext = { applicationId: string; applicationSlug: string }

async function login(page: Page) {
  await page.goto('/login')
  await page.getByLabel('用户名').fill('admin')
  await page.getByLabel('密码').fill(password)
  await page.getByRole('button', { name: '登录' }).click()
  await expect(page).toHaveURL(/\/$/)
}

test('Playground parameter mode invokes the published Application and restores server history', async ({ page }) => {
  const contextPath = process.env.AGENTX_V2_08_CONTEXT_OUTPUT
  if (!contextPath) throw new Error('AGENTX_V2_08_CONTEXT_OUTPUT is required')
  const runtime = JSON.parse(await readFile(contextPath, 'utf8')) as RuntimeContext
  await login(page)

  await page.goto(`/playground?applicationId=${runtime.applicationId}&mode=parameters`)
  await expect(page.getByRole('tab', { name: '参数测试' })).toHaveAttribute('aria-selected', 'true')
  await page.getByLabel('Message').fill('parameter-history-closure')
  const invoked = page.waitForResponse((response) => response.url().includes(`/gateway/v1/applications/${runtime.applicationSlug}/invocations`) && response.request().method() === 'POST')
  await page.getByRole('button', { name: '运行测试' }).click()
  expect((await invoked).status()).toBe(202)
  await expect(page.getByText('已完成', { exact: true })).toBeVisible({ timeout: 60_000 })
  await expect(page.getByText('parameter-history-closure', { exact: false })).toBeVisible()
  await expect(page.getByRole('link', { name: '执行详情 / Trace' })).toBeVisible()

  await page.reload()
  await expect(page.getByRole('tab', { name: '参数测试' })).toHaveAttribute('aria-selected', 'true')
  const firstHistory = page.locator('aside').getByRole('button').first()
  await expect(firstHistory).toBeVisible({ timeout: 30_000 })
  await firstHistory.click()
  await expect(page.getByText('parameter-history-closure', { exact: false })).toBeVisible({ timeout: 30_000 })
  await expect(page.getByLabel('Message')).toHaveValue('parameter-history-closure')
  await expect(page.getByRole('link', { name: '执行详情 / Trace' })).toBeVisible()
})
