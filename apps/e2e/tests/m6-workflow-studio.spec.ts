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
type Application = { id: string; name: string; slug: string }
type WorkflowVersion = { id: string; versionNumber: number }
type GatewayInvocation = { id: string; executionId?: string; status: string; errorCode?: string | null; errorMessage?: string | null }
type GatewayMessage = { role: string; parts: Array<{ content?: unknown }> }
type StudioDraft = {
  revision: number
  definition: {
    schemaVersion: string
    settings: { primaryOutputNodeId?: string | null }
    nodes: Array<{ id: string; type: string; name: string; resourceReferences: Array<{ bindingRole?: string }>; settings: { onError?: string } }>
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

async function mutate<T>(page: Page, token: string, path: string, method: 'POST' | 'PUT' | 'PATCH', body: unknown): Promise<T> {
  const response = await page.request.fetch(`/api/v1${path}`, { method, data: body, headers: { Authorization: `Bearer ${token}` } })
  if (!response.ok()) throw new Error(`${path}: ${response.status()} ${await response.text()}`)
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

function flowNode(page: Page, role: string) {
  const labels: Record<string, RegExp> = {
    trigger: /手动触发|Manual Trigger/,
    agent: /智能体|Agent/,
    code: /代码|Code/,
    approval: /审批|Approval/,
    error_handler: /错误处理|Error Handler/,
    merge: /合并|Merge/,
  }
  return page.locator('.react-flow__node').filter({ hasText: labels[role] ?? new RegExp(role, 'i') }).first()
}

function studioRun(page: Page) {
  return page.locator('header').getByRole('button', { name: '运行', exact: true })
}

async function addFromCreator(page: Page, testId: string) {
  if (!await page.getByTestId('node-creator').isVisible()) await page.getByTestId('node-creator-rail').getByRole('button', { name: '搜索节点' }).click()
  await expect(page.getByTestId('node-creator')).toBeVisible()
  await page.getByTestId(testId).click()
  await expect(page.getByTestId('node-creator')).toBeHidden()
}

async function openNodeDetails(page: Page, node: Locator) {
  const details = page.getByTestId('node-details-view')
  if (await details.isVisible()) await details.getByRole('button', { name: /^(关闭|Close)$/ }).click()
  await node.click()
  await expect(details).toBeVisible()
  return details
}

async function connect(page: Page, source: Locator, sourceHandle: string, target: Locator, targetHandle: string) {
  const edges = page.locator('.react-flow__edge')
  const edgeCount = await edges.count()
  const from = source.locator(`.react-flow__handle.source[data-handleid="${sourceHandle}"]`)
  const to = target.locator(`.react-flow__handle.target[data-handleid="${targetHandle}"]`)
  await expect(from).toBeVisible()
  await expect(to).toBeVisible()
  let boxes: Awaited<ReturnType<Locator['boundingBox']>>[] | undefined
  await expect.poll(async () => {
    const next = await Promise.all([from.boundingBox(), to.boundingBox()])
    if (next.some((box) => box === null)) return false
    boxes = next
    return true
  }).toBeTruthy()
  const [fromBox, toBox] = boxes!
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
  await scope.scrollIntoViewIfNeeded()
  const monaco = scope.getByRole('textbox', { name: 'Editor content' })
  const richText = scope.locator('[contenteditable="true"]').first()
  const usesMonaco = await richText.count() === 0
  const editor = usesMonaco ? monaco : richText
  await expect(editor).toBeVisible({ timeout: 30_000 })
  if (usesMonaco) {
    await editor.focus()
    await expect(editor).toBeFocused()
    await page.keyboard.press('Control+A')
    await page.keyboard.insertText(value)
    await expect.poll(async () => (await scope.locator('.view-lines:visible').textContent())?.replaceAll('\u00a0', ' ')).toContain(value.split('\n')[0])
  } else {
    await editor.fill(value)
    await expect(editor).toContainText(value.split('\n')[0])
  }
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
  await expect(page.getByTestId('node-creator')).toBeHidden()
  const rail = page.getByTestId('node-creator-rail')
  await expect(rail.getByRole('button', { name: '搜索节点' })).toBeVisible()
  await expect(rail.getByRole('button', { name: '便签' })).toBeVisible()
  await expect(rail.getByRole('button', { name: '分组' })).toBeVisible()
  await expect(rail.getByTestId('rail-action-agent')).toBeVisible()
  await expect(rail.getByTestId('rail-action-error_handler')).toBeVisible()
  expect(await rail.locator('[data-testid^="rail-action-"]').count()).toBeGreaterThan(8)
  const initialTrigger = flowNode(page, 'trigger')
  await initialTrigger.hover()
  await initialTrigger.getByRole('button', { name: /后添加节点/ }).click({ force: true })
  await page.getByTestId('palette-action-agent').click()
  await expect(page.locator('.react-flow__edge')).toHaveCount(1)
  for (const type of ['code', 'approval']) await addFromCreator(page, `palette-action-${type}`)
  for (const type of ['model', 'mcp_tool']) await addFromCreator(page, `palette-binding-${type}`)
  await page.getByRole('button', { name: 'Fit View' }).click()

  const agent = flowNode(page, 'agent')
  const code = flowNode(page, 'code')
  const approval = flowNode(page, 'approval')
  const model = page.locator('.react-flow__node-attachment').filter({ hasText: /model|模型/i }).first()
  const tool = page.locator('.react-flow__node-attachment').filter({ hasText: /mcp[ _]tool|工具/i }).first()
  await expect(approval).toBeVisible()

  await openNodeDetails(page, model)
  await choose(page, page.getByTestId('attachment-resource'), /m5-fixture-model/)
  await openNodeDetails(page, tool)
  await choose(page, page.getByTestId('attachment-resource'), /Echo/)

  await openNodeDetails(page, agent)
  await fillMonaco(page, page.getByTestId('parameter-systemPrompt'), 'Use the attached resources and return a concise result.')

  await openNodeDetails(page, code)
  await choose(page, page.getByTestId('parameter-runner'), /^python$/)
  await fillMonaco(page, page.getByTestId('parameter-source'), 'print("m6-studio-ok")')
  await choose(page, page.getByTestId('resource-selector-sandbox_profile'), /M5 Python Fixture/)

  await openNodeDetails(page, approval)
  await page.getByTestId('parameter-title').getByRole('textbox').fill('M6 Studio Approval')
  await page.getByTestId('parameter-description').getByRole('textbox').fill('Approve the Workflow created through the Studio UI.')
  await page.getByTestId('parameter-candidateUserId').getByRole('textbox').fill(userId)

  await page.getByTestId('node-details-view').getByRole('button', { name: /^(关闭|Close)$/ }).click()
  await page.getByRole('button', { name: 'Fit View' }).click()
  await connect(page, agent, 'main', code, 'main')
  await connect(page, code, 'main', approval, 'main')
  await connect(page, model, 'resource', agent, 'binding:ai_model')
  await connect(page, tool, 'resource', agent, 'binding:ai_tool')
  await expect(page.locator('.react-flow__edge')).toHaveCount(5)

  await code.hover()
  await code.getByRole('button', { name: /在 错误 后添加节点/ }).click({ force: true })
  await expect(page.getByTestId('node-creator')).toBeVisible()
  await expect(page.getByTestId('palette-action-error_handler')).toBeVisible()
  await page.getByTestId('palette-action-error_handler').click()
  await expect(page.getByTestId('node-creator')).toBeHidden()
  await expect(page.locator('.react-flow__edge')).toHaveCount(6)
  const errorHandler = flowNode(page, 'error_handler')
  await expect(errorHandler).toBeVisible()
  await expect(page.getByTestId('node-details-view')).toBeVisible()

  const approvalDetails = await openNodeDetails(page, approval)
  await approvalDetails.getByRole('button', { name: '更多节点操作' }).click()
  await page.getByRole('menuitem', { name: '设为主要输出' }).click()
  await expect(approval.locator('[title="主要输出"]')).toBeVisible()

  await openNodeDetails(page, code)
  await page.getByRole('button', { name: '关闭' }).click()

  await page.getByTestId('node-creator-rail').getByRole('button', { name: '便签' }).click()
  const note = page.locator('[data-testid^="studio-note-"]').first()
  await expect(note).toBeVisible()
  await note.dispatchEvent('dblclick')
  await note.getByRole('textbox').fill('M6 runtime notes')
  await note.getByRole('textbox').press('Control+Enter')
  await note.getByRole('button', { name: /便签颜色：blue/ }).click()

  await code.click()
  await page.keyboard.down('Shift')
  await approval.click()
  await page.keyboard.up('Shift')
  await page.getByTestId('node-creator-rail').getByRole('button', { name: '分组' }).click()
  const group = page.locator('[data-testid^="studio-group-"]').first()
  await expect(group).toBeVisible()
  await group.getByRole('button', { name: '折叠分组' }).click()
  await expect(group.getByRole('button', { name: '展开分组' })).toBeVisible()
  await group.getByRole('button', { name: '展开分组' }).click()

  const draft = await saveAndReadDraft(page, token, workflowId)
  expect(draft.definition.schemaVersion).toBe('3.0')
  expect(draft.definition.nodes.map((node) => node.type)).toEqual(expect.arrayContaining(['manual_trigger', 'agent', 'code', 'approval', 'error_handler']))
  expect(draft.definition.nodes.find((node) => node.type === 'agent')?.resourceReferences.map((item) => item.bindingRole)).toEqual(expect.arrayContaining(['ai_model', 'ai_tool']))
  expect(draft.definition.nodes.find((node) => node.type === 'code')?.settings.onError).toBe('continue_error_output')
  expect(draft.definition.settings.primaryOutputNodeId).toBe(draft.definition.nodes.find((node) => node.type === 'approval')?.id)
  expect(draft.editorDocument.bindingEdges).toHaveLength(2)

  const firstExecution = await startDebug(page, () => studioRun(page).click())
  const approvalPage = await context.newPage()
  await approveExecution(approvalPage, token, firstExecution)
  await waitExecution(page, token, firstExecution, ['succeeded'])

  const details = await openNodeDetails(page, code)
  await details.getByRole('tab', { name: '输出' }).click()
  await expect(details.getByRole('textbox', { name: '输出' })).toHaveValue(/m6-studio-ok/, { timeout: 30_000 })
  await details.getByRole('tab', { name: 'Trace' }).click()
  await expect(details.getByRole('tabpanel')).toContainText(/runs|sandboxes/, { timeout: 30_000 })
  const runtimeRail = page.getByTestId('runtime-rail')
  await runtimeRail.getByRole('tab', { name: '事件' }).click()
  await expect(runtimeRail.getByRole('tabpanel')).toContainText('node.completed')
  await runtimeRail.getByRole('button', { name: '折叠执行轨道' }).click()
  await expect(runtimeRail.getByRole('button', { name: '展开执行轨道' })).toBeVisible()
  await runtimeRail.getByRole('button', { name: '展开执行轨道' }).click()

  const agentDetails = await openNodeDetails(page, agent)
  await agentDetails.getByRole('tab', { name: '输出' }).click()
  const mockResponse = page.waitForResponse((value) => value.url().includes('/debug-overlays/') && value.request().method() === 'PUT')
  await agentDetails.getByRole('button', { name: '模拟', exact: true }).click()
  expect((await mockResponse).ok()).toBeTruthy()
  const codeDetails = await openNodeDetails(page, code)
  await codeDetails.getByRole('tab', { name: '输出' }).click()
  const pinResponse = page.waitForResponse((value) => value.url().includes('/debug-overlays/') && value.request().method() === 'PUT')
  await codeDetails.getByRole('button', { name: '固定', exact: true }).click()
  expect((await pinResponse).ok()).toBeTruthy()

  let failedEventPolls = 0
  await page.route('**/api/v1/executions/*/events?*', async (route) => {
    if (failedEventPolls++ < 2) await route.abort('connectionfailed')
    else await route.continue()
  })
  const overlayExecution = await startDebug(page, () => studioRun(page).click())
  await approveExecution(approvalPage, token, overlayExecution)
  await waitExecution(page, token, overlayExecution, ['succeeded'])
  await page.unroute('**/api/v1/executions/*/events?*')
  const overlayEvents = await api<{ items: Array<{ eventType: string }> }>(page, token, `/executions/${overlayExecution}/events?after=0&limit=200`)
  expect(overlayEvents.items.filter((item) => item.eventType === 'node.debug_overlay_applied')).toHaveLength(2)
  await expect(page.getByRole('tab', { name: '事件' })).toBeVisible()

  await openNodeDetails(page, code)
  await selectDebugMode(page, 'single_node')
  const singleExecution = await startDebug(page, async () => {
    await studioRun(page).click()
    const dialog = page.getByRole('dialog', { name: '调试输入' })
    await dialog.getByRole('button', { name: '运行', exact: true }).click()
  })
  await waitExecution(page, token, singleExecution, ['succeeded'])

  await selectDebugMode(page, 'to_node')
  const toExecution = await startDebug(page, () => studioRun(page).click())
  await waitExecution(page, token, toExecution, ['succeeded'])

  await selectDebugMode(page, 'from_node')
  const fromExecution = await startDebug(page, async () => {
    await studioRun(page).click()
    const dialog = page.getByRole('dialog', { name: '调试输入' })
    await dialog.getByRole('button', { name: '运行', exact: true }).click()
  })
  await approveExecution(approvalPage, token, fromExecution)
  await waitExecution(page, token, fromExecution, ['succeeded'])

  await page.getByTestId('runtime-rail').getByRole('tab', { name: '检查点' }).click()
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
  } else await expect(studioRun(page)).toBeVisible()

  await selectDebugMode(page, 'full')
  const stoppedExecution = await startDebug(page, () => studioRun(page).click())
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
  const concurrentDetails = await openNodeDetails(concurrent, flowNode(concurrent, 'code'))
  await concurrentDetails.getByRole('tab', { name: 'Parameters' }).click()
  await concurrentDetails.getByLabel('Name').fill('code concurrent')
  await concurrent.getByRole('button', { name: 'Save', exact: true }).click()
  await expect(concurrent.locator('header').getByText(/Revision \d+ · Saved/)).toBeVisible()
  const localDetails = await openNodeDetails(page, code)
  await localDetails.getByRole('tab', { name: 'Parameters' }).click()
  await localDetails.getByLabel('Name').fill('code local')
  await page.getByRole('button', { name: 'Save', exact: true }).click()
  await expect(page.getByRole('dialog', { name: 'Draft revision conflict' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Keep local copy' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Load server revision' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Overwrite server draft' })).toBeVisible()

  await concurrent.close()
  await approvalPage.close()
})

test('M6 Studio makes dual-Agent output selection explicit across serial, parallel and Merge topologies', async ({ page }, testInfo) => {
  const { token } = await login(page)
  const workflowName = `M6 Multi Agent ${Date.now()}`
  const workflowId = await createWorkflow(page, workflowName)
  await grantResource(page, 'credential', 'M5 Model Fixture Credential', undefined, workflowName)
  await grantResource(page, 'model', 'm5-fixture-model', undefined, workflowName)

  await page.goto(`/workflows/${workflowId}/editor`)
  const trigger = flowNode(page, 'trigger')
  await trigger.hover()
  await trigger.getByRole('button', { name: /后添加节点/ }).click({ force: true })
  await page.getByTestId('palette-action-agent').click()
  await addFromCreator(page, 'palette-action-agent')
  await addFromCreator(page, 'palette-binding-model')

  const agents = () => page.locator('.react-flow__node').filter({ hasText: /智能体|Agent/ })
  const firstAgent = agents().first()
  const secondAgent = agents().nth(1)
  const firstDetails = await openNodeDetails(page, firstAgent)
  await firstDetails.getByLabel('名称').fill('Agent A')
  await fillMonaco(page, firstDetails.getByTestId('parameter-systemPrompt'), 'Return the serial Agent A result.')
  const secondDetails = await openNodeDetails(page, secondAgent)
  await secondDetails.getByLabel('名称').fill('Agent B')
  await fillMonaco(page, secondDetails.getByTestId('parameter-systemPrompt'), 'Return the selected Agent B result.')

  const model = page.locator('.react-flow__node-attachment').first()
  await openNodeDetails(page, model)
  await choose(page, page.getByTestId('attachment-resource'), /m5-fixture-model/)
  await page.getByTestId('node-details-view').getByRole('button', { name: '关闭' }).click()
  await page.getByRole('button', { name: 'Fit View' }).click()

  await connect(page, model, 'resource', firstAgent, 'binding:ai_model')
  await connect(page, model, 'resource', secondAgent, 'binding:ai_model')
  await connect(page, firstAgent, 'main', secondAgent, 'main')
  const serialDraft = await saveAndReadDraft(page, token, workflowId)
  expect(serialDraft.definition.nodes.filter((node) => node.type === 'agent')).toHaveLength(2)
  const serialExecution = await startDebug(page, () => studioRun(page).click())
  await waitExecution(page, token, serialExecution, ['succeeded'], 180_000)

  const serialEdge = page.locator('.react-flow__edge').last()
  await serialEdge.click({ force: true })
  await page.keyboard.press('Delete')
  await expect(page.locator('.react-flow__edge')).toHaveCount(3)
  await page.getByRole('button', { name: 'Fit View' }).click()
  await connect(page, trigger, 'main', secondAgent, 'main')
  const parallelDraft = await saveAndReadDraft(page, token, workflowId)
  const parallelVersion = await mutate<WorkflowVersion>(page, token, `/workflows/${workflowId}/versions`, 'POST', { draftRevision: parallelDraft.revision })
  const application = await mutate<Application>(page, token, '/applications', 'POST', { workflowId, name: workflowName, slug: `m6-multi-agent-${Date.now()}`, description: 'Dual Agent output contract E2E', visibility: 'company' })
  const environments = await api<Array<{ id: string; code: string }>>(page, token, '/environments')
  const environment = environments.find((item) => item.code === 'development')
  expect(environment).toBeTruthy()
  await mutate<{ id: string }>(page, token, `/workflows/${workflowId}/deployments`, 'POST', { workflowVersionId: parallelVersion.id, environmentId: environment!.id })
  const rejected = await page.request.post(`/api/v1/applications/${application.id}/deployments`, { data: { workflowVersionId: parallelVersion.id, environmentId: environment!.id, inputSchema: {}, outputSchema: {}, outputExpression: null, sessionVersionPolicy: 'pinned' }, headers: { Authorization: `Bearer ${token}` } })
  expect(rejected.status()).toBe(422)
  expect((await rejected.json() as { code: string }).code).toBe('APPLICATION_PRIMARY_OUTPUT_REQUIRED')

  await openNodeDetails(page, secondAgent)
  await page.getByTestId('node-details-view').getByRole('button', { name: '更多节点操作' }).click()
  await page.getByRole('menuitem', { name: '设为主要输出' }).click()
  const primaryDraft = await saveAndReadDraft(page, token, workflowId)
  const primaryVersion = await mutate<WorkflowVersion>(page, token, `/workflows/${workflowId}/versions`, 'POST', { draftRevision: primaryDraft.revision })
  await mutate<{ id: string }>(page, token, `/workflows/${workflowId}/deployments`, 'POST', { workflowVersionId: primaryVersion.id, environmentId: environment!.id })
  const deployment = await mutate<{ id: string }>(page, token, `/applications/${application.id}/deployments`, 'POST', { workflowVersionId: primaryVersion.id, environmentId: environment!.id, inputSchema: {}, outputSchema: {}, outputExpression: null, sessionVersionPolicy: 'pinned' })
  expect(deployment.id).toBeTruthy()

  await page.goto('/playground')
  await page.getByRole('combobox').click()
  await page.getByRole('option', { name: workflowName, exact: true }).click()
  const sessionResponse = page.waitForResponse((value) => value.url().includes('/gateway/v1/applications/') && value.url().endsWith('/sessions') && value.request().method() === 'POST')
  await page.getByRole('button', { name: '新建会话' }).click()
  const session = await (await sessionResponse).json() as { id: string }
  const invocationResponse = page.waitForResponse((value) => value.url().includes('/gateway/v1/sessions/') && value.url().endsWith('/messages') && value.request().method() === 'POST')
  await page.getByPlaceholder('输入消息进行测试…').fill('M6 dual Agent output')
  await page.getByRole('button', { name: '发送' }).click()
  const invocation = await (await invocationResponse).json() as GatewayInvocation
  let completed: GatewayInvocation | undefined
  await expect.poll(async () => {
    const response = await page.request.get(`/gateway/v1/invocations/${invocation.id}`, { headers: { Authorization: `Bearer ${token}` } })
    completed = await response.json() as GatewayInvocation
    return completed.status
  }, { timeout: 180_000, intervals: [500, 1_000, 2_000] }).toBe('completed')
  expect(completed?.errorCode).toBeNull()
  const messagesResponse = await page.request.get(`/gateway/v1/sessions/${session.id}/messages`, { headers: { Authorization: `Bearer ${token}` } })
  const messages = await messagesResponse.json() as GatewayMessage[]
  const assistant = messages.find((message) => message.role === 'assistant')
  expect(assistant).toBeTruthy()
  const nodeRuns = await api<{ items: Array<{ nodeId: string; status: string; output?: { main?: Array<{ json?: { message?: { content?: string }; output?: string; text?: string } }> } }> }>(page, token, `/executions/${completed!.executionId}/nodes`)
  const selectedRun = nodeRuns.items.find((item) => item.nodeId === primaryDraft.definition.settings.primaryOutputNodeId && item.status === 'succeeded')
  const selectedJson = selectedRun?.output?.main?.[0]?.json
  const selectedText = selectedJson?.message?.content ?? selectedJson?.output ?? selectedJson?.text
  expect(selectedText).toBeTruthy()
  expect(assistant!.parts.some((part) => part.content === selectedText)).toBeTruthy()

  await page.goto(`/workflows/${workflowId}/editor`)
  await expect(page.getByTestId('workflow-canvas')).toBeVisible()
  await addFromCreator(page, 'palette-action-merge')
  const merge = flowNode(page, 'merge')
  await page.getByTestId('node-details-view').getByRole('button', { name: '关闭' }).click()
  await page.getByRole('button', { name: 'Fit View' }).click()
  await connect(page, firstAgent, 'main', merge, 'main')
  await connect(page, secondAgent, 'main', merge, 'main')
  await expect(page.getByText('主要输出已清除：该节点新增了正常输出连线')).toBeVisible()
  const mergedDraft = await saveAndReadDraft(page, token, workflowId)
  expect(mergedDraft.definition.nodes.some((node) => node.type === 'merge')).toBeTruthy()
  expect(mergedDraft.definition.settings.primaryOutputNodeId).toBeFalsy()
  const mergedExecution = await startDebug(page, () => studioRun(page).click())
  await waitExecution(page, token, mergedExecution, ['succeeded'], 180_000)

  await page.screenshot({ path: testInfo.outputPath('dual-agent-output-semantics.png'), fullPage: true })
})
