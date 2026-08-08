import { expect, type APIResponse, type Page, test } from '@playwright/test'
import { writeFile } from 'node:fs/promises'

const password = 'agentx-e2e-admin-password'
type Draft = { revision: number; updatedAt: string; [key: string]: unknown }
type Metric = { nodeCount: number; firstInteractiveMs: number; medianFps: number; p95InputDelayMs: number; renderedNodes: number }
type CatalogPage = { items: Array<{ nodeType: string; version: number; [key: string]: unknown }> }

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
  return response.json() as Promise<T>
}

async function expectResponse(response: APIResponse, label: string) {
  if (!response.ok()) throw new Error(`${label}: ${response.status()} ${await response.text()}`)
}

async function createWorkflow(page: Page, token: string, nodeCount: number) {
  return request<{ id: string }>(page, token, '/workflows', 'POST', { name: `M6 Canvas ${nodeCount} ${Date.now()}`, description: 'Synthetic canvas performance benchmark', visibility: 'company' })
}

function syntheticDraft(base: Draft, nodeCount: number) {
  const nodes = Array.from({ length: nodeCount }, (_, index) => ({
    id: `perf-${index}`, type: 'no_op', typeVersion: 1, name: `No Operation ${index}`, disabled: false,
    parameters: {}, resourceReferences: [], settings: {},
  }))
  const connections = nodes.slice(0, -1).map((value, index) => ({
    id: `perf-edge-${index}`, sourceNodeId: value.id, sourceHandle: 'main', targetNodeId: nodes[index + 1].id, targetHandle: 'main', order: 0,
  }))
  return {
    ...base,
    updatedAt: new Date().toISOString(),
    definition: { schemaVersion: '3.0', nodes, connections, settings: { executionOrder: 'deterministic', activationBudget: 10_000 } },
    editorDocument: {
      nodeLayouts: nodes.map((value, index) => ({ nodeId: value.id, x: 80 + (index % 50) * 150, y: 80 + Math.floor(index / 50) * 150 })),
      bindingLayouts: [], edges: connections.map((value) => ({ edgeId: value.id })), bindingEdges: [], annotations: [], groups: [],
      viewport: { x: 0, y: 0, zoom: 0.7 },
    },
  }
}

async function benchmark(page: Page, token: string, nodeCount: number): Promise<Metric> {
  const workflow = await createWorkflow(page, token, nodeCount)
  const base = await request<Draft>(page, token, `/workflows/${workflow.id}/draft`)
  const draft = syntheticDraft(base, nodeCount)
  await page.route(`**/api/v1/workflows/${workflow.id}/draft`, async (route) => {
    if (route.request().method() === 'GET') await route.fulfill({ json: draft })
    else await route.fulfill({ json: { ...draft, revision: base.revision + 1, updatedAt: new Date().toISOString() } })
  })

  const started = Date.now()
  await page.goto(`/workflows/${workflow.id}/editor`)
  const canvas = page.getByTestId('workflow-canvas')
  await expect(canvas).toBeVisible()
  const firstNode = page.getByTestId('studio-node-perf-0')
  await expect(firstNode).toBeVisible()
  const firstInteractiveMs = Date.now() - started
  expect(firstInteractiveMs).toBeLessThanOrEqual(3_000)
  await expect(page.locator('.react-flow__minimap')).toHaveCount(nodeCount >= 300 ? 0 : 1)

  await page.evaluate(() => {
    const target = window as Window & { __agentxPerf?: { frames: number[]; inputDelays: number[]; started: number; lastFrame?: number } }
    const metrics = { frames: [] as number[], inputDelays: [] as number[], started: performance.now(), lastFrame: undefined as number | undefined }
    target.__agentxPerf = metrics
    const input = () => {
      const eventAt = performance.now()
      requestAnimationFrame(() => metrics.inputDelays.push(performance.now() - eventAt))
    }
    document.addEventListener('pointermove', input, { passive: true })
    document.addEventListener('wheel', input, { passive: true })
    const frame = (now: number) => {
      if (metrics.lastFrame !== undefined) metrics.frames.push(now - metrics.lastFrame)
      metrics.lastFrame = now
      if (now - metrics.started < 1_800) requestAnimationFrame(frame)
      else {
        document.removeEventListener('pointermove', input)
        document.removeEventListener('wheel', input)
      }
    }
    requestAnimationFrame(frame)
  })

  const box = await firstNode.boundingBox()
  expect(box).toBeTruthy()
  await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2)
  await page.mouse.down()
  for (let index = 0; index < 50; index += 1) {
    await page.mouse.move(box!.x + box!.width / 2 + index * 2, box!.y + box!.height / 2 + Math.sin(index / 4) * 20)
    await page.waitForTimeout(8)
  }
  await page.mouse.up()
  const canvasBox = await canvas.boundingBox()
  expect(canvasBox).toBeTruthy()
  await page.mouse.move(canvasBox!.x + canvasBox!.width / 2, canvasBox!.y + canvasBox!.height / 2)
  for (let index = 0; index < 12; index += 1) await page.mouse.wheel(0, index % 2 === 0 ? -80 : 80)
  await page.waitForTimeout(1_900)

  const samples = await page.evaluate(() => (window as Window & { __agentxPerf?: { frames: number[]; inputDelays: number[] } }).__agentxPerf!)
  const medianFrame = percentile(samples.frames, 0.5)
  const metric = {
    nodeCount,
    firstInteractiveMs,
    medianFps: Number((1_000 / medianFrame).toFixed(1)),
    p95InputDelayMs: Number(percentile(samples.inputDelays, 0.95).toFixed(1)),
    renderedNodes: await page.locator('.react-flow__node-manifest').count(),
  }
  expect(metric.renderedNodes).toBeLessThan(nodeCount)
  expect(metric.medianFps).toBeGreaterThanOrEqual(nodeCount === 500 ? 50 : 30)
  expect(metric.p95InputDelayMs).toBeLessThanOrEqual(nodeCount === 500 ? 50 : 100)
  await page.unroute(`**/api/v1/workflows/${workflow.id}/draft`)
  return metric
}

function percentile(values: number[], ratio: number) {
  expect(values.length).toBeGreaterThan(0)
  const sorted = [...values].sort((left, right) => left - right)
  return sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil(sorted.length * ratio) - 1))]
}

test('Workflow canvas meets the 500 and 1000 node interaction budgets', async ({ page }, testInfo) => {
  test.slow()
  const token = await login(page)
  const catalog = await request<CatalogPage>(page, token, '/node-definitions?pageSize=100')
  const noOperation = catalog.items.find((item) => item.nodeType === 'no_op' && item.version === 1)
  expect(noOperation).toBeTruthy()
  const detail = await request<Record<string, unknown>>(page, token, '/node-definitions/no_op/versions/1')
  await page.route('**/api/v1/node-definitions?pageSize=100', (route) => route.fulfill({ json: { items: [noOperation] } }))
  await page.route('**/api/v1/node-definitions/no_op/versions/1', (route) => route.fulfill({ json: detail }))
  const metrics = [await benchmark(page, token, 500), await benchmark(page, token, 1_000)]
  const evidence = JSON.stringify(metrics, null, 2)
  const evidencePath = testInfo.outputPath('workflow-canvas-performance.json')
  await writeFile(evidencePath, evidence, 'utf8')
  await testInfo.attach('workflow-canvas-performance.json', { path: evidencePath, contentType: 'application/json' })
})
