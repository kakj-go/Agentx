import { expect, type Locator, type Page, test } from '@playwright/test'

const password = 'agentx-e2e-admin-password'
const resourceNames = [
  ['credential', 'M5 Model Fixture Credential', undefined],
  ['model', 'm5-fixture-model', undefined],
  ['mcp_server', 'M5 MCP Fixture', undefined],
  ['mcp_tool', 'Echo', 'M5 MCP Fixture'],
  ['sandbox_profile', 'M5 Python Fixture', undefined],
] as const
const resourceTabs = { credential: '凭证', model: '模型', mcp_server: 'MCP Server', mcp_tool: 'MCP Tool', sandbox_profile: '沙箱配置' } as const

type Execution = { id: string; status: string }
type Approval = { id: string; executionId: string; status: string }
type StudioDraft = {
  revision: number
  definition: {
    schemaVersion: string
    nodes: Array<{ type: string; resourceReferences: Array<{ bindingRole?: string }> }>
  }
  editorDocument: { bindingEdges: unknown[] }
}

async function login(page: Page) {
  await page.goto('/login')
  await page.getByLabel('用户名').fill('admin')
  await page.getByLabel('密码').fill(password)
  const response = page.waitForResponse((value) => value.url().endsWith('/api/v1/auth/login') && value.request().method() === 'POST')
  await page.getByRole('button', { name: '登录' }).click()
  await expect(page).toHaveURL(/\/$/)
  const token = ((await (await response).json()) as { accessToken: string }).accessToken
  const me = await page.request.get('/api/v1/auth/me', { headers: { Authorization: `Bearer ${token}` } })
  expect(me.ok()).toBeTruthy()
  return { token, userId: ((await me.json()) as { id: string }).id }
}

async function api<T>(page: Page, token: string, path: string): Promise<T> {
  const response = await page.request.get(`/api/v1${path}`, { headers: { Authorization: `Bearer ${token}` } })
  if (!response.ok()) {
    throw new Error(`${path}: ${response.status()} ${await response.text()}`)
  }
  return response.json() as Promise<T>
}

async function createWorkflow(page: Page, name: string) {
  await page.getByRole('link', { name: '工作流', exact: true }).click()
  await page.getByRole('button', { name: '新建工作流' }).click()
  const dialog = page.getByRole('dialog', { name: '新建工作流' })
  await dialog.getByLabel('工作流名称').fill(name)
  await dialog.getByLabel('描述').fill('M6 Workflow Studio Kubernetes E2E')
  await dialog.getByRole('button', { name: '保存' }).click()
  await expect(page).toHaveURL(/\/workflows\/[0-9a-f-]+$/)
  return page.url().split('/').at(-1) as string
}

async function grantResource(page: Page, resourceType: keyof typeof resourceTabs, resourceName: string, detail: string | undefined, workflowName: string) {
  await page.goto('/resource-grants')
  await page.getByRole('tab', { name: resourceTabs[resourceType], exact: true }).click()
  const search = page.getByRole('searchbox', { name: '搜索资源名称、类型或连接信息' })
  await search.fill(resourceName)
  const rows = page.getByRole('row').filter({ hasText: resourceName })
  const row = detail ? rows.filter({ hasText: detail }).first() : rows.first()
  await expect(row).toBeVisible()
  await row.getByRole('button', { name: '管理授权' }).click()
  const dialog = page.getByRole('dialog').filter({ hasText: resourceName })
  await expect(dialog).toBeVisible()
  const subject = dialog.getByRole('combobox').nth(1)
  await subject.click()
  await page.getByRole('option', { name: workflowName, exact: true }).click()
  const revokeButtons = dialog.getByRole('button', { name: '撤销授权' })
  const grantCount = await revokeButtons.count()
  await dialog.getByRole('button', { name: '添加授权' }).click()
  await expect(revokeButtons).toHaveCount(grantCount + 1)
  await dialog.getByRole('button', { name: '取消' }).click()
}

function flowNode(page: Page, label: string) {
  return page.locator('.react-flow__node').filter({ hasText: label }).first()
}

async function connect(page: Page, source: Locator, sourceHandle: string, target: Locator, targetHandle: string) {
  const edges = page.locator('.react-flow__edge')
  const edgeCount = await edges.count()
  const from = source.locator(`.react-flow__handle.source[data-handleid="${sourceHandle}"]`)
  const to = target.locator(`.react-flow__handle.target[data-handleid="${targetHandle}"]`)
  await expect(from).toBeVisible()
  await expect(to).toBeVisible()
  const fromBox = await from.boundingBox()
  const toBox = await to.boundingBox()
  expect(fromBox).not.toBeNull()
  expect(toBox).not.toBeNull()
  await page.mouse.move(fromBox!.x + fromBox!.width / 2, fromBox!.y + fromBox!.height / 2)
  await page.mouse.down()
  await page.mouse.move(toBox!.x + toBox!.width / 2, toBox!.y + toBox!.height / 2, { steps: 12 })
  await page.mouse.up()
  await expect(edges).toHaveCount(edgeCount + 1)
}

async function choose(page: Page, scope: Locator, option: string | RegExp) {
  const trigger = await scope.getAttribute('role') === 'combobox' ? scope : scope.getByRole('combobox')
  await trigger.click()
  await page.getByRole('option', { name: option }).click()
}

async function fillMonaco(page: Page, scope: Locator, value: string) {
  const monaco = scope.getByRole('textbox', { name: 'Editor content' })
  const editor = (await monaco.count()) ? monaco : scope.locator('[contenteditable="true"]').first()
  await expect(editor).toBeVisible({ timeout: 30_000 })
  await editor.focus()
  await expect(editor).toBeFocused()
  await page.keyboard.press('Control+A')
  await page.keyboard.insertText(value)
  await expect.poll(async () => {
    const lines = await scope.locator('.view-lines').textContent()
    return (lines ?? await editor.textContent())?.replaceAll('\u00a0', ' ')
  }).toContain(value.split('\n')[0])
}

async function saveAndReadDraft(page: Page, token: string, workflowId: string) {
  const save = page.getByRole('button', { name: '保存', exact: true })
  if (await save.isEnabled()) await save.click()
  await expect(page.locator('header').getByText(/Revision \d+ · 已保存/)).toBeVisible({ timeout: 30_000 })
  return api<StudioDraft>(page, token, `/workflows/${workflowId}/draft`)
}

async function startDebug(page: Page, action: () => Promise<void>, confirm = true) {
  if (confirm) page.once('dialog', (dialog) => dialog.accept())
  const response = page.waitForResponse((value) => value.url().includes('/debug-executions') && value.request().method() === 'POST')
  await action()
  const accepted = await response
  if (accepted.status() !== 202) {
    throw new Error(`debug execution: ${accepted.status()} ${await accepted.text()}`)
  }
  return (await accepted.json() as { executionId: string }).executionId
}

async function waitExecution(page: Page, token: string, executionId: string, statuses: string[], timeout = 180_000) {
  let current: Execution | undefined
  await expect.poll(async () => {
    current = await api<Execution>(page, token, `/executions/${executionId}`)
    return statuses.includes(current.status)
  }, { timeout, intervals: [500, 750, 1000, 2000] }).toBeTruthy()
  return current!
}

async function approveExecution(page: Page, token: string, executionId: string) {
  let approval: Approval | undefined
  await expect.poll(async () => {
    const result = await api<{ items: Approval[] }>(page, token, '/approvals?pageSize=100')
    approval = result.items.find((item) => item.executionId === executionId && item.status === 'pending')
    return approval?.id
  }, { timeout: 120_000 }).toBeTruthy()
  await page.goto(`/approvals/${approval!.id}`)
  await page.getByRole('button', { name: '领取' }).click()
  await expect(page.getByRole('button', { name: '通过' })).toBeVisible()
  await page.getByRole('button', { name: '通过' }).click()
  const dialog = page.getByRole('dialog', { name: '通过' })
  await dialog.getByRole('button', { name: '通过' }).click()
  await expect.poll(async () => {
    const result = await api<{ items: Approval[] }>(page, token, '/approvals?pageSize=100')
    return result.items.find((item) => item.id === approval!.id)?.status
  }).toBe('approved')
}

async function selectDebugMode(page: Page, mode: 'full' | 'single_node' | 'to_node' | 'from_node') {
  const labels = { full: '完整流程', single_node: '单节点', to_node: '运行到节点', from_node: '从节点继续' }
  await page.getByRole('combobox', { name: '调试输入' }).click()
  await page.getByRole('option', { name: labels[mode], exact: true }).click()
}

async function setLocale(page: Page, locale: 'zh-CN' | 'en-US') {
  if (await page.locator('html').getAttribute('lang') === locale) return
  await page.getByRole('button', { name: /^(语言|Language)$/ }).click()
  await page.getByRole('menuitemradio', { name: locale === 'zh-CN' ? '简体中文' : 'English' }).click()
  await expect(page.locator('html')).toHaveAttribute('lang', locale)
}

async function setTheme(page: Page, theme: 'light' | 'dark') {
  await page.getByRole('button', { name: /^(主题|Theme)$/ }).click()
  await page.getByRole('menuitemradio', { name: theme === 'light' ? /^(浅色|Light)$/ : /^(深色|Dark)$/ }).click()
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme)
}

test('M6 Studio creates, debugs, versions and publishes a manifest-driven Workflow', async ({ context, page }, testInfo) => {
  const { token, userId } = await login(page)
  const workflowName = `M6 Studio ${Date.now()}`
  const workflowId = await createWorkflow(page, workflowName)

  for (const [resourceType, resourceName, detail] of resourceNames) await grantResource(page, resourceType, resourceName, detail, workflowName)

  await page.goto(`/workflows/${workflowId}/editor`)
  await expect(page.getByTestId('workflow-canvas')).toBeVisible()
  for (const type of ['agent', 'code', 'approval']) await page.getByTestId(`palette-action-${type}`).click()
  for (const type of ['model', 'mcp_tool']) await page.getByTestId(`palette-binding-${type}`).click()
  await page.getByRole('button', { name: 'Fit View' }).click()

  const trigger = flowNode(page, 'Manual Trigger')
  const agent = flowNode(page, 'Agent')
  const code = flowNode(page, 'Code')
  const approval = flowNode(page, 'Approval')
  const model = page.locator('.react-flow__node-attachment').filter({ hasText: 'model' }).first()
  const tool = page.locator('.react-flow__node-attachment').filter({ hasText: 'mcp_tool' }).first()
  await expect(approval).toBeVisible()

  await model.click()
  await choose(page, page.getByTestId('attachment-resource'), /m5-fixture-model/)
  await tool.click()
  await choose(page, page.getByTestId('attachment-resource'), /Echo/)

  await agent.click()
  await fillMonaco(page, page.getByTestId('parameter-systemPrompt'), 'Use the attached resources and return a concise result.')

  await code.click()
  await choose(page, page.getByTestId('parameter-runner'), /^python$/)
  await fillMonaco(page, page.getByTestId('parameter-source'), 'print("m6-studio-ok")')
  await choose(page, page.getByTestId('resource-selector-sandbox_profile'), /M5 Python Fixture/)

  await approval.click()
  await page.getByTestId('parameter-title').getByRole('textbox').fill('M6 Studio Approval')
  await page.getByTestId('parameter-description').getByRole('textbox').fill('Approve the Workflow created through the Studio UI.')
  await page.getByTestId('parameter-candidateUserId').getByRole('textbox').fill(userId)

  await connect(page, trigger, 'main', agent, 'main')
  await connect(page, agent, 'main', code, 'main')
  await connect(page, code, 'main', approval, 'main')
  await connect(page, model, 'resource', agent, 'binding:ai_model')
  await connect(page, tool, 'resource', agent, 'binding:ai_tool')
  await expect(page.locator('.react-flow__edge')).toHaveCount(5)

  const draft = await saveAndReadDraft(page, token, workflowId)
  expect(draft.definition.schemaVersion).toBe('3.0')
  expect(draft.definition.nodes.map((node) => node.type)).toEqual(expect.arrayContaining(['manual_trigger', 'agent', 'code', 'approval']))
  expect(draft.definition.nodes.find((node) => node.type === 'agent')?.resourceReferences.map((item) => item.bindingRole)).toEqual(expect.arrayContaining(['ai_model', 'ai_tool']))
  expect(draft.editorDocument.bindingEdges).toHaveLength(2)

  const firstExecution = await startDebug(page, () => page.getByRole('button', { name: '运行', exact: true }).click())
  const approvalPage = await context.newPage()
  await approveExecution(approvalPage, token, firstExecution)
  await waitExecution(page, token, firstExecution, ['succeeded'])

  await code.click()
  await expect(page.getByRole('tabpanel').getByRole('textbox')).toHaveValue(/m6-studio-ok/, { timeout: 30_000 })
  await page.getByRole('tab', { name: 'Trace' }).click()
  await expect(page.getByRole('tabpanel')).toContainText(/agentRuns|sandboxes/, { timeout: 30_000 })
  await page.getByRole('tab', { name: '事件' }).click()
  await expect(page.getByRole('tabpanel')).toContainText('node.completed')

  await agent.click()
  await page.getByRole('button', { name: '模拟', exact: true }).click()
  await expect(page.getByText('调试覆盖已保存', { exact: true })).toBeVisible()
  await code.click()
  await page.getByRole('button', { name: '固定', exact: true }).click()
  await expect(page.getByText('调试覆盖已保存', { exact: true })).toBeVisible()

  let failedEventPolls = 0
  await page.route('**/api/v1/executions/*/events?*', async (route) => {
    if (failedEventPolls++ < 2) await route.abort('connectionfailed')
    else await route.continue()
  })
  const overlayExecution = await startDebug(page, () => page.getByRole('button', { name: '运行', exact: true }).click())
  await approveExecution(approvalPage, token, overlayExecution)
  await waitExecution(page, token, overlayExecution, ['succeeded'])
  await page.unroute('**/api/v1/executions/*/events?*')
  const overlayEvents = await api<{ items: Array<{ eventType: string }> }>(page, token, `/executions/${overlayExecution}/events?after=0&limit=200`)
  expect(overlayEvents.items.filter((item) => item.eventType === 'node.debug_overlay_applied')).toHaveLength(2)
  await expect(page.getByRole('tab', { name: '事件' })).toBeVisible()

  await code.click()
  await selectDebugMode(page, 'single_node')
  const singleExecution = await startDebug(page, async () => {
    await page.getByRole('button', { name: '运行', exact: true }).click()
    const dialog = page.getByRole('dialog', { name: '调试输入' })
    await dialog.getByRole('button', { name: '运行', exact: true }).click()
  })
  await waitExecution(page, token, singleExecution, ['succeeded'])

  await selectDebugMode(page, 'to_node')
  const toExecution = await startDebug(page, () => page.getByRole('button', { name: '运行', exact: true }).click())
  await waitExecution(page, token, toExecution, ['succeeded'])

  await selectDebugMode(page, 'from_node')
  const fromExecution = await startDebug(page, async () => {
    await page.getByRole('button', { name: '运行', exact: true }).click()
    const dialog = page.getByRole('dialog', { name: '调试输入' })
    await dialog.getByRole('button', { name: '运行', exact: true }).click()
  })
  await approveExecution(approvalPage, token, fromExecution)
  await waitExecution(page, token, fromExecution, ['succeeded'])

  await page.getByRole('tab', { name: '检查点' }).click()
  const forkButton = page.getByRole('button', { name: 'Fork' }).last()
  await expect(forkButton).toBeVisible({ timeout: 30_000 })
  const forkResponse = page.waitForResponse((value) => value.url().endsWith(`/executions/${fromExecution}/fork`) && value.request().method() === 'POST')
  await forkButton.click()
  const forkAccepted = await forkResponse
  expect(forkAccepted.status()).toBe(202)
  const forkExecution = (await forkAccepted.json() as { executionId: string }).executionId
  const stopFork = page.getByRole('button', { name: '停止', exact: true })
  if (await stopFork.isVisible()) {
    await stopFork.click()
    await waitExecution(page, token, forkExecution, ['cancelled', 'failed', 'succeeded'])
  } else await expect(page.getByRole('button', { name: '运行', exact: true })).toBeVisible()

  await selectDebugMode(page, 'full')
  const stoppedExecution = await startDebug(page, () => page.getByRole('button', { name: '运行', exact: true }).click())
  await waitExecution(page, token, stoppedExecution, ['waiting', 'waiting_approval'])
  await page.getByRole('button', { name: '停止', exact: true }).click()
  await waitExecution(page, token, stoppedExecution, ['cancelled'])

  await page.getByRole('button', { name: '版本', exact: true }).click()
  const versionDialog = page.getByRole('dialog', { name: '工作流版本' })
  await versionDialog.getByRole('button', { name: '创建版本' }).click()
  await expect(page.getByText('版本已创建', { exact: true })).toBeVisible()
  const versions = await api<Array<{ id: string; definition: unknown }>>(page, token, `/workflows/${workflowId}/versions`)
  expect(versions).toHaveLength(1)
  expect(JSON.stringify(versions[0].definition)).not.toMatch(/pin_data|mock_output|debugOverlay/)

  await page.getByRole('button', { name: '发布', exact: true }).click()
  const publishDialog = page.getByRole('dialog', { name: '发布工作流' })
  await choose(page, publishDialog.getByLabel('环境'), /Development/)
  await choose(page, publishDialog.getByLabel('版本'), /v1/)
  await publishDialog.getByRole('button', { name: '发布', exact: true }).click()
  await expect(page.getByText('工作流已发布', { exact: true })).toBeVisible()

  const viewports = [{ width: 1280, height: 800 }, { width: 1440, height: 900 }, { width: 1920, height: 1080 }]
  for (const locale of ['zh-CN', 'en-US'] as const) {
    await setLocale(page, locale)
    for (const theme of ['light', 'dark'] as const) {
      await setTheme(page, theme)
      for (const viewport of viewports) {
        await page.setViewportSize(viewport)
        await expect(page.getByTestId('workflow-canvas')).toBeVisible()
        await expect(page.locator('body')).not.toContainText(/studio\.[A-Za-z]/)
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBeTruthy()
        await page.screenshot({ path: testInfo.outputPath(`studio-${viewport.width}x${viewport.height}-${locale}-${theme}.png`), fullPage: true })
      }
    }
  }

  const concurrent = await context.newPage()
  await concurrent.goto(`/workflows/${workflowId}/editor`)
  await expect(concurrent.getByTestId('workflow-canvas')).toBeVisible()
  await flowNode(concurrent, 'code').click()
  await concurrent.getByLabel('Name').fill('code concurrent')
  await concurrent.getByRole('button', { name: 'Save', exact: true }).click()
  await expect(concurrent.locator('header').getByText(/Revision \d+ · Saved/)).toBeVisible()
  await code.click()
  await page.getByLabel('Name').fill('code local')
  await page.getByRole('button', { name: 'Save', exact: true }).click()
  await expect(page.getByRole('dialog', { name: 'Draft revision conflict' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Keep local copy' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Load server revision' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Overwrite server draft' })).toBeVisible()

  await concurrent.close()
  await approvalPage.close()
})
