import { expect, type Page, test } from '@playwright/test'

const password = 'agentx-e2e-admin-password'

async function login(page: Page) {
  await page.goto('/login')
  await page.getByLabel('用户名').fill('admin')
  await page.getByLabel('密码').fill(password)
  const loginResponse = page.waitForResponse((response) => response.url().endsWith('/api/v1/auth/login') && response.request().method() === 'POST')
  await page.getByRole('button', { name: '登录' }).click()
  await expect(page).toHaveURL(/\/$/)
  return ((await (await loginResponse).json()) as { accessToken: string }).accessToken
}

async function runFixture(page: Page, name: string) {
  await page.getByRole('link', { name: '工作流', exact: true }).click()
  await page.getByRole('searchbox', { name: '搜索工作流' }).fill(name)
  const row = page.getByRole('row').filter({ hasText: name })
  await expect(row).toBeVisible()
  await row.getByRole('link', { name: '详情' }).click()
  await page.getByRole('button', { name: '运行版本' }).click()
  await expect(page).toHaveURL(/\/executions\/[0-9a-f-]+$/)
  return page.url().split('/').at(-1) as string
}

function succeededRun(page: Page, name: string) {
  return page.getByRole('button', { name: new RegExp(`^${name} 运行 \\d+ · 成功$`) }).first()
}

test('M4 runtime executes deterministic graph semantics and forks through the workbench', async ({ page }) => {
  const accessToken = await login(page)
  const platformHeaders = { Authorization: `Bearer ${accessToken}` }
  const sourceExecutionId = await runFixture(page, 'M4 Runtime Fixture')

  await expect(page.locator('header').getByText('成功', { exact: true })).toBeVisible({ timeout: 120_000 })
  await expect(page.getByRole('complementary', { name: '执行节点大纲' }).getByRole('button')).toHaveText([
    /Left Branch.*运行 0.*成功/,
    /Right Branch.*运行 0.*成功/,
    /Required Merge.*运行 0.*成功/,
    /Loop Over Items.*运行 0.*成功/,
    /Loop Over Items.*运行 1.*成功/,
    /Fixed Sub-workflow.*运行 0.*已跳过/,
    /Remote Echo.*运行 0.*已跳过/,
    /Fixed Sub-workflow.*运行 1.*成功/,
    /Remote Echo.*运行 1.*成功/,
  ])
  for (const name of ['Left Branch', 'Right Branch', 'Required Merge', 'Loop Over Items', 'Fixed Sub-workflow', 'Remote Echo']) {
    await expect(succeededRun(page, name)).toBeVisible()
  }

  await succeededRun(page, 'Required Merge').click()
  await expect(page.getByRole('tab', { name: /Lineage [2-9]/ })).toBeVisible()
  await page.getByRole('tab', { name: /Lineage/ }).click()
  await expect(page.getByText(/输出 0 · 项 0/).first()).toBeVisible()
  await page.getByRole('tab', { name: /尝试记录/ }).click()
  await expect(page.getByText('尝试 1')).toBeVisible()

  await succeededRun(page, 'Remote Echo').click()
  await page.getByRole('tab', { name: /日志/ }).click()
  await expect(page.getByText('node.completed').first()).toBeVisible()
  await expect(page.getByText(/检查点 · [1-9]/)).toBeVisible()

  const checkpoints = await (await page.request.get(`/api/v1/executions/${sourceExecutionId}/checkpoints`, { headers: platformHeaders })).json() as { items: Array<{ id: string }> }
  const guardedForkResponse = await page.request.post(`/api/v1/executions/${sourceExecutionId}/fork`, {
    headers: platformHeaders,
    data: {
      checkpointId: checkpoints.items.at(-1)?.id,
      mode: 'whole',
      nodeId: null,
      inputOverrides: {},
      sideEffectDecisions: {},
      idempotencyKey: `m4-confirm-${sourceExecutionId}`,
    },
  })
  expect(guardedForkResponse.status()).toBe(202)
  const guardedFork = await guardedForkResponse.json() as { executionId: string }
  await page.goto(`/executions/${guardedFork.executionId}`)
  await expect(page.locator('header').getByText('等待中', { exact: true })).toBeVisible({ timeout: 120_000 })
  const guardedNodes = await (await page.request.get(`/api/v1/executions/${guardedFork.executionId}/nodes`, { headers: platformHeaders })).json() as { items: Array<{ id: string, status: string, sideEffectLevel: string }> }
  const guardedNode = guardedNodes.items.find((node) => node.sideEffectLevel === 'irreversible' && node.status === 'waiting')
  expect(guardedNode).toBeTruthy()
  const confirmation = {
    nodeExecutionId: guardedNode?.id,
    checkpointId: null,
    decision: 'dry_run',
    idempotencyKey: `m4-side-effect-${guardedFork.executionId}`,
  }
  const confirmed = await page.request.post(`/api/v1/executions/${guardedFork.executionId}/side-effect-confirmations`, { headers: platformHeaders, data: confirmation })
  expect(await confirmed.json()).toMatchObject({ accepted: true, replayed: false })
  const confirmationReplay = await page.request.post(`/api/v1/executions/${guardedFork.executionId}/side-effect-confirmations`, { headers: platformHeaders, data: confirmation })
  expect(await confirmationReplay.json()).toMatchObject({ accepted: true, replayed: true })
  await page.goto(`/executions/${guardedFork.executionId}`)
  await expect(page.locator('header').getByText('成功', { exact: true })).toBeVisible({ timeout: 120_000 })

  await page.goto(`/executions/${sourceExecutionId}`)

  await page.getByRole('button', { name: '派生执行' }).click()
  const fork = page.getByRole('dialog', { name: '派生执行' })
  await expect(fork.getByText('执行预览')).toBeVisible()
  await expect(fork.getByText(/不可逆节点需要明确决策/)).toBeVisible()
  await fork.getByRole('button', { name: '创建派生执行' }).click()

  await expect(page).toHaveURL(new RegExp(`/executions/(?!${sourceExecutionId})[0-9a-f-]+$`))
  await expect(page.getByText(`父执行 ${sourceExecutionId}`)).toBeVisible()
  await expect(page.locator('header').getByText('成功', { exact: true })).toBeVisible({ timeout: 120_000 })

  await runFixture(page, 'M4 Broker Fixture')
  await expect(page.locator('header').getByText('成功', { exact: true })).toBeVisible({ timeout: 120_000 })
  await succeededRun(page, 'Remote Broker Probe').click()
  await page.getByRole('tab', { name: '输出' }).click()
  await expect(page.getByTestId('execution-json')).toContainText('"credentialResolved": true')
  await expect(page.getByTestId('execution-json')).toContainText('"leaseValid": true')
  await expect(page.getByTestId('execution-json')).not.toContainText('m4-broker-secret')

  await runFixture(page, 'M4 Cycle Budget Fixture')
  await expect(page.locator('header').getByText('失败', { exact: true })).toBeVisible({ timeout: 120_000 })
  await expect(page.getByText('ACTIVATION_BUDGET_EXCEEDED')).toBeVisible()
  expect(await page.getByRole('button', { name: /Cycle Step.*运行 \d+.*成功/ }).count()).toBeGreaterThan(1)
})
