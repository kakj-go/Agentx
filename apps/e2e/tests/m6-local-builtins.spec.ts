import { expect, type APIResponse, type Locator, type Page, test } from '@playwright/test'

const password = 'agentx-e2e-admin-password'

type Execution = { id: string; status: string; errorCode?: string | null }
type NodeRun = { nodeId: string; status: string; errorCode?: string | null; output?: Record<string, Array<{ json: Record<string, unknown> }>> }

async function login(page: Page) {
  await page.goto('/login')
  await page.getByLabel('用户名').fill('admin')
  await page.getByLabel('密码').fill(password)
  const response = page.waitForResponse((value) => value.url().endsWith('/api/v1/auth/login') && value.request().method() === 'POST')
  await page.getByRole('button', { name: '登录' }).click()
  await expect(page).toHaveURL(/\/$/)
  return ((await (await response).json()) as { accessToken: string }).accessToken
}

async function request<T>(page: Page, token: string, path: string, method = 'GET', body?: unknown): Promise<T> {
  const response = await page.request.fetch(`/api/v1${path}`, { method, data: body, headers: { Authorization: `Bearer ${token}` } })
  await expectResponse(response, path)
  const bytes = await response.body()
  return bytes.length ? JSON.parse(bytes.toString()) as T : undefined as T
}

async function expectResponse(response: APIResponse, label: string) {
  if (!response.ok()) throw new Error(`${label}: ${response.status()} ${await response.text()}`)
}

async function createWorkflow(page: Page, name: string) {
  await page.getByRole('link', { name: '工作流', exact: true }).click()
  await page.getByRole('button', { name: '新建工作流' }).click()
  const dialog = page.getByRole('dialog', { name: '新建工作流' })
  await dialog.getByLabel('工作流名称').fill(name)
  await dialog.getByLabel('描述').fill('M6 local built-in Kubernetes E2E')
  await dialog.getByRole('button', { name: '保存' }).click()
  await expect(page).toHaveURL(/\/workflows\/[0-9a-f-]+$/)
  return page.url().split('/').at(-1) as string
}

async function runFromStudio(page: Page, token: string, workflowId: string, expectedStatus: 'succeeded' | 'failed') {
  await page.goto(`/workflows/${workflowId}/editor`)
  await expect(page.getByTestId('workflow-canvas')).toBeVisible()
  const accepted = page.waitForResponse((value) => value.url().endsWith(`/workflows/${workflowId}/debug-executions`) && value.request().method() === 'POST')
  await page.locator('header').getByRole('button', { name: '运行', exact: true }).click()
  const response = await accepted
  if (response.status() !== 202) throw new Error(`debug execution: ${response.status()} ${await response.text()}`)
  const executionId = ((await response.json()) as { executionId: string }).executionId
  let execution: Execution | undefined
  await expect.poll(async () => {
    execution = await request<Execution>(page, token, `/executions/${executionId}`)
    return execution.status
  }, { timeout: 180_000, intervals: [500, 750, 1_000, 2_000] }).toBe(expectedStatus)
  const runs = await request<{ items: NodeRun[] }>(page, token, `/executions/${executionId}/nodes`)
  return { execution: execution!, runs: runs.items }
}

function output(runs: NodeRun[], nodeId: string, port: string) {
  return runs.find((run) => run.nodeId === nodeId)?.output?.[port] ?? []
}

async function openEditor(page: Page, workflowId: string) {
  await page.goto(`/workflows/${workflowId}/editor`)
  await expect(page.getByTestId('workflow-canvas')).toBeVisible()
}

async function addNode(page: Page, nodeType: string) {
  const nodes = page.locator('.react-flow__node-manifest')
  const count = await nodes.count()
  if (!await page.getByTestId('node-creator').isVisible()) await page.getByTestId('node-creator-trigger').click()
  await page.getByRole('textbox', { name: /搜索节点|Search nodes/ }).fill(nodeType)
  await page.getByTestId(`palette-action-${nodeType}`).click()
  await expect(nodes).toHaveCount(count + 1)
  const node = nodes.last()
  const testId = await node.locator('[data-testid^="studio-node-"]').getAttribute('data-testid')
  if (!testId) throw new Error(`Node ${nodeType} did not expose a stable canvas id`)
  await expect(page.getByTestId('node-details-view')).toBeVisible()
  return { node: page.getByTestId(testId), nodeId: testId.replace('studio-node-', '') }
}

function field(page: Page, name: string) {
  return page.getByTestId(`parameter-${name}`)
}

async function setText(page: Page, name: string, value: string) {
  await field(page, name).getByRole('textbox').fill(value)
}

async function setNumber(page: Page, name: string, value: number) {
  await field(page, name).getByRole('spinbutton').fill(String(value))
}

async function setSelect(page: Page, name: string, value: string) {
  await field(page, name).getByRole('combobox').click()
  await page.getByRole('option', { name: value, exact: true }).click()
}

async function setEditor(page: Page, name: string, value: string) {
  const scope = field(page, name)
  const editor = scope.getByRole('textbox', { name: 'Editor content' })
  await expect(editor).toBeVisible({ timeout: 30_000 })
  await editor.focus()
  await page.keyboard.press('Control+A')
  await page.keyboard.press('Backspace')
  await page.keyboard.insertText(value)
  await page.keyboard.press('Control+Home')
  const visibleText = async () => (await scope.locator('.view-lines:visible').textContent())?.replaceAll('\u00a0', ' ')
  if (name === 'items' || name === 'schema') {
    const parsed = JSON.parse(value) as Record<string, unknown> | Array<Record<string, unknown>>
    const sample = Array.isArray(parsed) ? parsed[0] : parsed
    const [key, sampleValue] = Object.entries(sample)[0]
    await expect.poll(visibleText).toContain(`"${key}": ${JSON.stringify(sampleValue)}`)
    await expect(scope.getByText(/JSON 格式无效|Invalid JSON/)).toHaveCount(0)
  } else {
    await expect.poll(visibleText).toContain(value.split('\n')[0].slice(0, 64))
  }
}

async function addCollectionItem(page: Page, name: string, value: string | Record<string, unknown>) {
  const scope = field(page, name)
  await scope.getByRole('button').last().click()
  if (typeof value === 'string') await scope.getByRole('textbox').last().fill(value)
  else await scope.locator('textarea').last().fill(JSON.stringify(value))
}

async function fit(page: Page) {
  const details = page.getByTestId('node-details-view')
  if (await details.isVisible()) await details.getByRole('button', { name: /关闭|Close/ }).click()
  await page.getByRole('button', { name: 'Fit View' }).click()
}

async function connect(page: Page, source: Locator, sourceHandle: string, target: Locator, targetHandle: string) {
  const edges = page.locator('.react-flow__edge')
  const count = await edges.count()
  const from = source.locator(`.react-flow__handle.source[data-handleid="${sourceHandle}"]`)
  const to = target.locator(`.react-flow__handle.target[data-handleid="${targetHandle}"]`)
  const [fromBox, toBox] = await Promise.all([from.boundingBox(), to.boundingBox()])
  if (!fromBox || !toBox) throw new Error(`Cannot connect ${sourceHandle} to ${targetHandle}`)
  await page.mouse.move(fromBox.x + fromBox.width / 2, fromBox.y + fromBox.height / 2)
  await page.mouse.down()
  await page.mouse.move(toBox.x + toBox.width / 2, toBox.y + toBox.height / 2, { steps: 12 })
  await page.mouse.up()
  await expect(edges).toHaveCount(count + 1)
}

async function save(page: Page) {
  const button = page.getByRole('button', { name: /^(保存|Save)$/ })
  if (await button.isEnabled()) await button.click()
  await expect(page.locator('header').getByText(/Revision \d+ · (已保存|Saved)/)).toBeVisible({ timeout: 30_000 })
}

test('M6 local built-ins execute transform, multi-input and validation/error workflows through Studio', async ({ page }, testInfo) => {
  test.slow()
  const token = await login(page)
  const suffix = Date.now()

  const transformId = await createWorkflow(page, `M6 Local Transform ${suffix}`)
  await openEditor(page, transformId)
  const transformNodes = [] as Array<{ node: Locator; nodeId: string }>
  const generatedItems = [
    { id: 1, group: 'a', score: 3, tags: ['x', 'y'], payload: '{"ok":true}', when: '2026-01-01T00:00:00Z', text: 'hello', old: { name: 'alpha' } },
    { id: 1, group: 'a', score: 2, tags: ['z'], payload: '{"ok":false}', when: '2026-01-02T00:00:00Z', text: 'duplicate', old: { name: 'duplicate' } },
    { id: 2, group: 'b', score: 4, tags: ['m'], payload: '{"ok":true}', when: '2026-02-01T00:00:00Z', text: 'world', old: { name: 'beta' } },
    { id: 3, group: 'b', score: 1, tags: ['ignored'], payload: '{}', when: '2026-03-01T00:00:00Z', text: 'low', old: { name: 'low' } },
  ]
  transformNodes.push(await addNode(page, 'item_generator')); await setEditor(page, 'items', JSON.stringify(generatedItems))
  transformNodes.push(await addNode(page, 'filter')); await setEditor(page, 'condition', '=$json.score >= 2')
  transformNodes.push(await addNode(page, 'sort')); await addCollectionItem(page, 'fields', { field: 'score', direction: 'desc', nulls: 'last' })
  transformNodes.push(await addNode(page, 'remove_duplicates')); await addCollectionItem(page, 'fields', 'id')
  transformNodes.push(await addNode(page, 'split_out')); await setText(page, 'field', 'tags')
  transformNodes.push(await addNode(page, 'rename_fields')); await addCollectionItem(page, 'mappings', { from: 'old.name', to: 'name' })
  transformNodes.push(await addNode(page, 'json_transform')); await setText(page, 'field', 'payload'); await setText(page, 'outputField', 'parsed')
  transformNodes.push(await addNode(page, 'date_time')); await setSelect(page, 'operation', 'add'); await setText(page, 'field', 'when'); await setText(page, 'outputField', 'adjusted'); await setNumber(page, 'amount', 1); await setSelect(page, 'unit', 'days')
  transformNodes.push(await addNode(page, 'base64')); await setText(page, 'field', 'text'); await setText(page, 'outputField', 'encoded')
  transformNodes.push(await addNode(page, 'hash')); await setText(page, 'field', 'text'); await setText(page, 'outputField', 'digest')
  transformNodes.push(await addNode(page, 'limit')); await setNumber(page, 'maxItems', 10)
  transformNodes.push(await addNode(page, 'aggregate')); await addCollectionItem(page, 'groupBy', 'group'); await addCollectionItem(page, 'operations', { operation: 'count', outputField: 'count' })
  transformNodes.push(await addNode(page, 'no_op'))
  await fit(page)
  for (let index = 0; index < transformNodes.length - 1; index += 1) await connect(page, transformNodes[index].node, 'main', transformNodes[index + 1].node, 'main')
  await save(page)
  const transform = await runFromStudio(page, token, transformId, 'succeeded')
  const aggregateItems = output(transform.runs, transformNodes.at(-2)!.nodeId, 'main')
  expect(aggregateItems).toHaveLength(2)
  expect(aggregateItems.map((item) => item.json.count)).toEqual(expect.arrayContaining([1, 2]))
  expect(output(transform.runs, transformNodes.at(-1)!.nodeId, 'main')).toHaveLength(2)
  await page.screenshot({ path: testInfo.outputPath('local-transform-chain.png'), fullPage: true })

  const compareId = await createWorkflow(page, `M6 Merge Compare ${suffix}`)
  await openEditor(page, compareId)
  const left = await addNode(page, 'item_generator'); await setEditor(page, 'items', JSON.stringify([{ id: 1, left: 'A' }, { id: 2, left: 'B' }]))
  const right = await addNode(page, 'item_generator'); await setEditor(page, 'items', JSON.stringify([{ id: 2, right: 'B' }, { id: 3, right: 'C' }]))
  const merge = await addNode(page, 'merge'); await setSelect(page, 'mode', 'combine_by_key'); await setText(page, 'leftField', 'id'); await setText(page, 'rightField', 'id'); await setSelect(page, 'joinType', 'full'); await setSelect(page, 'conflictStrategy', 'suffix')
  const compare = await addNode(page, 'compare_datasets'); await addCollectionItem(page, 'keyFields', 'id')
  await fit(page)
  await connect(page, left.node, 'main', merge.node, 'left')
  await connect(page, right.node, 'main', merge.node, 'right')
  await connect(page, merge.node, 'main', compare.node, 'left')
  await connect(page, right.node, 'main', compare.node, 'right')
  await save(page)
  const compared = await runFromStudio(page, token, compareId, 'succeeded')
  expect(output(compared.runs, merge.nodeId, 'main')).toHaveLength(3)
  expect(output(compared.runs, compare.nodeId, 'same')).toHaveLength(1)
  expect(output(compared.runs, compare.nodeId, 'different')).toHaveLength(1)
  expect(output(compared.runs, compare.nodeId, 'left_only')).toHaveLength(1)
  expect(output(compared.runs, compare.nodeId, 'right_only')).toHaveLength(0)
  await page.screenshot({ path: testInfo.outputPath('merge-compare-chain.png'), fullPage: true })

  const failureId = await createWorkflow(page, `M6 Validator Stop ${suffix}`)
  await openEditor(page, failureId)
  const validationSource = await addNode(page, 'item_generator'); await setEditor(page, 'items', JSON.stringify([{ id: 1, name: 'valid' }, { id: 2 }]))
  const validator = await addNode(page, 'structured_validator'); await setEditor(page, 'schema', JSON.stringify({ type: 'object', required: ['name'], properties: { name: { type: 'string' } } }))
  const validPass = await addNode(page, 'no_op')
  const stop = await addNode(page, 'stop_and_error'); await setEditor(page, 'code', 'E2E_VALIDATION_STOP'); await setEditor(page, 'message', 'Invalid local item')
  await fit(page)
  await connect(page, validationSource.node, 'main', validator.node, 'main')
  await connect(page, validator.node, 'valid', validPass.node, 'main')
  await connect(page, validator.node, 'invalid', stop.node, 'main')
  await save(page)
  const failed = await runFromStudio(page, token, failureId, 'failed')
  expect(output(failed.runs, validator.nodeId, 'valid')).toHaveLength(1)
  expect(output(failed.runs, validator.nodeId, 'invalid')).toHaveLength(1)
  expect(failed.runs.find((run) => run.nodeId === stop.nodeId)).toMatchObject({ status: 'failed', errorCode: 'E2E_VALIDATION_STOP' })
  await page.screenshot({ path: testInfo.outputPath('validator-stop-error-chain.png'), fullPage: true })
})
