import { createHmac } from 'node:crypto'

import { expect, type Page, test } from '@playwright/test'

const password = 'agentx-e2e-admin-password'
const resumeSecret = process.env.AGENTX_E2E_WAIT_SIGNING_SECRET ?? 'agentx-local-jwt-signing-secret-change-me'

async function login(page: Page) {
  await page.goto('/login')
  await page.getByLabel('用户名').fill('admin')
  await page.getByLabel('密码').fill(password)
  await page.getByRole('button', { name: '登录' }).click()
  await expect(page).toHaveURL(/\/$/)
}

async function runFixture(page: Page, name: string) {
  await page.getByRole('link', { name: '工作流', exact: true }).click()
  await page.getByRole('searchbox', { name: '搜索工作流' }).fill(name)
  const row = page.getByRole('row').filter({ hasText: name })
  await expect(row).toBeVisible()
  await row.getByRole('link', { name: '详情' }).click()
  await page.getByRole('button', { name: '运行版本' }).click()
  await expect(page).toHaveURL(/\/executions\/[0-9a-f-]+$/)
  return { executionId: page.url().split('/').at(-1) as string, url: page.url() }
}

async function waitResumePath(page: Page) {
  const locator = page.getByText(/\/gateway\/v1\/waits\/[0-9a-f-]+\/resume/)
  await expect(locator).toBeVisible({ timeout: 60_000 })
  return (await locator.textContent()) as string
}

function signedResume(path: string, body: string, idempotencyKey: string) {
  const bindingId = path.split('/').at(-2) as string
  return {
    'Content-Type': 'application/json',
    'Idempotency-Key': idempotencyKey,
    'X-Agentx-Wait-Signature': createHmac('sha256', resumeSecret).update(`${bindingId}.${body}`).digest('base64url'),
  }
}

test('M4 recovery resumes waits and approvals once, handles timers, and rejects late resume after cancel', async ({ page }) => {
  await login(page)

  const wait = await runFixture(page, 'M4 Wait Fixture')
  await expect(page.locator('header').getByText('waiting', { exact: true })).toBeVisible({ timeout: 60_000 })
  const resumePath = await waitResumePath(page)
  const body = JSON.stringify({ outputPort: 'resumed', payload: { accepted: true } })
  const unauthorized = await page.request.post(resumePath, { data: body, headers: { 'Content-Type': 'application/json', 'Idempotency-Key': 'm4-unsigned-resume' } })
  expect(unauthorized.status()).toBe(401)
  const headers = signedResume(resumePath, body, 'm4-signed-resume')
  const resumed = await page.request.post(resumePath, { data: body, headers })
  expect(resumed.ok()).toBeTruthy()
  expect(await resumed.json()).toMatchObject({ accepted: true, replayed: false })
  const replay = await page.request.post(resumePath, { data: body, headers })
  expect(await replay.json()).toMatchObject({ accepted: true, replayed: true })
  await page.goto(wait.url)
  await expect(page.locator('header').getByText('succeeded', { exact: true })).toBeVisible({ timeout: 60_000 })

  await runFixture(page, 'M4 Timer Fixture')
  await expect(page.getByRole('button', { name: /Duration Wait/ })).toBeVisible({ timeout: 60_000 })
  await expect(page.locator('header').getByText('succeeded', { exact: true })).toBeVisible({ timeout: 60_000 })

  await runFixture(page, 'M4 Datetime Wait Fixture')
  await expect(page.getByRole('button', { name: /Datetime Wait/ })).toBeVisible({ timeout: 60_000 })
  await expect(page.getByText('datetime', { exact: true }).first()).toBeVisible()
  await expect(page.locator('header').getByText('succeeded', { exact: true })).toBeVisible({ timeout: 60_000 })

  const form = await runFixture(page, 'M4 Form Wait Fixture')
  await expect(page.locator('header').getByText('waiting', { exact: true })).toBeVisible({ timeout: 60_000 })
  const formResumePath = await waitResumePath(page)
  const invalidFormBody = JSON.stringify({ outputPort: 'resumed', payload: {} })
  const invalidForm = await page.request.post(formResumePath, { data: invalidFormBody, headers: signedResume(formResumePath, invalidFormBody, `m4-form-invalid-${form.executionId}`) })
  expect(invalidForm.status()).toBe(400)
  const formBody = JSON.stringify({ outputPort: 'resumed', payload: { name: 'M4 form' } })
  const formResume = await page.request.post(formResumePath, { data: formBody, headers: signedResume(formResumePath, formBody, `m4-form-${form.executionId}`) })
  expect(formResume.ok()).toBeTruthy()
  await page.goto(form.url)
  await expect(page.locator('header').getByText('succeeded', { exact: true })).toBeVisible({ timeout: 60_000 })

  const approval = await runFixture(page, 'M4 Approval Fixture')
  await expect(page.getByText('M4 Charge Approval')).toBeVisible({ timeout: 60_000 })
  await page.getByRole('link', { name: '打开审批' }).click()
  await page.getByRole('button', { name: '领取' }).click()
  await page.getByRole('button', { name: '通过' }).click()
  const approvalDialog = page.getByRole('dialog', { name: '通过' })
  await approvalDialog.getByRole('button', { name: '通过' }).click()
  await page.goto(approval.url)
  await expect(page.locator('header').getByText('succeeded', { exact: true })).toBeVisible({ timeout: 60_000 })

  const rejectedApproval = await runFixture(page, 'M4 Approval Fixture')
  await expect(page.getByText('M4 Charge Approval')).toBeVisible({ timeout: 60_000 })
  await page.getByRole('link', { name: '打开审批' }).click()
  await page.getByRole('button', { name: '领取' }).click()
  await page.getByRole('button', { name: '拒绝' }).click()
  await page.getByRole('dialog', { name: '拒绝' }).getByRole('button', { name: '拒绝' }).click()
  await page.goto(rejectedApproval.url)
  await expect(page.locator('header').getByText('succeeded', { exact: true })).toBeVisible({ timeout: 60_000 })
  await expect(page.getByRole('button', { name: /Rejected Output Run \d+ · succeeded/ })).toBeVisible()

  await runFixture(page, 'M4 Approval Timeout Fixture')
  await expect(page.getByText('M4 Expiring Approval')).toBeVisible({ timeout: 60_000 })
  await expect(page.locator('header').getByText('succeeded', { exact: true })).toBeVisible({ timeout: 60_000 })
  await expect(page.getByRole('button', { name: /Approval Timeout Output Run \d+ · succeeded/ })).toBeVisible()

  const cancelled = await runFixture(page, 'M4 Wait Fixture')
  await expect(page.locator('header').getByText('waiting', { exact: true })).toBeVisible({ timeout: 60_000 })
  const cancelledResumePath = await waitResumePath(page)
  await page.getByRole('button', { name: 'Cancel' }).click()
  await page.getByRole('dialog', { name: 'Cancel execution?' }).getByRole('button', { name: 'Cancel execution' }).click()
  await expect(page.locator('header').getByText('cancelled', { exact: true })).toBeVisible({ timeout: 60_000 })
  const lateBody = JSON.stringify({ outputPort: 'resumed', payload: { late: true } })
  const late = await page.request.post(cancelledResumePath, { data: lateBody, headers: signedResume(cancelledResumePath, lateBody, `m4-late-${cancelled.executionId}`) })
  expect(late.ok()).toBeFalsy()
})
