import { expect, type Page, test } from '@playwright/test'

const password = 'agentx-e2e-admin-password'

async function login(page: Page) {
  const bootstrapStatus = await page.request.get('/api/v1/bootstrap/status')
  await expect(bootstrapStatus).toBeOK()
  if (((await bootstrapStatus.json()) as { required: boolean }).required) {
    await page.goto('/setup')
    await page.getByLabel('公司名称').fill('Agentx E2E')
    await page.getByLabel('用户名').fill('admin')
    await page.getByLabel('管理员姓名').fill('E2E Admin')
    await page.getByLabel('密码').fill(password)
    await page.getByRole('button', { name: '初始化并进入工作台' }).click()
    await expect(page).toHaveURL(/\/$/)
    return
  }
  await page.goto('/login')
  await page.getByLabel('用户名').fill('admin')
  await page.getByLabel('密码').fill(password)
  await page.getByRole('button', { name: '登录' }).click()
  await expect(page).toHaveURL(/\/$/)
}

test('Session Diagnostics loads durable state, reports permission errors, and clears a session', async ({ page }) => {
  await login(page)

  await page.route('**/api/v1/agent-sessions?*', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        apiVersion: 1,
        items: [{
          sessionKey: 'invocation:execution-1:attempt-1',
          sessionId: 'invocation-session-1',
          stableAgentNodeKey: 'agent',
          applicationId: '00000000-0000-7000-8000-000000000001',
          stateVersion: 4,
          fencingToken: 2,
          openOperationId: null,
          terminalState: 'completed',
          bundleHash: 'sha256:bundle',
          modelVersion: 'model:1',
          updatedAt: '2026-08-25T00:00:00Z',
        }],
        next: null,
      }),
    })
  })
  await page.route('**/api/v1/agent-sessions/invocation%3Aexecution-1%3Aattempt-1/agent', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        apiVersion: 1,
        summary: {
          sessionKey: 'invocation:execution-1:attempt-1',
          sessionId: 'invocation-session-1',
          stableAgentNodeKey: 'agent',
          applicationId: '00000000-0000-7000-8000-000000000001',
          stateVersion: 4,
          fencingToken: 2,
          openOperationId: null,
          terminalState: 'completed',
          bundleHash: 'sha256:bundle',
          modelVersion: 'model:1',
          updatedAt: '2026-08-25T00:00:00Z',
        },
        entries: [{ entryId: 'entry-1', entryKind: 'message.user', operationId: 'op-1', createdAt: '2026-08-25T00:00:00Z' }],
        usages: [{ usageKind: 'model', inputTokens: 5, outputTokens: 4, costMicros: 9, costCurrency: 'USD', createdAt: '2026-08-25T00:00:00Z' }],
        compaction: { kind: 'threshold', snapshotId: 'snapshot-1' },
        recovery: { phase: 'checkpoint', recoveryAction: 'resume' },
      }),
    })
  })
  await page.route('**/api/v1/agent-subject-memory/audit', async (route) => {
    await route.fulfill({
      status: 403,
      contentType: 'application/json',
      body: JSON.stringify({ code: 'FORBIDDEN', message: '无权访问主体记忆' }),
    })
  })
  await page.route('**/api/v1/agent-sessions/clear', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ apiVersion: 1, clearedEntries: 2, clearedPending: 0, auditId: 'audit-1' }),
    })
  })

  await page.goto('/agent-sessions')
  await expect(page.getByText('invocation:execution-1:attempt-1', { exact: true })).toBeVisible()
  await page.getByRole('button', { name: '查看会话' }).click()
  await expect(page.getByText('message.user', { exact: true })).toBeVisible()
  await expect(page.getByText(/threshold/).first()).toBeVisible()

  await page.getByPlaceholder('UUID').fill('memory-version-1')
  await page.getByRole('button', { name: '加载审计' }).click()
  await expect(page.getByText('无权访问主体记忆')).toBeVisible()

  await page.getByRole('button', { name: '清除会话' }).first().click()
  const dialog = page.getByRole('dialog')
  await expect(dialog).toBeVisible()
  await dialog.getByRole('button', { name: '清除会话' }).click()
  await expect(page.getByText('会话诊断')).toHaveCount(0)
})
