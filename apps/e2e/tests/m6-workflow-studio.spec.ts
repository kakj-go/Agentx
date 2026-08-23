import { expect, type Locator, type Page, test } from '@playwright/test'
import { readFile } from 'node:fs/promises'

import { publishCompatibleChatMapping, useRuntimePortForward } from './playground-helpers'

const password = 'agentx-e2e-admin-password'
const gatewayBase = process.env.AGENTX_E2E_RUNTIME_URL ?? ''
const echoBaseUrl = process.env.AGENTX_E2E_ECHO_BASE_URL ?? 'http://echo-mcp:8090'
const studioCredentialName = 'M6 Studio Fixture Credential'
const studioModelName = 'm6-studio-fixture-model'
const studioMcpName = 'M6 Studio Fixture MCP'
const studioSandboxName = 'M6 Studio Python Fixture'
const resourceNames = [
  ['credential', studioCredentialName, undefined],
  ['model', studioModelName, undefined],
  ['mcp_server', studioMcpName, undefined],
  ['mcp_tool', 'Echo', studioMcpName],
  ['sandbox_profile', studioSandboxName, undefined],
] as const
const resourceTabs = { credential: '凭证', model: '模型', mcp_server: 'MCP 服务', mcp_tool: 'MCP 工具', sandbox_profile: '沙箱配置' } as const

type Execution = { id: string; status: string; errorCode?: string | null }
type Approval = { id: string; executionId: string; status: string }
type Application = { id: string; name: string; slug: string }
type ApplicationDeployment = { id: string; status: string; publishErrorCode?: string | null; publishErrorMessage?: string | null }
type WorkflowVersion = { id: string; versionNumber: number }
type GatewayInvocation = { id: string; executionId?: string; status: string; error?: unknown | null }
type GatewayMessage = { role: string; parts: Array<{ content?: unknown }> }
type PageResponse<T> = { items: T[] }
type NamedResource = { id: string; name?: string; alias?: string; serverId?: string; version?: number }
type StudioDraft = {
  revision: number
  definition: {
    schemaVersion: string
    start: { inputs: unknown; contexts: Record<string, unknown> }
    settings: Record<string, unknown>
    nodes: Array<{ id: string; key: string; type: string; name: string; parameters: Record<string, unknown>; resourceReferences: Array<{ bindingRole?: string }>; outputProjection: Record<string, Record<string, { value: DynamicValue }>>; contextWrites: Array<{ operation: string; path: string; value: DynamicValue }>; settings: { onError?: string } }>
    connections: Array<{ id: string; sourceNodeId: string; sourceHandle: string; targetNodeId: string; targetHandle: string; order: number }>
    end: { outputs: Record<string, { value: DynamicValue }>; error: { strategy: string; collectWindowMs: number; outputs: Record<string, { value: DynamicValue }> } }
  }
  editorDocument: { bindingEdges: unknown[] }
}

async function login(page: Page) {
  const bootstrapStatus = await page.request.get('/api/v1/bootstrap/status')
  if (!bootstrapStatus.ok()) throw new Error(`bootstrap status: ${bootstrapStatus.status()} ${await bootstrapStatus.text()}`)
  if (((await bootstrapStatus.json()) as { required: boolean }).required) {
    await page.goto('/setup')
    await page.getByLabel('公司名称').fill('Agentx E2E')
    await page.getByLabel('用户名').fill('admin')
    await page.getByLabel('管理员姓名').fill('E2E Admin')
    await page.getByLabel('密码').fill(password)
    const response = page.waitForResponse((value) => value.url().endsWith('/api/v1/bootstrap') && value.request().method() === 'POST')
    await page.getByRole('button', { name: '初始化并进入工作台' }).click()
    await expect(page).toHaveURL(/\/$/)
    const token = ((await (await response).json()) as { accessToken: string }).accessToken
    const me = await page.request.get('/api/v1/auth/me', { headers: { Authorization: `Bearer ${token}` } })
    expect(me.ok()).toBeTruthy()
    return { token, userId: ((await me.json()) as { id: string }).id }
  }
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
  if (!await page.getByTestId('node-creator').isVisible()) await page.getByTestId('node-creator-trigger').click()
  await expect(page.getByTestId('node-creator')).toBeVisible()
  await page.getByRole('textbox', { name: '搜索节点' }).fill(testId.replace(/^palette-(action|binding)-/, ''))
  await page.getByTestId(testId).click()
  await expect(page.getByTestId('node-creator')).toBeHidden()
}

async function openNodeDetails(page: Page, node: Locator) {
  const details = page.getByTestId('node-details-view')
  if (await details.isVisible()) await details.getByRole('button', { name: /^(关闭|Close)$/ }).first().click()
  await node.dispatchEvent('click')
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
  await page.waitForTimeout(75)
  await page.mouse.up()
  await expect(edges).toHaveCount(edgeCount + 1)
}
type DynamicValue = { kind: string; selector?: { namespace: string; sourceNodeId?: string; port?: string; path: Array<string | number> }; missingPolicy?: { kind: string } }

async function waitApplicationDeployment(page: Page, token: string, applicationId: string, deploymentId: string) {
  await expect.poll(async () => {
    const deployments = await api<ApplicationDeployment[]>(page, token, `/applications/${applicationId}/deployments`)
    const deployment = deployments.find((item) => item.id === deploymentId)
    if (deployment?.status === 'rejected') {
      throw new Error(`application deployment ${deploymentId} was rejected: ${deployment.publishErrorCode ?? ''} ${deployment.publishErrorMessage ?? ''}`)
    }
    return deployment?.status
  }, { timeout: 180_000, intervals: [500, 1_000, 2_000] }).toBe('active')
}

async function ensureStudioResources(page: Page, token: string) {
  const departments = await api<Array<{ id: string; isRoot: boolean }>>(page, token, '/departments')
  const department = departments.find((item) => item.isRoot) ?? departments[0]
  if (!department) throw new Error('M6 Studio requires a Department created through the bootstrap API')

  const credentials = await api<PageResponse<NamedResource>>(page, token, `/credentials?pageSize=100&search=${encodeURIComponent(studioCredentialName)}`)
  const credential = credentials.items.find((item) => item.name === studioCredentialName)
    ?? await mutate<NamedResource>(page, token, '/credentials', 'POST', {
      name: studioCredentialName,
      credentialType: 'bearer',
      secret: 'm5-model-secret',
      ownerDepartmentId: department.id,
    })

  const models = await api<PageResponse<NamedResource>>(page, token, `/models/aliases?pageSize=100&search=${encodeURIComponent(studioModelName)}`)
  if (!models.items.some((item) => item.alias === studioModelName)) {
    await mutate(page, token, '/models/aliases', 'POST', {
      connectionName: 'M6 Studio Fixture Model',
      providerType: 'openai_compatible',
      endpoint: `${echoBaseUrl}/v1`,
      credentialId: credential.id,
      ownerDepartmentId: department.id,
      alias: studioModelName,
      modelName: 'echo-model-v1',
      maxInputTokens: 8192,
      maxOutputTokens: 2048,
      defaultParameters: { temperature: 0 },
      price: { currency: 'USD', inputPerMillion: '5', outputPerMillion: '30' },
    })
  }

  const servers = await api<PageResponse<NamedResource>>(page, token, `/mcp/servers?pageSize=100&search=${encodeURIComponent(studioMcpName)}`)
  const server = servers.items.find((item) => item.name === studioMcpName)
    ?? await mutate<NamedResource>(page, token, '/mcp/servers', 'POST', {
      name: studioMcpName,
      description: 'M6 API-first MCP fixture',
      ownerDepartmentId: department.id,
      transport: 'streamable_http',
      endpoint: `${echoBaseUrl}/mcp`,
      credentialId: credential.id,
      configuration: {},
    })
  const tools = await api<PageResponse<NamedResource>>(page, token, `/mcp/tools?pageSize=100&search=Echo`)
  if (!tools.items.some((item) => item.name === 'echo' && item.serverId === server.id)) {
    await mutate(page, token, `/mcp/servers/${server.id}/discover`, 'POST', {})
  }
  await mutate(page, token, `/mcp/servers/${server.id}`, 'PATCH', {
    name: studioMcpName,
    description: 'M6 API-first MCP fixture',
    transport: 'streamable_http',
    endpoint: `${echoBaseUrl}/mcp`,
    credentialId: credential.id,
    configuration: {},
    status: 'active',
    version: server.version,
  })

  const sandboxes = await api<PageResponse<NamedResource>>(page, token, `/sandbox-profiles?pageSize=100&search=${encodeURIComponent(studioSandboxName)}`)
  if (!sandboxes.items.some((item) => item.name === studioSandboxName)) {
    await mutate(page, token, '/sandbox-profiles', 'POST', {
      name: studioSandboxName,
      description: 'M6 API-first OpenSandbox fixture',
      ownerDepartmentId: department.id,
      runner: 'python',
      imageDigest: 'opensandbox/code-interpreter@sha256:64cd01f03f54ba347d1a1310dcbc18ac5cb17d01714e23b4ea4b840fbb0d6623',
      cpuMillis: 500,
      memoryBytes: 536870912,
      pidsLimit: 256,
      diskBytes: 1073741824,
      timeoutSeconds: 120,
      outputLimitBytes: 1048576,
      networkPolicy: { defaultAction: 'deny', egressMode: 'none' },
    })
  }
}

async function connectIntoOccupiedBoundary(page: Page, source: Locator, sourceHandle: string, target: Locator, targetHandle: string) {
  const edges = page.locator('.react-flow__edge')
  const edgeCount = await edges.count()
  const from = source.locator(`.react-flow__handle.source[data-handleid="${sourceHandle}"]`)
  const to = target.locator(`.react-flow__handle.target[data-handleid="${targetHandle}"]`)
  await expect(from).toBeVisible()
  await expect(to).toBeVisible()
  const [fromBox, toBox] = await Promise.all([from.boundingBox(), to.boundingBox()])
  expect(fromBox).toBeTruthy()
  expect(toBox).toBeTruthy()
  await page.mouse.move(toBox!.x + toBox!.width / 2, toBox!.y + toBox!.height / 2)
  await page.mouse.down()
  await page.mouse.move(fromBox!.x + fromBox!.width / 2, fromBox!.y + fromBox!.height / 2, { steps: 12 })
  await page.waitForTimeout(75)
  await page.mouse.up()
  await expect(edges).toHaveCount(edgeCount + 1)
}

async function hoverEdge(page: Page, edge: Locator, toolbar: Locator) {
  const points = await edge.locator('path[stroke="transparent"]').evaluate((element) => {
    const path = element as SVGPathElement
    const matrix = path.getScreenCTM()
    if (!matrix) return []
    const length = path.getTotalLength()
    return [0.05, 0.1, 0.2, 0.8, 0.9, 0.95].map((ratio) => {
      const point = path.getPointAtLength(length * ratio).matrixTransform(matrix)
      return { x: point.x, y: point.y }
    })
  })
  for (const point of points) {
    await page.mouse.move(point.x, point.y)
    if (await toolbar.evaluate((element) => getComputedStyle(element).opacity === '1')) return
  }
  throw new Error('No unobstructed hover point was found on the connection path')
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
  await page.keyboard.press('Escape')
}

async function fillNativeText(page: Page, scope: Locator, value: string, index = 0) {
  await scope.scrollIntoViewIfNeeded()
  const editor = scope.getByRole('textbox').nth(index)
  await expect(editor).toBeVisible()
  await editor.fill(value)
  if (await editor.getAttribute('contenteditable') === 'true') await expect(editor).toHaveText(value)
  else await expect(editor).toHaveValue(value)
  await page.keyboard.press('Escape')
}

async function chooseReference(page: Page, scope: Locator, namespace: RegExp, labels: string[]) {
  const editor = scope.getByRole('textbox', { name: 'Value' }).first()
  await editor.click()
  const picker = page.getByTestId('reference-picker')
  await expect(picker).toBeVisible()
  await picker.getByRole('button', { name: namespace }).click()
  for (const [index, label] of labels.entries()) {
    const row = picker.getByRole('button', { name: new RegExp(label.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'i') }).first()
    const toggle = row.locator('[data-tree-toggle]')
    if (index < labels.length - 1 && await toggle.count()) await toggle.click()
    else await row.click()
  }
  await expect(picker).toBeHidden()
}

async function setEndOutput(page: Page, nodeKey: string, fieldPath = 'json', port = 'main', required = true, type = 'string', verifyBoundaryEditing = false) {
  await page.getByTestId('workflow-end').click()
  const panel = page.getByTestId('workflow-interface-panel')
  await expect(panel).toBeVisible()
  await panel.getByRole('button', { name: /添加字段|Add field/ }).first().click()
  const dialog = page.getByRole('dialog', { name: /输出字段|Output field/ })
  const outputName = dialog.getByLabel(/输出名称|Output name/)
  await outputName.fill('answer')
  await outputName.blur()
  if (verifyBoundaryEditing) {
    await dialog.getByRole('button', { name: /保存|Save/ }).click()
    await expect(dialog.getByRole('alert')).toContainText(/输出表达式不能为空|expression is required/i)
    await expect(dialog).toBeVisible()
  }
  if (type !== 'string') {
    await dialog.getByLabel(/类型|Type/).click()
    await page.getByRole('option', { name: /对象|Object/ }).click()
  }
  const fields = fieldPath.split('.').filter((field) => field !== 'json')
  await chooseReference(page, dialog, /输出|Outputs/, [nodeKey, port, 'current', ...fields])
  if (verifyBoundaryEditing) {
    const editor = dialog.getByRole('textbox', { name: 'Value' })
    await editor.press('ArrowLeft')
    await editor.pressSequentially('before ')
    await editor.press('End')
    await editor.pressSequentially(' after')
    await expect(editor).toContainText('before')
    await expect(editor).toContainText('after')
    await expect(dialog.locator('[data-agentx-variable]')).toBeVisible()
  }
  if (required) await dialog.getByLabel(/必填|Required/).check()
  await expect(dialog.locator('[data-agentx-variable]')).toBeVisible()
  await dialog.getByRole('button', { name: /保存|Save/ }).click()
  await panel.getByRole('button', { name: /^(关闭|Close)$/ }).click()
}

async function setStartInputs(page: Page) {
  await page.getByTestId('workflow-start').click()
  const panel = page.getByTestId('workflow-interface-panel')
  await expect(panel).toBeVisible()
  await panel.getByRole('button', { name: /添加字段|Add field/ }).first().click()
  let dialog = page.getByRole('dialog', { name: /添加字段|Add field/ })
  const fieldName = dialog.getByLabel(/字段名|Field name/)
  await fieldName.fill('question')
  await fieldName.blur()
  await dialog.getByLabel(/标题|Title/).fill('Question')
  await dialog.getByLabel(/必填|Required/).check()
  await dialog.getByRole('button', { name: /保存|Save/ }).click()
  await panel.getByRole('button', { name: /添加字段|Add field/ }).first().click()
  dialog = page.getByRole('dialog', { name: /添加字段|Add field/ })
  const attachmentsName = dialog.getByLabel(/字段名|Field name/)
  await attachmentsName.fill('attachments')
  await attachmentsName.blur()
  await dialog.getByLabel(/类型|Type/).click()
  await page.getByRole('option', { name: /文件数组|File array/ }).click()
  await dialog.getByLabel(/标题|Title/).fill('Attachments')
  await dialog.getByLabel(/允许的文件类型|Allowed content types/).fill('application/pdf, image/*')
  await dialog.getByLabel(/最大文件大小|Maximum file size/).fill('1048576')
  await dialog.getByLabel(/最少文件数|Minimum files/).fill('0')
  await dialog.getByLabel(/最多文件数|Maximum files/).fill('3')
  await dialog.getByLabel(/文件总大小上限|Maximum total file size/).fill('2097152')
  await dialog.getByRole('button', { name: /保存|Save/ }).click()
  const contextSection = panel.locator('section').filter({ hasText: /全局变量|Global variables/ }).last()
  await contextSection.getByRole('button', { name: /添加全局变量|Add global variable/ }).click()
  const contextDialog = page.getByRole('dialog', { name: /添加全局变量|Add global variable/ })
  const contextName = contextDialog.getByLabel(/全局变量名称|Global variable name/)
  await contextName.fill('session_note')
  await contextName.blur()
  await contextDialog.getByLabel(/默认值|Default value/).fill('initial')
  await contextDialog.getByRole('button', { name: /保存|Save/ }).click()
  await panel.getByRole('button', { name: /^(关闭|Close)$/ }).click()
}

async function configureProjectionAndContextWrite(page: Page, details: Locator) {
  const key = await details.getByLabel(/引用键|Reference key/).inputValue()
  await details.locator('button').filter({ hasText: /自定义输出|custom output/i }).click()
  const outputDialog = page.getByRole('dialog', { name: /添加自定义输出|Add custom output/ })
  const outputName = outputDialog.getByLabel(/输出名称|Output name/)
  await outputName.fill('summary')
  await outputName.blur()
  await chooseReference(page, outputDialog, /当前数据|Current data/, ['当前节点原始输出', 'stdout'])
  await outputDialog.getByRole('button', { name: /保存|Save/ }).click()
  await details.getByRole('button', { name: /添加全局变量写入|Add global variable write/ }).click()
  const writeDialog = page.getByRole('dialog', { name: /添加全局变量写入|Add global variable write/ })
  await writeDialog.getByLabel(/全局变量|Global variable/).click()
  await page.getByRole('option', { name: 'session_note' }).click()
  await chooseReference(page, writeDialog, /输出|Outputs/, [key, 'main', 'current', 'summary'])
  await writeDialog.getByRole('button', { name: /保存|Save/ }).click()
}

async function setEndErrorOutput(page: Page) {
  await page.getByTestId('workflow-end').click()
  const panel = page.getByTestId('workflow-interface-panel')
  await panel.getByLabel(/错误策略|Error strategy/).click()
  await page.getByRole('option', { name: /收集错误|Collect errors/ }).click()
  await panel.getByLabel(/收集窗口|Collect window/).fill('1200')
  const errorFields = panel.locator('section').filter({ hasText: /错误字段|Error fields/ }).last()
  await errorFields.getByRole('button', { name: /添加字段|Add field/ }).click()
  const dialog = page.getByRole('dialog', { name: /输出字段|Output field/ })
  const outputName = dialog.getByLabel(/输出名称|Output name/)
  await outputName.fill('failure_message')
  await outputName.blur()
  await chooseReference(page, dialog, /当前数据|Current data/, ['当前错误', '错误消息'])
  await expect(dialog.locator('[data-agentx-variable]')).toBeVisible()
  await dialog.getByRole('button', { name: /保存|Save/ }).click()
  await panel.getByRole('button', { name: /^(关闭|Close)$/ }).click()
}

async function saveAndReadDraft(page: Page, token: string, workflowId: string) {
  const save = page.getByRole('button', { name: '保存', exact: true })
  if (await save.isEnabled()) {
    const responsePromise = page.waitForResponse((response) => response.url().endsWith(`/api/v1/workflows/${workflowId}/draft`) && response.request().method() === 'PUT')
    await save.click()
    const response = await responsePromise
    if (!response.ok()) throw new Error(`save workflow draft: ${response.status()} ${await response.text()}`)
  }
  await expect(page.locator('header').getByText(/修订号 \d+ · 已保存/)).toBeVisible({ timeout: 30_000 })
  return api<StudioDraft>(page, token, `/workflows/${workflowId}/draft`)
}

async function startDebug(page: Page, action: () => Promise<void>, confirm = true) {
  if (confirm) page.once('dialog', (dialog) => dialog.accept())
  const response = page.waitForResponse((value) => value.url().includes('/debug-executions') && value.request().method() === 'POST')
  await action()
  const parameters = page.getByRole('dialog', { name: /运行工作流|调试输入|Run workflow|Debug input/ })
  const hasParameters = await parameters.waitFor({ state: 'visible', timeout: 2_000 }).then(() => true).catch(() => false)
  if (hasParameters) {
    const question = parameters.getByLabel(/Question|问题/)
    if (await question.isVisible().catch(() => false)) {
      await question.fill('Run the workflow from the Studio.')
    } else {
      const input = parameters.getByRole('textbox', { name: /输入|Input/ })
      if (await input.isVisible().catch(() => false)) await input.fill('{"question":"Run the workflow from the Studio."}')
    }
    await parameters.getByRole('button', { name: /^(开始运行|运行|Run workflow|Run)$/ }).click()
  }
  const accepted = await response
  if (accepted.status() !== 202) {
    throw new Error(`debug execution: ${accepted.status()} ${await accepted.text()}`)
  }
  return (await accepted.json() as { executionId: string }).executionId
}

async function waitExecution(page: Page, token: string, executionId: string, statuses: string[], timeout = 180_000) {
  let current: Execution | undefined
  await expect.poll(async () => {
    try {
      current = await api<Execution>(page, token, `/executions/${executionId}`)
    } catch (error) {
      if (error instanceof Error && error.message.includes(': 404 ')) return false
      throw error
    }
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
  await ensureStudioResources(page, token)
  const workflowName = `M6 Studio ${Date.now()}`
  const workflowId = await createWorkflow(page, workflowName)

  for (const [resourceType, resourceName, detail] of resourceNames) await grantResource(page, resourceType, resourceName, detail, workflowName)

  await page.goto(`/workflows/${workflowId}/editor`)
  await expect(page.getByTestId('workflow-canvas')).toBeVisible()
  await setStartInputs(page)
  await expect(page.getByTestId('node-creator')).toBeHidden()
  await expect(page.getByTestId('node-creator-trigger')).toBeVisible()
  await expect(page.getByTestId('node-creator-rail')).toHaveCount(0)
  await addFromCreator(page, 'palette-action-agent')
  await expect(page.locator('.react-flow__edge')).toHaveCount(0)
  for (const type of ['code', 'approval']) await addFromCreator(page, `palette-action-${type}`)
  for (const type of ['model', 'mcp_tool']) await addFromCreator(page, `palette-binding-${type}`)
  await page.getByRole('button', { name: /^(适应画布|Fit View)$/ }).click({ force: true })

  const agent = flowNode(page, 'agent')
  const code = flowNode(page, 'code')
  const approval = flowNode(page, 'approval')
  const model = page.locator('.react-flow__node-attachment').nth(0)
  const tool = page.locator('.react-flow__node-attachment').nth(1)
  await expect(approval).toBeVisible()

  await openNodeDetails(page, model)
  await choose(page, page.getByTestId('attachment-resource'), new RegExp(studioModelName))
  await openNodeDetails(page, tool)
  await choose(page, page.getByTestId('attachment-resource'), new RegExp(studioMcpName))

  const agentConfigDetails = await openNodeDetails(page, agent)
  await expect(agentConfigDetails.getByText('节点参数', { exact: true })).toHaveCount(0)
  const prompt = agentConfigDetails.getByTestId('parameter-systemPrompt')
  await expect(prompt.locator('[contenteditable="true"]')).toHaveCount(1)
  await fillNativeText(page, prompt, 'Use the attached resources and return a concise result.')
  const question = agentConfigDetails.getByTestId('parameter-userQuestion')
  await expect(question.locator('[contenteditable="true"]')).toHaveCount(1)
  await chooseReference(page, question, /输入|Inputs/, ['question'])
  await expect(agentConfigDetails.getByText('高级配置', { exact: true })).toBeVisible()
  await expect(agentConfigDetails.getByTestId('parameter-maxDurationMs')).toContainText('毫秒')
  await expect(agentConfigDetails.getByTestId('parameter-maxTotalTokens')).toContainText('Token')

  const configuredCodeDetails = await openNodeDetails(page, code)
  const codeName = configuredCodeDetails.locator('[data-field-path="name"] input')
  await codeName.fill('M6 Python Code')
  await codeName.blur()
  await expect(code).toContainText('M6 Python Code')
  await choose(page, page.getByTestId('parameter-runner'), /^Python$/)
  await fillMonaco(page, page.getByTestId('parameter-source'), 'print("m6-studio-ok")')
  await choose(page, page.getByTestId('resource-selector-sandbox_profile'), new RegExp(studioSandboxName))
  await configureProjectionAndContextWrite(page, configuredCodeDetails)

  await openNodeDetails(page, approval)
  await page.getByTestId('parameter-title').getByRole('textbox').fill('M6 Studio Approval')
  await page.getByTestId('parameter-description').getByRole('textbox').fill('Approve the Workflow created through the Studio UI.')
  await page.getByTestId('parameter-candidateUserId').getByRole('textbox').fill(userId)

  await page.getByTestId('node-details-view').getByRole('button', { name: /^(关闭|Close)$/ }).first().click()
  await page.getByRole('button', { name: /^(适应画布|Fit View)$/ }).click({ force: true })
  await connect(page, page.getByTestId('workflow-start'), 'main', agent, 'main')
  await agent.locator('.react-flow__handle.source[data-handleid="main"]').hover()
  await expect(page.getByTestId('workflow-canvas')).toHaveAttribute('data-connection-state', 'source-hover')
  await page.getByRole('button', { name: /^(适应画布|Fit View)$/ }).hover()
  await expect(page.getByTestId('workflow-canvas')).toHaveAttribute('data-connection-state', 'idle')
  await connect(page, agent, 'main', code, 'main')
  await connect(page, code, 'main', approval, 'main')
  await connect(page, model, 'resource', agent, 'binding:ai_model')
  await connect(page, tool, 'resource', agent, 'binding:ai_tool')

  await page.getByTestId('node-creator-trigger').click()
  await expect(page.getByTestId('node-creator')).toBeVisible()
  await page.getByRole('button', { name: /流程控制|Flow control/ }).click()
  await expect(page.getByTestId('palette-action-error_handler')).toBeVisible()
  await page.getByTestId('palette-action-error_handler').click()
  await expect(page.getByTestId('node-creator')).toBeHidden()
  const errorHandler = flowNode(page, 'error_handler')
  await expect(errorHandler).toBeVisible()
  await expect(page.getByTestId('node-details-view')).toBeVisible()

  await page.getByTestId('node-details-view').getByRole('button', { name: /^(关闭|Close)$/ }).first().click()
  await page.getByRole('button', { name: /^(适应画布|Fit View)$/ }).click({ force: true })
  await connect(page, code, 'error', errorHandler, 'error')
  await connect(page, code, 'error', page.getByTestId('workflow-end'), 'error')
  await connect(page, approval, 'approved', page.getByTestId('workflow-end'), 'main')
  await connectIntoOccupiedBoundary(page, errorHandler, 'recovered', page.getByTestId('workflow-end'), 'main')

  const approvalKey = (await saveAndReadDraft(page, token, workflowId)).definition.nodes.find((node) => node.type === 'approval')!.key
  await setEndOutput(page, approvalKey, 'json.decision', 'approved', false, 'string', true)
  await setEndErrorOutput(page)

  await openNodeDetails(page, code)
  await page.getByRole('button', { name: '关闭' }).click()

  await page.getByTestId('node-creator-trigger').click()
  await page.getByTestId('node-creator').getByRole('button', { name: '便签' }).click()
  const note = page.locator('[data-testid^="studio-note-"]').first()
  await expect(note).toBeVisible()
  await note.dispatchEvent('dblclick')
  await note.getByRole('textbox').fill('M6 runtime notes')
  await note.getByRole('textbox').press('Control+Enter')
  await note.getByRole('button', { name: /便签颜色：blue/ }).click()

  await code.click()
  await page.keyboard.down('Shift')
  await approval.click({ force: true })
  await page.keyboard.up('Shift')
  await page.getByTestId('node-creator-trigger').click()
  await page.getByTestId('node-creator').getByRole('button', { name: '分组' }).click()
  const group = page.locator('[data-testid^="studio-group-"]').first()
  await expect(group).toBeVisible()
  await group.getByRole('button', { name: '折叠分组' }).click()
  await expect(group.getByRole('button', { name: '展开分组' })).toBeVisible()
  await group.getByRole('button', { name: '展开分组' }).click()

  const draft = await saveAndReadDraft(page, token, workflowId)
  expect(draft.definition.schemaVersion).toBe('5.0')
  expect(draft.definition.nodes.map((node) => node.type)).toEqual(expect.arrayContaining(['agent', 'code', 'approval', 'error_handler']))
  expect(draft.definition.nodes.map((node) => node.type)).not.toContain('manual_trigger')
  expect(draft.definition.nodes.find((node) => node.type === 'code')?.name).toBe('M6 Python Code')
  expect(draft.definition.nodes.find((node) => node.type === 'agent')?.parameters.userQuestion).toMatchObject({ kind: 'reference', selector: { namespace: 'inputs', path: ['question'] } })
  expect(draft.definition.nodes.find((node) => node.type === 'agent')?.resourceReferences.map((item) => item.bindingRole)).toEqual(expect.arrayContaining(['ai_model', 'ai_tool']))
  expect(draft.definition.nodes.find((node) => node.type === 'code')?.settings.onError).toBe('continue_error_output')
  expect(draft.definition.nodes.find((node) => node.type === 'code')?.outputProjection.main.summary.value).toMatchObject({ kind: 'reference', selector: { namespace: 'item', path: ['stdout'] } })
  expect(draft.definition.nodes.find((node) => node.type === 'code')?.contextWrites).toContainEqual({ operation: 'set', path: 'session_note', value: expect.objectContaining({ kind: 'reference', selector: expect.objectContaining({ namespace: 'outputs', port: 'main', path: ['summary'] }) }) })
  expect(draft.definition.start.contexts).toMatchObject({ session_note: { default: 'initial', mutable: true } })
  const startInputs = draft.definition.start.inputs as { properties: Record<string, Record<string, unknown>> }
  expect(startInputs.properties.attachments).toMatchObject({
    type: 'array',
    minItems: 0,
    maxItems: 3,
    'x-agentx-artifact': true,
    'x-agentx-artifact-array': true,
    'x-agentx-content-types': ['application/pdf', 'image/*'],
    'x-agentx-max-size-bytes': 1048576,
    'x-agentx-max-total-size-bytes': 2097152,
  })
  expect(draft.definition.end.outputs.answer.value).toMatchObject({
    kind: 'template',
    segments: [
      { kind: 'text', text: 'before ' },
      { kind: 'reference', selector: { namespace: 'outputs', port: 'approved', path: ['decision'] } },
      { kind: 'text', text: ' after' },
    ],
  })
  expect(draft.definition.end.error).toMatchObject({ strategy: 'collect', collectWindowMs: 1200, outputs: { failure_message: { value: { kind: 'reference', selector: { namespace: 'item', path: ['message'] } } } } })
  expect(draft.definition.connections).toContainEqual(expect.objectContaining({ sourceNodeId: draft.definition.nodes.find((node) => node.type === 'code')!.id, sourceHandle: 'error', targetNodeId: '__end__', targetHandle: 'error' }))
  expect(draft.editorDocument.bindingEdges).toHaveLength(2)

  const firstExecution = await startDebug(page, () => studioRun(page).click())
  const approvalPage = await context.newPage()
  await approveExecution(approvalPage, token, firstExecution)
  await waitExecution(page, token, firstExecution, ['succeeded'])

  const details = await openNodeDetails(page, code)
  await details.getByRole('tab', { name: '输出' }).click()
  await expect(details.getByRole('textbox', { name: '输出' })).toHaveValue(/m6-studio-ok/, { timeout: 30_000 })
  await details.getByRole('tab', { name: 'Trace' }).click()
  await expect(details.getByRole('heading', { name: '输入与解析参数' })).toBeVisible({ timeout: 30_000 })
  await expect(details.getByRole('heading', { name: '语义输出' })).toBeVisible()
  await expect(details).toContainText('m6-studio-ok')
  const runtimeRail = page.getByTestId('runtime-rail')
  type ExecutionEvent = { sequence: number; eventType: string; status: string }
  let executionEvents: { items: ExecutionEvent[]; nextCursor?: number | null } | undefined
  await expect.poll(async () => {
    executionEvents = await api<{ items: ExecutionEvent[]; nextCursor?: number | null }>(page, token, `/executions/${firstExecution}/events?after=0&limit=200`)
    return executionEvents.items.at(-1)?.eventType
  }, { timeout: 30_000, intervals: [250, 500, 1_000] }).toBe('execution.succeeded')
  expect(executionEvents!.items.map((event) => event.sequence)).toEqual(executionEvents!.items.map((event) => event.sequence).sort((left, right) => left - right))
  expect(new Set(executionEvents!.items.map((event) => event.sequence)).size).toBe(executionEvents!.items.length)
  const incrementalAfter = executionEvents!.items[Math.min(3, executionEvents!.items.length - 2)].sequence
  const incrementalEvents = await api<{ items: ExecutionEvent[]; nextCursor?: number | null }>(page, token, `/executions/${firstExecution}/events?after=${incrementalAfter}&limit=2`)
  expect(incrementalEvents.items).toHaveLength(2)
  expect(incrementalEvents.items.every((event) => event.sequence > incrementalAfter)).toBe(true)
  expect(incrementalEvents.nextCursor).toBe(incrementalEvents.items.at(-1)?.sequence)
  await runtimeRail.getByRole('tab', { name: '事件', exact: true }).click()
  const executionEventsPanel = runtimeRail.getByTestId('execution-events')
  await expect(executionEventsPanel).toContainText('execution.succeeded')
  await expect(executionEventsPanel).not.toContainText('未知（reserved）')
  expect(await executionEventsPanel.evaluate((panel) => getComputedStyle(panel).overflowY)).toMatch(/auto|scroll/)
  await runtimeRail.getByRole('tab', { name: 'Trace' }).click()
  await expect(runtimeRail).toHaveCSS('height', '560px')
  const resizeHandle = runtimeRail.getByRole('button', { name: '调整运行面板高度' })
  const [runtimeRailBoxForHandle, resizeHandleBox] = await Promise.all([runtimeRail.boundingBox(), resizeHandle.boundingBox()])
  expect(runtimeRailBoxForHandle).toBeTruthy()
  expect(resizeHandleBox).toBeTruthy()
  expect(resizeHandleBox!.y).toBeLessThan(runtimeRailBoxForHandle!.y)
  expect(await resizeHandle.evaluate((handle) => {
    const box = handle.getBoundingClientRect()
    const hit = document.elementFromPoint(box.x + box.width / 2, box.y + 1)
    return hit === handle || handle.contains(hit)
  })).toBe(true)
  const executionToolbar = runtimeRail.getByTestId('execution-toolbar')
  await expect(executionToolbar).toBeVisible()
  await expect(executionToolbar).toContainText('执行记录')
  await expect(executionToolbar).toContainText('不会筛选 Trace 状态')
  await expect(executionToolbar).toHaveCSS('border-top-width', '1px')
  await expect(executionToolbar).toHaveCSS('border-bottom-width', '1px')
  const executionSelector = executionToolbar.getByRole('combobox', { name: '切换执行' })
  await expect(executionSelector).toContainText(/成功|运行中|失败/)
  await expect(executionSelector).not.toContainText('—')
  const traceNodeView = runtimeRail.getByTestId('trace-node-view')
  await expect(traceNodeView).toBeVisible({ timeout: 30_000 })
  expect(await traceNodeView.evaluate((view) => {
    view.scrollTop = view.scrollHeight
    return getComputedStyle(view).overflowY === 'auto' && Math.ceil(view.scrollTop + view.clientHeight) >= view.scrollHeight
  })).toBe(true)
  await expect(runtimeRail.getByTestId('trace-start-boundary')).toBeVisible()
  await expect(runtimeRail.getByTestId('trace-end-boundary')).toBeVisible()
  await runtimeRail.getByRole('tab', { name: '高级瀑布' }).click()
  await expect(runtimeRail.getByRole('treegrid', { name: 'Trace 层级瀑布' })).toBeVisible({ timeout: 30_000 })
  type TraceSpan = { spanId: string; spanKind: string; spanName: string; status: string; inputTokens?: number; outputTokens?: number; costMicros: number; resourceType?: string }
  let traceSnapshot: { complete: boolean; spans: TraceSpan[] } | undefined
  await expect.poll(async () => {
    traceSnapshot = await api<{ complete: boolean; spans: TraceSpan[] }>(page, token, `/executions/${firstExecution}/trace?limit=200`)
    const kinds = new Set(traceSnapshot.spans.map((span) => span.spanKind))
    return ['execution', 'boundary', 'node', 'attempt', 'agent_run', 'agent_iteration', 'runtime_call', 'sandbox', 'wait'].filter((kind) => !kinds.has(kind))
  }, { timeout: 60_000, intervals: [500, 1_000, 2_000] }).toEqual([])
  expect(traceSnapshot?.complete).toBe(true)
  const modelCall = traceSnapshot?.spans.find((span) => span.spanKind === 'runtime_call' && span.spanName === 'Model call')
  const mcpCall = traceSnapshot?.spans.find((span) => span.spanKind === 'runtime_call' && span.spanName === 'MCP tool call')
  expect(modelCall).toMatchObject({ status: 'succeeded', resourceType: 'model' })
  expect(modelCall?.costMicros ?? 0).toBeGreaterThan(0)
  expect((modelCall?.inputTokens ?? 0) + (modelCall?.outputTokens ?? 0)).toBeGreaterThan(0)
  let runtimeDetails: { calls: Array<{ resourceType?: string; costMicros: number; costCurrency?: string }> } | undefined
  await expect.poll(async () => {
    try {
      const details = await api<{ calls: Array<{ resourceType?: string; costMicros: number; costCurrency?: string }> }>(page, token, `/executions/${firstExecution}/runtime-details`)
      runtimeDetails = details
      return details.calls.some((call) => call.resourceType === 'model')
    } catch {
      return false
    }
  }, { timeout: 30_000, intervals: [500, 1_000, 2_000] }).toBeTruthy()
  const runtimeModelCall = runtimeDetails!.calls.find((call) => call.resourceType === 'model')
  expect(runtimeModelCall).toMatchObject({ costCurrency: 'USD' })
  expect(runtimeModelCall?.costMicros).toBe(modelCall?.costMicros)
  expect(mcpCall).toMatchObject({ status: 'succeeded', resourceType: 'mcp' })
  expect(traceSnapshot?.spans.find((span) => span.spanKind === 'sandbox')).toMatchObject({ status: 'succeeded' })
  expect(traceSnapshot?.spans.find((span) => span.spanKind === 'wait')).toMatchObject({ status: expect.stringMatching(/approved|succeeded/) })
  const codeTraceRow = runtimeRail.getByRole('row').filter({ hasText: 'M6 Python Code' }).first()
  await expect(codeTraceRow).toBeVisible()
  await codeTraceRow.click()
  await page.screenshot({ path: testInfo.outputPath('trace-waterfall-skywalking.png'), fullPage: true })
  const waterfall = runtimeRail.getByTestId('trace-waterfall')
  await waterfall.getByRole('tab', { name: '原始数据' }).click()
  const rawTracePanel = waterfall.getByRole('tabpanel')
  await expect(rawTracePanel).toBeVisible()
  const rawTraceBox = await rawTracePanel.boundingBox()
  const runtimeRailBox = await runtimeRail.boundingBox()
  expect(rawTraceBox!.y + rawTraceBox!.height).toBeLessThanOrEqual(runtimeRailBox!.y + runtimeRailBox!.height + 1)
  expect(await rawTracePanel.evaluate((panel) => {
    panel.scrollTop = panel.scrollHeight
    return Math.ceil(panel.scrollTop + panel.clientHeight) >= panel.scrollHeight
  })).toBe(true)
  await waterfall.getByRole('tab', { name: '事件' }).click()
  await expect(waterfall.getByRole('tabpanel')).toContainText(/node\.(finished|completed)/)
  await runtimeRail.getByRole('button', { name: '折叠执行轨道' }).click()
  await expect(runtimeRail.getByRole('button', { name: '展开执行轨道' })).toBeVisible()
  await runtimeRail.getByRole('button', { name: '展开执行轨道' }).click()

  const executionPage = await context.newPage()
  await executionPage.goto(`/executions/${firstExecution}`)
  const executionTraceTab = executionPage.getByRole('tab', { name: 'Trace', exact: true })
  await expect(executionTraceTab).toHaveAttribute('aria-selected', 'true')
  await expect(executionPage.getByTestId('trace-node-view')).toBeVisible({ timeout: 30_000 })
  await executionPage.getByRole('tab', { name: '高级瀑布' }).click()
  await expect(executionPage.getByRole('treegrid', { name: 'Trace 层级瀑布' })).toBeVisible({ timeout: 30_000 })
  await executionPage.getByRole('row').filter({ hasText: 'M6 Python Code' }).first().click()
  await expect(executionPage.getByTestId('trace-detail').getByRole('tab')).toHaveCount(5)
  await executionPage.getByRole('tab', { name: '恢复', exact: true }).click()
  await expect(executionPage.getByRole('complementary', { name: '执行节点大纲' })).toHaveCount(0)
  await expect(executionPage.getByText(/检查点|Checkpoint/).first()).toBeVisible()
  await executionPage.close()

  const stableAgent = page.getByTestId(`rf__node-${draft.definition.nodes.find((node) => node.type === 'agent')!.id}`)
  const agentDetails = await openNodeDetails(page, stableAgent)
  await agentDetails.getByRole('tab', { name: '输出' }).click()
  await agentDetails.getByRole('textbox', { name: '输出' }).fill(JSON.stringify({ main: [{ json: { text: 'incomplete-agent-output' } }] }))
  const invalidMockResponse = page.waitForResponse((value) => value.url().includes('/debug-overlays/') && value.request().method() === 'PUT')
  await agentDetails.getByRole('button', { name: '模拟', exact: true }).click()
  expect((await invalidMockResponse).ok()).toBeTruthy()
  const contractFailureId = await startDebug(page, () => studioRun(page).click())
  const contractFailure = await waitExecution(page, token, contractFailureId, ['failed'], 30_000)
  expect(contractFailure.errorCode).toBe('NODE_OUTPUT_SCHEMA_VALIDATION_FAILED')
  const failedNodes = await api<{ items: Array<{ nodeId: string; status: string; errorCode?: string | null }> }>(page, token, `/executions/${contractFailureId}/nodes`)
  expect(failedNodes.items.filter((node) => node.nodeId === draft.definition.nodes.find((item) => item.type === 'agent')!.id)).toEqual([
    expect.objectContaining({ status: 'failed', errorCode: 'NODE_OUTPUT_SCHEMA_VALIDATION_FAILED' }),
  ])

  const restoredAgentDetails = await openNodeDetails(page, stableAgent)
  await restoredAgentDetails.getByRole('tab', { name: '输出' }).click()
  await restoredAgentDetails.getByRole('textbox', { name: '输出' }).fill(JSON.stringify({
    main: [{
      json: {
        text: 'm6-mock-output',
        reasoningContent: null,
        structuredOutput: null,
        files: [],
        citations: [],
        usage: { inputTokens: 0, outputTokens: 0, totalTokens: 0, costMicros: 0 },
        finishReason: 'stop', partial: false,
      },
    }],
  }))
  const mockResponse = page.waitForResponse((value) => value.url().includes('/debug-overlays/') && value.request().method() === 'PUT')
  await restoredAgentDetails.getByRole('button', { name: '模拟', exact: true }).click()
  expect((await mockResponse).ok()).toBeTruthy()

  const failedCodeDetails = await openNodeDetails(page, code)
  await failedCodeDetails.getByRole('tab', { name: '输出' }).click()
  await expect(failedCodeDetails.getByRole('button', { name: '固定', exact: true })).toBeDisabled()

  const restoredExecution = await startDebug(page, () => studioRun(page).click())
  await approveExecution(approvalPage, token, restoredExecution)
  await waitExecution(page, token, restoredExecution, ['succeeded'])

  const codeDetails = await openNodeDetails(page, code)
  await codeDetails.getByRole('tab', { name: '输出' }).click()
  await expect(codeDetails.getByRole('textbox', { name: '输出' })).toHaveValue(/m6-studio-ok/, { timeout: 30_000 })
  await expect(codeDetails.getByRole('button', { name: '固定', exact: true })).toBeEnabled()
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
  await expect(runtimeRail.getByRole('tab', { name: '事件', exact: true }).first()).toBeVisible()

  await runtimeRail.getByRole('tab', { name: 'Trace' }).click()
  const failedTraceRequest = page.waitForResponse((response) => response.url().includes(`/api/v1/executions/${contractFailureId}/trace?`) && response.request().method() === 'GET')
  await executionSelector.click()
  await page.locator(`[role="option"][data-option-value="${contractFailureId}"]`).click()
  await failedTraceRequest
  await expect(executionSelector).toContainText('失败')
  await expect(runtimeRail.getByTestId('trace-node-view')).toBeVisible()
  await expect(runtimeRail.getByTestId('trace-node-view')).toContainText('NODE_OUTPUT_SCHEMA_VALIDATION_FAILED')
  await runtimeRail.getByRole('tab', { name: '高级瀑布' }).click()
  await expect(runtimeRail.getByRole('treegrid', { name: 'Trace 层级瀑布' })).toBeVisible()

  await openNodeDetails(page, code)
  await selectDebugMode(page, 'single_node')
  const singleExecution = await startDebug(page, () => studioRun(page).click())
  await waitExecution(page, token, singleExecution, ['succeeded'])

  await selectDebugMode(page, 'to_node')
  const toExecution = await startDebug(page, () => studioRun(page).click())
  await waitExecution(page, token, toExecution, ['succeeded'])

  await selectDebugMode(page, 'from_node')
  const fromExecution = await startDebug(page, () => studioRun(page).click())
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
  await waitExecution(page, token, forkExecution, ['waiting', 'waiting_approval'])
  await approveExecution(approvalPage, token, forkExecution)
  await waitExecution(page, token, forkExecution, ['succeeded'])

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
    await runtimeRail.getByRole('tab', { name: 'Trace' }).click()
    await runtimeRail.getByRole('tab', { name: locale === 'zh-CN' ? '高级瀑布' : 'Advanced waterfall' }).click()
    for (const theme of ['light', 'dark'] as const) {
      await setTheme(page, theme)
      for (const viewport of viewports) {
        await page.setViewportSize(viewport)
        await expect(page.getByTestId('workflow-canvas')).toBeVisible()
        await expect(runtimeRail.getByTestId('trace-waterfall')).toBeVisible()
        await expect(runtimeRail.getByRole('treegrid', { name: locale === 'zh-CN' ? 'Trace 层级瀑布' : 'Trace hierarchy waterfall' })).toBeVisible()
        await expect(runtimeRail.getByTestId('trace-detail')).toBeVisible()
        await expect(page.locator('body')).not.toContainText(/studio\.[A-Za-z]/)
        await expect(page.getByText('Invalid Date', { exact: true })).toHaveCount(0)
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBeTruthy()
        await page.screenshot({ path: testInfo.outputPath(`studio-${viewport.width}x${viewport.height}-${locale}-${theme}.png`), fullPage: true })
      }
    }
    const versionsEndpoint = `/api/v1/workflows/${workflowId}/versions`
    await page.route(versionsEndpoint, (route) => route.fulfill({
      status: 422,
      contentType: 'application/json',
      body: JSON.stringify({ code: 'INVALID_WORKFLOW_DEFINITION', message: locale === 'zh-CN' ? 'Workflow definition is invalid' : '工作流定义无效', requestId: crypto.randomUUID() }),
    }))
    await page.getByRole('button', { name: locale === 'zh-CN' ? '版本' : 'Version', exact: true }).click()
    const localizedVersionDialog = page.getByRole('dialog', { name: locale === 'zh-CN' ? '工作流版本' : 'Workflow versions' })
    await localizedVersionDialog.getByRole('button', { name: locale === 'zh-CN' ? '创建版本' : 'Create version' }).click()
    await expect(page.getByText(locale === 'zh-CN' ? '工作流定义无效' : 'The workflow definition is invalid', { exact: true })).toBeVisible()
    await localizedVersionDialog.getByRole('button', { name: locale === 'zh-CN' ? '关闭' : 'Close' }).click()
    await page.unroute(versionsEndpoint)
  }

  const concurrent = await context.newPage()
  await concurrent.goto(`/workflows/${workflowId}/editor`)
  await expect(concurrent.getByTestId('workflow-canvas')).toBeVisible()
  const concurrentDetails = await openNodeDetails(concurrent, flowNode(concurrent, 'code'))
  await concurrentDetails.getByRole('tab', { name: 'Parameters' }).click()
  await concurrentDetails.getByRole('textbox', { name: 'Name', exact: true }).fill('code concurrent')
  await concurrent.getByRole('button', { name: 'Save', exact: true }).click()
  await expect(concurrent.locator('header').getByText(/Revision \d+ · Saved/)).toBeVisible()
  const localDetails = await openNodeDetails(page, code)
  await localDetails.getByRole('tab', { name: 'Parameters' }).click()
  await localDetails.getByRole('textbox', { name: 'Name', exact: true }).fill('code local')
  await page.getByRole('button', { name: 'Save', exact: true }).click()
  await expect(page.getByRole('dialog', { name: 'Draft revision conflict' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Keep local copy' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Load server revision' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Overwrite server draft' })).toBeVisible()

  await concurrent.close()
  await approvalPage.close()
})

test('M6 standalone Model sends prompt and question as ordered messages and exposes text', async ({ page }) => {
  const { token } = await login(page)
  await ensureStudioResources(page, token)
  const workflowName = `M6 Model Contract ${Date.now()}`
  const workflowId = await createWorkflow(page, workflowName)
  await grantResource(page, 'credential', studioCredentialName, undefined, workflowName)
  await grantResource(page, 'model', studioModelName, undefined, workflowName)

  await page.goto(`/workflows/${workflowId}/editor`)
  await expect(page.getByTestId('workflow-canvas')).toBeVisible()
  await setStartInputs(page)
  await addFromCreator(page, 'palette-action-model')
  await page.getByRole('button', { name: /^(适应画布|Fit View)$/ }).click({ force: true })
  const model = page.locator('.react-flow__node-manifest').filter({ hasText: /模型|Model/ }).first()
  const details = await openNodeDetails(page, model)
  await choose(page, details.getByTestId('resource-selector-model'), new RegExp(studioModelName))
  await fillNativeText(page, details.getByTestId('parameter-prompt'), '你叫 kakj\nTRACE_LARGE_RESPONSE')
  await chooseReference(page, details.getByTestId('parameter-userQuestion'), /输入|Inputs/, ['question'])
  const modelKey = await details.getByLabel(/引用键|Reference key/).inputValue()
  await details.getByRole('button', { name: /^(关闭|Close)$/ }).first().click()

  await connect(page, page.getByTestId('workflow-start'), 'main', model, 'main')
  await connect(page, model, 'main', page.getByTestId('workflow-end'), 'main')
  await setEndOutput(page, modelKey, 'json.text')
  const draft = await saveAndReadDraft(page, token, workflowId)
  const modelDefinition = draft.definition.nodes.find((node) => node.type === 'model')
  expect(modelDefinition?.parameters).toMatchObject({
    prompt: { kind: 'literal', value: '你叫 kakj\nTRACE_LARGE_RESPONSE' },
    userQuestion: expect.objectContaining({ kind: 'reference', selector: expect.objectContaining({ namespace: 'inputs', path: ['question'] }) }),
  })
  expect(draft.definition.end.outputs.answer.value).toMatchObject({
    kind: 'reference',
    selector: expect.objectContaining({ sourceNodeId: modelDefinition?.id, path: ['text'] }),
  })

  const executionId = await startDebug(page, () => studioRun(page).click())
  await waitExecution(page, token, executionId, ['succeeded'])
  let modelSpan: { spanId: string } | undefined
  await expect.poll(async () => {
    const trace = await api<{ spans: Array<{ spanId: string; spanKind: string; spanName: string }> }>(page, token, `/executions/${executionId}/trace?limit=100`)
    modelSpan = trace.spans.find((span) => span.spanKind === 'runtime_call' && span.spanName === 'Model call')
    return modelSpan?.spanId
  }, { timeout: 60_000, intervals: [500, 1_000, 2_000] }).toBeTruthy()
  const nodeRuns = await api<{ items: Array<{ nodeId: string; costMicros: number; costCurrency?: string; startedAt?: string; endedAt?: string; output?: { main?: Array<{ json?: Record<string, unknown> }> } }> }>(page, token, `/executions/${executionId}/nodes`)
  const modelRun = nodeRuns.items.find((node) => node.nodeId === modelDefinition?.id)
  const modelOutput = modelRun?.output?.main?.[0].json
  expect(modelOutput).toMatchObject({ text: expect.any(String), reasoningContent: null, structuredOutput: null, files: [], citations: [], partial: false })
  expect(modelRun).toMatchObject({ costMicros: 390, costCurrency: 'USD', startedAt: expect.any(String), endedAt: expect.any(String) })
  expect(modelOutput).not.toHaveProperty('message')
  expect(modelOutput).not.toHaveProperty('messages')
  expect(modelOutput).not.toHaveProperty('toolCalls')
  expect(modelOutput).not.toHaveProperty('providerRawResponse')

  const traceDetail = await api<{
    contents: Array<{ kind: string; preview?: { messages?: Array<{ role: string; content: string }>; choices?: unknown[] }; contentRef?: string | null }>
  }>(page, token, `/executions/${executionId}/trace/spans/${modelSpan!.spanId}`)
  const requestContent = traceDetail.contents.find((content) => content.kind === 'runtime_request')
  const responseContent = traceDetail.contents.find((content) => content.kind === 'runtime_response')
  expect(requestContent?.preview?.messages).toEqual([
    { role: 'system', content: '你叫 kakj\nTRACE_LARGE_RESPONSE' },
    { role: 'user', content: 'Run the workflow from the Studio.' },
  ])
  expect(responseContent?.preview).toBeNull()
  expect(responseContent?.contentRef).toMatch(/^[0-9a-f-]{36}$/)
  const traceJson = JSON.stringify(traceDetail)
  expect(traceJson).not.toContain('m5-model-secret')
  expect(traceJson.toLowerCase()).not.toContain('authorization')

  type ModelExecutionEvent = { sequence: number; eventType: string; status: string }
  let modelEvents: { items: ModelExecutionEvent[]; nextCursor?: number | null } | undefined
  await expect.poll(async () => {
    modelEvents = await api<{ items: ModelExecutionEvent[]; nextCursor?: number | null }>(page, token, `/executions/${executionId}/events?after=0&limit=200`)
    return modelEvents.items.at(-1)?.eventType
  }, { timeout: 30_000, intervals: [250, 500, 1_000] }).toBe('execution.succeeded')
  expect(modelEvents!.items.map((event) => event.sequence)).toEqual(modelEvents!.items.map((event) => event.sequence).sort((left, right) => left - right))
  const modelIncrementalEvents = await api<{ items: ModelExecutionEvent[]; nextCursor?: number | null }>(page, token, `/executions/${executionId}/events?after=4&limit=2`)
  expect(modelIncrementalEvents.items).toHaveLength(2)
  expect(modelIncrementalEvents.items.every((event) => event.sequence > 4)).toBe(true)
  expect(modelIncrementalEvents.nextCursor).toBe(modelIncrementalEvents.items.at(-1)?.sequence)

  const modelRuntimeRail = page.getByTestId('runtime-rail')
  await modelRuntimeRail.getByRole('tab', { name: '事件', exact: true }).click()
  await expect(modelRuntimeRail.getByTestId('execution-events')).toContainText('execution.succeeded')
  await expect(modelRuntimeRail.getByTestId('execution-events')).not.toContainText('未知（reserved）')
  await modelRuntimeRail.getByRole('tab', { name: 'Trace' }).click()
  await expect(modelRuntimeRail).toHaveCSS('height', '560px')
  await expect(modelRuntimeRail.getByTestId('trace-final-output')).toContainText(/kakj/i)
  await expect(modelRuntimeRail.getByTestId('trace-node-view')).toContainText(/text|kakj/i)
  const modelNodeCard = modelRuntimeRail.getByTestId('trace-node-card').filter({ hasText: /模型|Model/ }).first()
  await expect(modelNodeCard).toContainText(/\$0\.000390/)
  expect(await modelNodeCard.locator('button').first().getAttribute('class')).toContain('grid-cols-[minmax(160px,1fr)_auto_auto]')
  await modelRuntimeRail.getByRole('tab', { name: '高级瀑布' }).click()
  const modelWaterfall = modelRuntimeRail.getByTestId('trace-waterfall')
  const modelCallRow = modelWaterfall.getByRole('row').filter({ hasText: 'Model call' }).first()
  await expect(modelCallRow).toBeVisible({ timeout: 30_000 })
  await modelCallRow.click()
  await modelWaterfall.getByRole('tab', { name: 'Provider / Runtime 响应' }).click()
  const artifactDownload = page.waitForEvent('download')
  await modelWaterfall.getByRole('button', { name: /Artifact/ }).click()
  const artifact = await artifactDownload
  expect(artifact.suggestedFilename()).toBe(`${responseContent!.contentRef}.json`)
  const artifactPath = await artifact.path()
  expect(artifactPath).toBeTruthy()
  const artifactText = await readFile(artifactPath!, 'utf8')
  expect(artifactText.length).toBeGreaterThan(16 * 1024)
  expect((JSON.parse(artifactText) as { choices?: unknown[] }).choices).toHaveLength(1)
  expect(artifactText).toContain('trace-artifact-marker')
  expect(artifactText).not.toContain('m5-model-secret')
  expect(artifactText.toLowerCase()).not.toContain('authorization')
  await modelWaterfall.getByRole('tab', { name: '原始数据' }).click()
  const modelRawPanel = modelWaterfall.getByRole('tabpanel')
  const modelRawBox = await modelRawPanel.boundingBox()
  const modelRailBox = await modelRuntimeRail.boundingBox()
  expect(modelRawBox!.y + modelRawBox!.height).toBeLessThanOrEqual(modelRailBox!.y + modelRailBox!.height + 1)
  expect(await modelRawPanel.evaluate((panel) => {
    panel.scrollTop = panel.scrollHeight
    return Math.ceil(panel.scrollTop + panel.clientHeight) >= panel.scrollHeight
  })).toBe(true)
})

test('M6 standalone MCP consumes configured arguments and returns semantic fields', async ({ page }) => {
  const { token } = await login(page)
  await ensureStudioResources(page, token)
  const workflowName = `M6 MCP Contract ${Date.now()}`
  const workflowId = await createWorkflow(page, workflowName)
  await grantResource(page, 'credential', studioCredentialName, undefined, workflowName)
  await grantResource(page, 'mcp_server', studioMcpName, undefined, workflowName)
  await grantResource(page, 'mcp_tool', 'Echo', studioMcpName, workflowName)

  await page.goto(`/workflows/${workflowId}/editor`)
  await expect(page.getByTestId('workflow-canvas')).toBeVisible()
  await setStartInputs(page)
  await addFromCreator(page, 'palette-action-mcp_tool')
  await page.getByRole('button', { name: /^(适应画布|Fit View)$/ }).click({ force: true })
  const mcp = page.locator('.react-flow__node-manifest').filter({ hasText: /工具|Tool/ }).first()
  const details = await openNodeDetails(page, mcp)
  await choose(
    page,
    details.getByTestId('resource-selector-mcp_tool'),
    new RegExp(`echo.*${studioMcpName}`, 'i'),
  )
  const argumentsField = details.getByTestId('parameter-arguments')
  await argumentsField.getByRole('button', { name: /添加字段|Add field/ }).click()
  const key = argumentsField.getByRole('textbox', { name: /键|Key/ }).last()
  await key.fill('text')
  await key.blur()
  const argumentText = details.locator('[data-field-path="arguments.text"]').getByRole('textbox').last()
  await expect(argumentText).toBeVisible()
  await argumentText.fill('standalone-mcp-arguments')
  const mcpKey = await details.getByLabel(/引用键|Reference key/).inputValue()
  await details.getByRole('button', { name: /^(关闭|Close)$/ }).first().click()

  await connect(page, page.getByTestId('workflow-start'), 'main', mcp, 'main')
  await connect(page, mcp, 'main', page.getByTestId('workflow-end'), 'main')
  await setEndOutput(page, mcpKey, 'json.text')
  const draft = await saveAndReadDraft(page, token, workflowId)
  const mcpDefinition = draft.definition.nodes.find((node) => node.type === 'mcp_tool')
  expect(JSON.stringify(mcpDefinition?.parameters.arguments)).toContain('standalone-mcp-arguments')

  const executionId = await startDebug(page, () => studioRun(page).click())
  await waitExecution(page, token, executionId, ['succeeded'])
  const runs = await api<{ items: Array<{ nodeId: string; output?: { main?: Array<{ json?: Record<string, unknown> }> } }> }>(page, token, `/executions/${executionId}/nodes`)
  const output = runs.items.find((node) => node.nodeId === mcpDefinition?.id)?.output?.main?.[0].json
  expect(output).toMatchObject({ text: expect.any(String), structuredOutput: { text: 'standalone-mcp-arguments' }, files: [] })
  expect(Object.keys(output ?? {}).sort()).toEqual(['files', 'structuredOutput', 'text'])
})

test('M6 standalone Wait resumes a duration suspension with the stable output contract', async ({ page }) => {
  const { token } = await login(page)
  const workflowName = `M6 Wait Contract ${Date.now()}`
  const workflowId = await createWorkflow(page, workflowName)
  await page.goto(`/workflows/${workflowId}/editor`)
  await expect(page.getByTestId('workflow-canvas')).toBeVisible()
  await setStartInputs(page)
  await addFromCreator(page, 'palette-action-wait')
  await page.getByRole('button', { name: /^(适应画布|Fit View)$/ }).click({ force: true })
  const wait = page.locator('.react-flow__node-manifest').filter({ hasText: /等待|Wait/ }).first()
  const details = await openNodeDetails(page, wait)
  await choose(page, details.getByTestId('parameter-kind'), /持续时间|Duration/)
  await details.getByTestId('parameter-durationMs').getByRole('spinbutton').fill('1500')
  const waitKey = await details.getByLabel(/引用键|Reference key/).inputValue()
  await details.getByRole('button', { name: /^(关闭|Close)$/ }).first().click()

  await connect(page, page.getByTestId('workflow-start'), 'main', wait, 'main')
  await connect(page, wait, 'resumed', page.getByTestId('workflow-end'), 'main')
  await setEndOutput(page, waitKey, 'json.status', 'resumed', false)
  const draft = await saveAndReadDraft(page, token, workflowId)
  const waitDefinition = draft.definition.nodes.find((node) => node.type === 'wait')
  expect(waitDefinition?.parameters).toMatchObject({ kind: 'duration', durationMs: 1500 })

  const executionId = await startDebug(page, () => studioRun(page).click())
  await waitExecution(page, token, executionId, ['succeeded'])
  const runs = await api<{ items: Array<{ nodeId: string; output?: { resumed?: Array<{ json?: Record<string, unknown> }> } }> }>(page, token, `/executions/${executionId}/nodes`)
  const output = runs.items.find((node) => node.nodeId === waitDefinition?.id)?.output?.resumed?.[0].json
  expect(output).toMatchObject({ status: 'resumed', payload: expect.anything(), resumedAt: expect.any(String) })
  expect(Number.isNaN(Date.parse(String(output?.resumedAt)))).toBe(false)
})

test('M6 Studio makes dual-Agent output selection explicit across serial, parallel and Merge topologies', async ({ page }, testInfo) => {
  await useRuntimePortForward(page)
  const { token } = await login(page)
  await ensureStudioResources(page, token)
  const workflowName = `M6 Multi Agent ${Date.now()}`
  const workflowId = await createWorkflow(page, workflowName)
  await grantResource(page, 'credential', studioCredentialName, undefined, workflowName)
  await grantResource(page, 'model', studioModelName, undefined, workflowName)

  await page.goto(`/workflows/${workflowId}/editor`)
  await setStartInputs(page)
  await addFromCreator(page, 'palette-action-agent')
  await addFromCreator(page, 'palette-action-agent')
  await addFromCreator(page, 'palette-binding-model')

  const agents = () => page.locator('.react-flow__node').filter({ hasText: /智能体|Agent/ })
  const firstAgent = agents().first()
  const secondAgent = agents().nth(1)
  const firstDetails = await openNodeDetails(page, firstAgent)
  await firstDetails.getByLabel('名称').fill('Agent A')
  await fillNativeText(page, firstDetails.getByTestId('parameter-systemPrompt'), 'Return the serial Agent A result.')
  const secondDetails = await openNodeDetails(page, secondAgent)
  await secondDetails.getByLabel('名称').fill('Agent B')
  await fillNativeText(page, secondDetails.getByTestId('parameter-systemPrompt'), 'Return the selected Agent B result.')

  const model = page.locator('.react-flow__node-attachment').first()
  await openNodeDetails(page, model)
  await choose(page, page.getByTestId('attachment-resource'), new RegExp(studioModelName))
  await page.getByTestId('node-details-view').getByRole('button', { name: '关闭' }).first().click()
  await page.getByRole('button', { name: /^(适应画布|Fit View)$/ }).click({ force: true })

  await connect(page, page.getByTestId('workflow-start'), 'main', firstAgent, 'main')
  await connect(page, model, 'resource', firstAgent, 'binding:ai_model')
  await connect(page, model, 'resource', secondAgent, 'binding:ai_model')
  await connect(page, firstAgent, 'main', secondAgent, 'main')
  await connect(page, secondAgent, 'main', page.getByTestId('workflow-end'), 'main')
  const draftBeforeEnd = await saveAndReadDraft(page, token, workflowId)
  const secondAgentKey = draftBeforeEnd.definition.nodes.find((node) => node.name === 'Agent B')!.key
  await setEndOutput(page, secondAgentKey, 'json.text')
  const serialDraft = await saveAndReadDraft(page, token, workflowId)
  expect(serialDraft.definition.nodes.filter((node) => node.type === 'agent')).toHaveLength(2)
  const serialExecution = await startDebug(page, () => studioRun(page).click())
  await waitExecution(page, token, serialExecution, ['succeeded'], 180_000)

  const firstAgentId = serialDraft.definition.nodes.find((node) => node.name === 'Agent A')!.id
  const secondAgentId = serialDraft.definition.nodes.find((node) => node.name === 'Agent B')!.id
  const serialConnection = serialDraft.definition.connections.find((connection) => connection.sourceNodeId === firstAgentId && connection.targetNodeId === secondAgentId)!
  const serialEdge = page.locator(`.react-flow__edge[data-testid="rf__edge-${serialConnection.id}"]`)
  await expect(serialEdge).toHaveCount(1)
  const serialToolbar = page.locator(`.studio-edge-toolbar[data-edge-id="${serialConnection.id}"]`)
  await hoverEdge(page, serialEdge, serialToolbar)
  await expect(serialToolbar).toHaveCSS('opacity', '1')
  const deleteConnection = serialToolbar.getByRole('button', { name: /删除连线|Delete connection/ })
  await expect(deleteConnection).toBeVisible()
  await deleteConnection.click()
  await expect(page.locator('.react-flow__edge:not([data-testid*="__start__"]):not([data-testid*="__end__"])')).toHaveCount(4)
  await page.getByRole('button', { name: /^(适应画布|Fit View)$/ }).click({ force: true })
  await connect(page, page.getByTestId('workflow-start'), 'main', secondAgent, 'main')
  await connect(page, firstAgent, 'main', page.getByTestId('workflow-end'), 'main')
  const parallelDraft = await saveAndReadDraft(page, token, workflowId)
  const parallelVersion = await mutate<WorkflowVersion>(page, token, `/workflows/${workflowId}/versions`, 'POST', { draftRevision: parallelDraft.revision })
  const application = await mutate<Application>(page, token, '/applications', 'POST', { workflowId, name: workflowName, slug: `m6-multi-agent-${Date.now()}`, description: 'Dual Agent output contract E2E', visibility: 'company' })
  const environments = await api<Array<{ id: string; code: string }>>(page, token, '/environments')
  const environment = environments.find((item) => item.code === 'development')
  expect(environment).toBeTruthy()
  await mutate<{ id: string }>(page, token, `/workflows/${workflowId}/deployments`, 'POST', { workflowVersionId: parallelVersion.id, environmentId: environment!.id })
  const primaryDraft = await saveAndReadDraft(page, token, workflowId)
  const primaryVersion = await mutate<WorkflowVersion>(page, token, `/workflows/${workflowId}/versions`, 'POST', { draftRevision: primaryDraft.revision })
  await mutate<{ id: string }>(page, token, `/workflows/${workflowId}/deployments`, 'POST', { workflowVersionId: primaryVersion.id, environmentId: environment!.id })
  const deployment = await mutate<ApplicationDeployment>(page, token, `/applications/${application.id}/deployments`, 'POST', { workflowVersionId: primaryVersion.id, environmentId: environment!.id, sessionVersionPolicy: 'pinned' })
  expect(deployment.id).toBeTruthy()
  await waitApplicationDeployment(page, token, application.id, deployment.id)
  await publishCompatibleChatMapping(page, token, application.id, deployment.id)

  await page.goto('/playground')
  await page.getByRole('combobox').click()
  await page.getByRole('option', { name: workflowName, exact: true }).click()
  await page.getByRole('tab', { name: '对话测试' }).click()
  const sessionResponse = page.waitForResponse((value) => value.url().includes('/gateway/v1/applications/') && value.url().endsWith('/sessions') && value.request().method() === 'POST')
  await page.getByRole('button', { name: '新建会话' }).click()
  const session = await (await sessionResponse).json() as { id: string }
  const invocationResponse = page.waitForResponse((value) => value.url().includes('/gateway/v1/sessions/') && value.url().endsWith('/messages') && value.request().method() === 'POST')
  const composer = page.getByPlaceholder('输入消息进行测试…')
  await expect(composer).toHaveJSProperty('tagName', 'TEXTAREA')
  await composer.fill('M6 dual Agent output')
  await page.getByRole('button', { name: '发送' }).click()
  const invocationHttpResponse = await invocationResponse
  if (!invocationHttpResponse.ok()) {
    throw new Error(`Session message failed with HTTP ${invocationHttpResponse.status()}: ${await invocationHttpResponse.text()}`)
  }
  const invocation = await invocationHttpResponse.json() as GatewayInvocation
  let completed: GatewayInvocation | undefined
  await expect.poll(async () => {
    const response = await page.request.get(`${gatewayBase}/gateway/v1/invocations/${invocation.id}`, { headers: { Authorization: `Bearer ${token}` } })
    completed = await response.json() as GatewayInvocation
    return completed.status
  }, { timeout: 180_000, intervals: [500, 1_000, 2_000] }).toBe('completed')
  expect(completed?.error).toBeNull()
  const messagesResponse = await page.request.get(`${gatewayBase}/gateway/v1/sessions/${session.id}/messages`, { headers: { Authorization: `Bearer ${token}` } })
  const messages = await messagesResponse.json() as GatewayMessage[]
  const sessionTitle = page.locator('aside').getByText('M6 dual Agent output', { exact: true })
  await expect(sessionTitle).toBeVisible({ timeout: 30_000 })
  await sessionTitle.hover()
  await expect(page.getByRole('tooltip')).toHaveText('M6 dual Agent output')
  const assistant = messages.find((message) => message.role === 'assistant')
  expect(assistant).toBeTruthy()
  const nodeRuns = await api<{ items: Array<{ nodeId: string; status: string; output?: { main?: Array<{ json?: { text?: string } }> } }> }>(page, token, `/executions/${completed!.executionId}/nodes`)
  const selectedRun = nodeRuns.items.find((item) => item.nodeId === secondAgentId && item.status === 'succeeded')
  const selectedJson = selectedRun?.output?.main?.[0]?.json
  const selectedText = selectedJson?.text
  expect(selectedText).toBeTruthy()
  expect(assistant!.parts.some((part) => part.content === selectedText)).toBeTruthy()

  await page.goto(`/workflows/${workflowId}/editor`)
  await expect(page.getByTestId('workflow-canvas')).toBeVisible()
  await addFromCreator(page, 'palette-action-merge')
  const merge = flowNode(page, 'merge')
  await page.getByTestId('node-details-view').getByRole('button', { name: '关闭' }).first().click()
  await page.getByRole('button', { name: /^(适应画布|Fit View)$/ }).click({ force: true })
  await connect(page, firstAgent, 'main', merge, 'main')
  await connect(page, secondAgent, 'main', merge, 'main')
  await connect(page, merge, 'main', page.getByTestId('workflow-end'), 'main')
  const mergedDraft = await saveAndReadDraft(page, token, workflowId)
  expect(mergedDraft.definition.nodes.some((node) => node.type === 'merge')).toBeTruthy()
  expect(mergedDraft.definition.end.outputs.answer.value).toMatchObject({ kind: 'reference', selector: { namespace: 'outputs', port: 'main', path: ['text'] } })
  const mergedExecution = await startDebug(page, () => studioRun(page).click())
  await waitExecution(page, token, mergedExecution, ['succeeded'], 180_000)

  await page.screenshot({ path: testInfo.outputPath('dual-agent-output-semantics.png'), fullPage: true })
})
