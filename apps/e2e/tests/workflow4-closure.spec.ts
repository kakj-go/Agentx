import { expect, type APIResponse, type Page, test } from '@playwright/test'

const password = 'agentx-e2e-admin-password'
const company = 'Agentx E2E'
const gatewayBase = process.env.AGENTX_E2E_RUNTIME_URL ?? ''
const echoBaseUrl = process.env.AGENTX_E2E_ECHO_BASE_URL ?? 'http://echo-mcp:8090'

type Workflow = { id: string; name: string }
type Draft = { revision: number; definition: Definition; editorDocument: Record<string, unknown> }
type Version = { id: string; versionNumber: number }
type Application = { id: string; slug: string; apiKey: string }
type Deployment = { id: string; status: string }
type Environment = { id: string; code: string }
type TerminalError = { primaryError?: { code?: string; message?: string }; errors?: unknown[]; outputs?: Record<string, unknown> }
type Invocation = { id: string; executionId?: string; status: string; outputs?: Record<string, unknown>; error?: TerminalError }
type Execution = { id: string; status: string; parentExecutionId?: string; errorCode?: string }
type ExecutionPage = { items: Execution[] }
type Approval = { id: string; executionId: string; status: string; version: number }
type PageResponse<T> = { items: T[] }
type Definition = {
  schemaVersion: '6.0'
  start: { inputs: Record<string, unknown>; contexts: Record<string, unknown> }
  nodes: Array<Record<string, unknown> & { id: string; key: string; type: string }>
  connections: Array<{ id: string; sourceNodeId: string; sourceHandle: string; targetNodeId: string; targetHandle: string; order: number }>
  end: { outputs: Record<string, unknown>; error?: { strategy: 'fail_fast' | 'collect'; collectWindowMs: number; outputs: Record<string, unknown> } }
  settings: Record<string, unknown>
}

async function login(page: Page) {
  const bootstrapStatus = await page.request.get('/api/v1/bootstrap/status')
  await expectResponse(bootstrapStatus, 'bootstrap status')
  if (((await bootstrapStatus.json()) as { required: boolean }).required) {
    await page.goto('/setup')
    await page.getByLabel('公司名称').fill(company)
    await page.getByLabel('用户名').fill('admin')
    await page.getByLabel('管理员姓名').fill('E2E Admin')
    await page.getByLabel('密码').fill(password)
    const response = page.waitForResponse((value) => value.url().endsWith('/api/v1/bootstrap') && value.request().method() === 'POST')
    await page.getByRole('button', { name: '初始化并进入工作台' }).click()
    await expect(page).toHaveURL(/\/$/)
    return ((await (await response).json()) as { accessToken: string }).accessToken
  }
  await page.goto('/login')
  await page.getByLabel('用户名').fill('admin')
  await page.getByLabel('密码').fill(password)
  const response = page.waitForResponse((value) => value.url().endsWith('/api/v1/auth/login') && value.request().method() === 'POST')
  await page.getByRole('button', { name: '登录' }).click()
  await expect(page).toHaveURL(/\/$/)
  return ((await (await response).json()) as { accessToken: string }).accessToken
}

async function expectResponse(response: APIResponse, label: string) {
  if (!response.ok()) throw new Error(`${label}: ${response.status()} ${await response.text()}`)
}

async function request<T>(page: Page, token: string, path: string, method = 'GET', body?: unknown, gateway = false, headers: Record<string, string> = {}): Promise<T> {
  const response = await page.request.fetch(`${gateway ? `${gatewayBase}/gateway/v1` : '/api/v1'}${path}`, {
    method,
    data: body,
    headers: { Authorization: `Bearer ${token}`, ...headers },
  })
  await expectResponse(response, path)
  const text = await response.text()
  return (text ? JSON.parse(text) : undefined) as T
}

function node(id: string, type: string, parameters: Record<string, unknown>, extra: Record<string, unknown> = {}) {
  return {
    id,
    key: id.replaceAll('-', '_'),
    type,
    typeVersion: 1,
    name: id,
    disabled: false,
    parameters,
    outputProjection: {},
    contextWrites: [],
    resourceReferences: [],
    settings: {},
    ...extra,
  }
}

const reference = (namespace: 'inputs' | 'outputs' | 'contexts' | 'item', path: string[], sourceNodeId?: string) => ({
  kind: 'reference',
  selector: { namespace, sourceNodeId, port: sourceNodeId ? 'main' : undefined, run: { kind: 'current' }, item: { kind: 'current' }, path },
  missingPolicy: { kind: 'error' },
})
const template = (prefix: string, value: ReturnType<typeof reference>) => ({
  kind: 'template',
  segments: [{ kind: 'text', text: prefix }, { kind: 'reference', selector: value.selector, missingPolicy: value.missingPolicy }],
})
const literal = (value: unknown) => ({ kind: 'literal', value })

async function createWorkflow(page: Page, token: string, name: string, definition: Definition) {
  const workflow = await request<Workflow>(page, token, '/workflows', 'POST', {
    name,
    description: 'Workflow 5.0 Kubernetes closure',
    visibility: 'company',
  })
  const draft = await request<Draft>(page, token, `/workflows/${workflow.id}/draft`)
  const saved = await request<Draft>(page, token, `/workflows/${workflow.id}/draft`, 'PUT', {
    expectedRevision: draft.revision,
    definition,
    editorDocument: {
      ...draft.editorDocument,
      nodeLayouts: definition.nodes.map((value, index) => ({ nodeId: value.id, x: 120 + index * 260, y: 180 })),
      edges: definition.connections.map((value) => ({ edgeId: value.id })),
    },
  })
  const version = await request<Version>(page, token, `/workflows/${workflow.id}/versions`, 'POST', { draftRevision: saved.revision })
  return { workflow, version, draft: saved }
}

async function reviseWorkflow(page: Page, token: string, workflowId: string, definition: Definition) {
  const draft = await request<Draft>(page, token, `/workflows/${workflowId}/draft`)
  const saved = await request<Draft>(page, token, `/workflows/${workflowId}/draft`, 'PUT', {
    expectedRevision: draft.revision,
    definition,
    editorDocument: {
      ...draft.editorDocument,
      nodeLayouts: definition.nodes.map((value, index) => ({ nodeId: value.id, x: 120 + index * 260, y: 180 })),
      edges: definition.connections.map((value) => ({ edgeId: value.id })),
    },
  })
  return request<Version>(page, token, `/workflows/${workflowId}/versions`, 'POST', { draftRevision: saved.revision })
}

async function deployApplication(page: Page, token: string, workflow: Workflow, version: Version, environmentId: string, slug: string) {
  await request(page, token, `/workflows/${workflow.id}/deployments`, 'POST', { workflowVersionId: version.id, environmentId })
  const application = await request<Omit<Application, 'apiKey'>>(page, token, '/applications', 'POST', {
    workflowId: workflow.id,
    name: `${workflow.name} Application`,
    slug,
    description: 'Workflow 5.0 E2E application',
    visibility: 'company',
  })
  const apiKey = await request<{ secret: string }>(page, token, `/applications/${application.id}/api-keys`, 'POST', {
    name: 'Workflow 5.0 E2E',
  })
  const deployment = await request<Deployment>(page, token, `/applications/${application.id}/deployments`, 'POST', {
    workflowVersionId: version.id,
    environmentId,
    sessionVersionPolicy: 'pinned',
  })
  await expect.poll(async () => {
    const deployments = await request<Deployment[]>(page, token, `/applications/${application.id}/deployments`)
    const status = deployments.find((value) => value.id === deployment.id)?.status
    if (status === 'rejected') throw new Error(`application deployment ${deployment.id} was rejected`)
    return status
  }, { timeout: 180_000, intervals: [500, 1_000, 2_000] }).toBe('active')
  return { ...application, apiKey: apiKey.secret }
}

async function waitInvocation(page: Page, token: string, id: string) {
  let current: Invocation | undefined
  await expect.poll(async () => {
    current = await request<Invocation>(page, token, `/invocations/${id}`, 'GET', undefined, true)
    return ['completed', 'failed', 'cancelled'].includes(current.status)
  }, { timeout: 180_000, intervals: [250, 500, 1_000, 2_000] }).toBeTruthy()
  return current!
}

async function waitExecution(page: Page, token: string, id: string, statuses: string[]) {
  let current: Execution | undefined
  await expect.poll(async () => {
    current = await request<Execution>(page, token, `/executions/${id}`)
    return statuses.includes(current.status)
  }, { timeout: 180_000, intervals: [250, 500, 1_000, 2_000] }).toBeTruthy()
  return current!
}

const counterContext = {
  counter: {
    schema: { type: 'number' },
    default: 0,
    mutable: true,
    sensitive: false,
    scope: 'execution_tree',
    mergePolicy: 'increment',
    clientWritable: false,
  },
}

test('Workflow 5.0 closes Composite, Context, Package, Multipart and cancellation contracts', async ({ page }) => {
  const token = await login(page)
  const suffix = Date.now()
  const environment = (await request<Environment[]>(page, token, '/environments')).find((value) => value.code === 'development')
  expect(environment).toBeTruthy()

  const childDefinition: Definition = {
    schemaVersion: '6.0',
    start: {
      inputs: { type: 'object', required: ['question'], properties: { question: { type: 'string' } }, additionalProperties: false },
      contexts: counterContext,
    },
    nodes: [node('answer', 'set', { values: { answer: template('v1:', reference('inputs', ['question'])) }, keepOnlySet: true }, {
      contextWrites: [{ operation: 'increment', path: 'counter', value: { kind: 'literal', value: 1 } }],
    })],
    connections: [
      { id: 'start-answer', sourceNodeId: '__start__', sourceHandle: 'main', targetNodeId: 'answer', targetHandle: 'main', order: 0 },
      { id: 'answer-end', sourceNodeId: 'answer', sourceHandle: 'main', targetNodeId: '__end__', targetHandle: 'main', order: 0 },
    ],
    end: { outputs: { answer: { value: reference('outputs', ['answer'], 'answer'), schema: { type: 'string' }, required: true, sensitive: false } } },
    settings: { executionOrder: 'deterministic' },
  }
  const child = await createWorkflow(page, token, `W4 Child ${suffix}`, childDefinition)
  const childType = `workflow.${child.version.id.replaceAll('-', '')}`

  const parentDefinition: Definition = {
    schemaVersion: '6.0',
    start: {
      inputs: {
        type: 'object',
        required: ['question', 'attachments'],
        properties: {
          question: { type: 'string', minLength: 2 },
          prefix: { type: 'string', default: 'default-applied' },
          attachments: {
            type: 'array',
            minItems: 1,
            maxItems: 2,
            items: { type: 'object' },
            'x-agentx-artifact': true,
            'x-agentx-artifact-array': true,
            'x-agentx-content-types': ['text/plain'],
            'x-agentx-max-size-bytes': 100,
            'x-agentx-max-total-size-bytes': 100,
          },
        },
        additionalProperties: false,
      },
      contexts: counterContext,
    },
    nodes: [
      node('child', childType, { workflowVersionId: child.version.id, inputs: { question: reference('inputs', ['question']) } }),
      node('summary', 'set', { values: { answer: reference('outputs', ['answer'], 'child'), counter: reference('contexts', ['counter']) }, keepOnlySet: true }, {
        outputProjection: {
          main: {
            answer_text: { value: reference('item', ['answer']), schema: { type: 'string' }, sensitive: false },
            counter_value: { value: reference('item', ['counter']), schema: { type: 'number' }, sensitive: false },
          },
        },
      }),
    ],
    connections: [
      { id: 'start-child', sourceNodeId: '__start__', sourceHandle: 'main', targetNodeId: 'child', targetHandle: 'main', order: 0 },
      { id: 'child-summary', sourceNodeId: 'child', sourceHandle: 'main', targetNodeId: 'summary', targetHandle: 'main', order: 0 },
      { id: 'summary-end', sourceNodeId: 'summary', sourceHandle: 'main', targetNodeId: '__end__', targetHandle: 'main', order: 0 },
    ],
    end: {
      outputs: {
        answer: { value: reference('outputs', ['answer_text'], 'summary'), schema: { type: 'string' }, required: true, sensitive: false },
        counter: { value: reference('outputs', ['counter_value'], 'summary'), schema: { type: 'number' }, required: true, sensitive: false },
        attachments: { value: reference('inputs', ['attachments']), schema: { type: 'array' }, required: true, sensitive: false },
        prefix: { value: reference('inputs', ['prefix']), schema: { type: 'string' }, required: true, sensitive: false },
      },
    },
    settings: { executionOrder: 'deterministic' },
  }
  const parent = await createWorkflow(page, token, `W4 Parent ${suffix}`, parentDefinition)

  const childV2 = structuredClone(childDefinition)
  ;(childV2.nodes[0].parameters as Record<string, unknown>).values = { answer: template('v2:', reference('inputs', ['question'])) }
  const secondChildVersion = await reviseWorkflow(page, token, child.workflow.id, childV2)
  expect(secondChildVersion.versionNumber).toBe(2)

  const application = await deployApplication(page, token, parent.workflow, parent.version, environment!.id, `w4-parent-${suffix}`)
  const invalidInput = await page.request.post(`${gatewayBase}/gateway/v1/workflows/${application.slug}/invoke`, {
    headers: { Authorization: `Bearer ${application.apiKey}`, 'Idempotency-Key': `w4-invalid-${suffix}` },
    data: { input: { question: 'x', attachments: [] }, responseMode: 'async' },
  })
  expect(invalidInput.status()).toBe(400)
  expect(await invalidInput.json()).toMatchObject({ code: 'INPUT_SCHEMA_VALIDATION_FAILED' })

  const uploadArtifact = async (id: string, file: { name: string; type: string; contents: string }, index = 0) => {
    const response = await page.request.post(`${gatewayBase}/gateway/v1/artifacts`, {
      headers: { Authorization: `Bearer ${application.apiKey}`, 'Idempotency-Key': `w4-artifact-${id}-${index}-${suffix}` },
      multipart: { file: { name: file.name, mimeType: file.type, buffer: Buffer.from(file.contents) } },
    })
    await expectResponse(response, `Artifact upload ${id}/${index}`)
    return { ...await response.json() as { artifactId: string; contentType: string; sizeBytes: number; sha256: string }, type: 'file', fileName: file.name }
  }
  const rejectFiles = async (id: string, files: Array<{ name: string; type: string; contents: string }>, code: string) => {
    const attachments = await Promise.all(files.map((file, index) => uploadArtifact(id, file, index)))
    const response = await page.request.post(`${gatewayBase}/gateway/v1/workflows/${application.slug}/invoke`, {
      headers: { Authorization: `Bearer ${application.apiKey}`, 'Idempotency-Key': `w4-${id}-${suffix}` },
      data: { input: { question: 'hello', attachments }, responseMode: 'async' },
    })
    expect(response.status()).toBe(400)
    expect(await response.json()).toMatchObject({ code })
  }
  await rejectFiles('mime', [{ name: 'question.json', type: 'application/json', contents: '{}' }], 'ARTIFACT_CONTENT_TYPE_NOT_ALLOWED')
  await rejectFiles('file-size', [{ name: 'large.txt', type: 'text/plain', contents: 'x'.repeat(101) }], 'ARTIFACT_TOO_LARGE')
  await rejectFiles('total-size', [
    { name: 'first.txt', type: 'text/plain', contents: 'x'.repeat(60) },
    { name: 'second.txt', type: 'text/plain', contents: 'x'.repeat(60) },
  ], 'ARTIFACT_TOTAL_SIZE_EXCEEDED')
  await rejectFiles('file-count', [
    { name: 'first.txt', type: 'text/plain', contents: '1' },
    { name: 'second.txt', type: 'text/plain', contents: '2' },
    { name: 'third.txt', type: 'text/plain', contents: '3' },
  ], 'INPUT_SCHEMA_VALIDATION_FAILED')

  const artifact = await uploadArtifact('success', { name: 'question.txt', type: 'text/plain', contents: 'workflow-4-file' })
  const artifactInvocation = await page.request.post(`${gatewayBase}/gateway/v1/workflows/${application.slug}/invoke`, {
    headers: { Authorization: `Bearer ${application.apiKey}`, 'Idempotency-Key': `w4-multipart-${suffix}` },
    data: { input: { question: 'hello', attachments: [artifact] }, responseMode: 'sync' },
  })
  await expectResponse(artifactInvocation, 'Artifact reference invoke')
  const invocation = await artifactInvocation.json() as Invocation
  const completed = invocation.status === 'completed' ? invocation : await waitInvocation(page, application.apiKey, invocation.id)
  expect(completed.status).toBe('completed')
  expect(completed.outputs).toMatchObject({ answer: 'v1:hello', counter: 1, prefix: 'default-applied' })
  expect(completed.outputs?.attachments).toEqual([
    expect.objectContaining({ artifactId: expect.stringMatching(/^[0-9a-f-]{36}$/), fileName: 'question.txt', contentType: 'text/plain' }),
  ])

  const exported = await request<Record<string, unknown>>(page, token, `/workflows/${parent.workflow.id}/export`)
  const imported = await request<{ workflowId: string }>(page, token, '/workflows/import', 'POST', {
    name: `W4 Imported ${suffix}`,
    description: 'Cross-project package contract',
    visibility: 'company',
    package: exported,
    resourceBindings: {},
  })
  const importedDraft = await request<Draft>(page, token, `/workflows/${imported.workflowId}/draft`)
  expect(importedDraft.definition.schemaVersion).toBe('6.0')
  expect(importedDraft.definition.nodes[0].type).toBe(childType)

  const recursiveDefinition = structuredClone(childV2)
  recursiveDefinition.nodes = [node('parent', `workflow.${parent.version.id.replaceAll('-', '')}`, {
    workflowVersionId: parent.version.id,
    inputs: { question: reference('inputs', ['question']), attachments: [] },
  })]
  recursiveDefinition.connections = [
    { id: 'start-parent', sourceNodeId: '__start__', sourceHandle: 'main', targetNodeId: 'parent', targetHandle: 'main', order: 0 },
    { id: 'parent-end', sourceNodeId: 'parent', sourceHandle: 'main', targetNodeId: '__end__', targetHandle: 'main', order: 0 },
  ]
  recursiveDefinition.end.outputs = {}
  const recursiveDraft = await request<Draft>(page, token, `/workflows/${child.workflow.id}/draft`)
  const savedRecursive = await request<Draft>(page, token, `/workflows/${child.workflow.id}/draft`, 'PUT', {
    expectedRevision: recursiveDraft.revision,
    definition: recursiveDefinition,
    editorDocument: { ...recursiveDraft.editorDocument, nodeLayouts: [{ nodeId: 'parent', x: 120, y: 180 }], edges: [] },
  })
  const recursiveVersion = await page.request.post(`/api/v1/workflows/${child.workflow.id}/versions`, {
    headers: { Authorization: `Bearer ${token}` },
    data: { draftRevision: savedRecursive.revision },
  })
  expect(recursiveVersion.status()).toBe(422)
  expect(await recursiveVersion.json()).toMatchObject({ code: 'RECURSIVE_SUBWORKFLOW' })

  const slowChildDefinition: Definition = {
    schemaVersion: '6.0',
    start: { inputs: { type: 'object', properties: {}, additionalProperties: false }, contexts: {} },
    nodes: [node('wait', 'wait', { kind: 'duration', durationMs: 60_000 })],
    connections: [
      { id: 'start-wait', sourceNodeId: '__start__', sourceHandle: 'main', targetNodeId: 'wait', targetHandle: 'main', order: 0 },
      { id: 'wait-end', sourceNodeId: 'wait', sourceHandle: 'resumed', targetNodeId: '__end__', targetHandle: 'main', order: 0 },
      { id: 'wait-timeout-end', sourceNodeId: 'wait', sourceHandle: 'timed_out', targetNodeId: '__end__', targetHandle: 'main', order: 0 },
    ],
    end: { outputs: {} },
    settings: { executionOrder: 'deterministic' },
  }
  const slowChild = await createWorkflow(page, token, `W4 Slow Child ${suffix}`, slowChildDefinition)
  const slowParentDefinition: Definition = {
    schemaVersion: '6.0',
    start: { inputs: { type: 'object', properties: {}, additionalProperties: false }, contexts: {} },
    nodes: [node('slow_child', `workflow.${slowChild.version.id.replaceAll('-', '')}`, { workflowVersionId: slowChild.version.id, inputs: {} }, {
      settings: { timeoutMs: 1_500, retryOnFail: false, maxTries: 1 },
    })],
    connections: [
      { id: 'start-slow-child', sourceNodeId: '__start__', sourceHandle: 'main', targetNodeId: 'slow_child', targetHandle: 'main', order: 0 },
      { id: 'slow-child-end', sourceNodeId: 'slow_child', sourceHandle: 'main', targetNodeId: '__end__', targetHandle: 'main', order: 0 },
    ],
    end: {
      outputs: {
        status: { value: { kind: 'literal', value: 'timeout-test' }, schema: { type: 'string' }, required: true, sensitive: false },
      },
    },
    settings: { executionOrder: 'deterministic' },
  }
  const slowParent = await createWorkflow(page, token, `W4 Slow Parent ${suffix}`, slowParentDefinition)
  const slowApplication = await deployApplication(page, token, slowParent.workflow, slowParent.version, environment!.id, `w4-slow-${suffix}`)
  const slowInvoke = await request<Invocation>(page, slowApplication.apiKey, `/workflows/${slowApplication.slug}/invoke`, 'POST', { input: {}, responseMode: 'async' }, true, {
    'Idempotency-Key': `w4-slow-${suffix}`,
  })
  const slowTerminal = await waitInvocation(page, slowApplication.apiKey, slowInvoke.id)
  expect(slowTerminal.status).toBe('failed')
  expect(slowTerminal.error?.primaryError?.code).toBeTruthy()
  const parentExecution = await waitExecution(page, token, slowTerminal.executionId!, ['failed'])
  let childExecution: Execution | undefined
  await expect.poll(async () => {
    const executions = await request<ExecutionPage>(page, token, '/executions?limit=100')
    childExecution = executions.items.find((value) => value.parentExecutionId === parentExecution.id)
    return childExecution?.status
  }, { timeout: 60_000 }).toBe('cancelled')
})

test('Workflow 5.0 declarative HTTP consumes request parameters and canonically converts its body for End', async ({ page }) => {
  const token = await login(page)
  const suffix = Date.now()
  const environment = (await request<Environment[]>(page, token, '/environments')).find((value) => value.code === 'development')!
  const definition: Definition = {
    schemaVersion: '6.0',
    start: { inputs: { type: 'object', properties: {}, additionalProperties: false }, contexts: {} },
    nodes: [node('http', 'declarative_http', {
      method: literal('POST'),
      url: literal(`${echoBaseUrl}/v1/chat/completions`),
      headers: {
        Authorization: literal('Bearer m5-model-secret'),
        'Content-Type': literal('application/json'),
      },
      body: {
        model: literal('contract-http'),
        messages: [
          { role: literal('system'), content: literal('Declarative HTTP contract') },
          { role: literal('user'), content: literal('Return the fixture response') },
        ],
      },
    })],
    connections: [
      { id: 'start-http', sourceNodeId: '__start__', sourceHandle: 'main', targetNodeId: 'http', targetHandle: 'main', order: 0 },
      { id: 'http-end', sourceNodeId: 'http', sourceHandle: 'main', targetNodeId: '__end__', targetHandle: 'main', order: 0 },
    ],
    end: {
      outputs: {
        status_code: { value: reference('outputs', ['statusCode'], 'http'), schema: { type: 'integer' }, required: true, sensitive: false },
        body_text: { value: reference('outputs', ['body'], 'http'), schema: { type: 'string' }, required: true, sensitive: false },
      },
    },
    settings: { executionOrder: 'deterministic' },
  }
  const workflow = await createWorkflow(page, token, `W4 Declarative HTTP ${suffix}`, definition)
  const application = await deployApplication(page, token, workflow.workflow, workflow.version, environment.id, `w4-http-${suffix}`)
  const invocation = await request<Invocation>(page, application.apiKey, `/workflows/${application.slug}/invoke`, 'POST', {
    input: {}, responseMode: 'sync',
  }, true, { 'Idempotency-Key': `w4-http-${suffix}` })
  const terminal = invocation.status === 'completed' ? invocation : await waitInvocation(page, application.apiKey, invocation.id)

  expect(terminal.status).toBe('completed')
  expect(terminal.outputs?.status_code).toBe(200)
  expect(terminal.outputs?.body_text).toBe('{"choices":[{"finish_reason":"stop","index":0,"message":{"content":"M5 Agent completed after the MCP tool result","role":"assistant"}}],"id":"m5-completion","model":"contract-http","object":"chat.completion","usage":{"completion_tokens":9,"prompt_tokens":24,"total_tokens":45}}')
  expect(terminal.executionId).toBeTruthy()
  let endBoundary: { spanId: string } | undefined
  await expect.poll(async () => {
    const trace = await request<{ spans: Array<{ spanId: string; spanKind: string; spanName: string; hasDetails: boolean }> }>(page, token, `/executions/${terminal.executionId}/trace?limit=100`)
    endBoundary = trace.spans.find((span) => span.spanKind === 'boundary' && span.spanName === 'End' && span.hasDetails)
    return endBoundary?.spanId
  }, { timeout: 60_000, intervals: [500, 1_000, 2_000] }).toBeTruthy()
  const endDetail = await request<{
    contents: Array<{ eventId: string; kind: string; preview?: { records?: Array<{ targetPath: string; sourceType: string; mode: string; resultBytes: number }> } }>
    events: Array<{ eventId: string; attributes?: { diagnostic?: string; conversionCount?: number } }>
  }>(page, token, `/executions/${terminal.executionId}/trace/spans/${endBoundary!.spanId}`)
  const conversion = endDetail.contents.find((content) => content.kind === 'conversion_record')
  expect(conversion).toBeTruthy()
  const conversionEvent = endDetail.events.find((event) => event.eventId === conversion!.eventId)
  expect(conversionEvent?.attributes).toMatchObject({ diagnostic: 'string_conversion', conversionCount: 1 })
  expect(conversion?.preview?.records).toEqual([
    expect.objectContaining({ sourceType: 'object', mode: 'reference', resultBytes: (terminal.outputs?.body_text as string).length }),
  ])
  const conversionJson = JSON.stringify({ content: conversion, event: conversionEvent })
  expect(conversionJson).not.toContain('m5-model-secret')
  expect(conversionJson).not.toContain('M5 Agent completed after the MCP tool result')
})

test('Workflow 5.0 turns concurrent Session Context CAS conflicts into a terminal failure', async ({ page }) => {
  const token = await login(page)
  const actor = await request<{ id: string }>(page, token, '/auth/me')
  const suffix = Date.now()
  const environment = (await request<Environment[]>(page, token, '/environments')).find((value) => value.code === 'development')!
  const definition: Definition = {
    schemaVersion: '6.0',
    start: {
      inputs: { type: 'object', properties: {}, additionalProperties: false },
      contexts: {
        session_counter: {
          schema: { type: 'number' }, default: 0, mutable: true, sensitive: false,
          scope: 'session', mergePolicy: 'reject_conflict', clientWritable: false,
        },
      },
    },
    nodes: [
      node('approval', 'approval', {
        title: `W4 Session CAS barrier ${suffix}`,
        timeoutMs: 300_000,
        candidateUserId: literal(actor.id),
      }),
      node('write', 'set', { values: { counter: reference('contexts', ['session_counter']) }, keepOnlySet: true }, {
        contextWrites: [{ operation: 'increment', path: 'session_counter', value: { kind: 'literal', value: 1 } }],
      }),
    ],
    connections: [
      { id: 'start-approval', sourceNodeId: '__start__', sourceHandle: 'main', targetNodeId: 'approval', targetHandle: 'main', order: 0 },
      { id: 'approval-write', sourceNodeId: 'approval', sourceHandle: 'approved', targetNodeId: 'write', targetHandle: 'main', order: 0 },
      { id: 'write-end', sourceNodeId: 'write', sourceHandle: 'main', targetNodeId: '__end__', targetHandle: 'main', order: 0 },
    ],
    end: { outputs: { counter: { value: reference('contexts', ['session_counter']), schema: { type: 'number' }, required: true, sensitive: false } } },
    settings: { executionOrder: 'parallel' },
  }
  const workflow = await createWorkflow(page, token, `W4 Session CAS ${suffix}`, definition)
  const application = await deployApplication(page, token, workflow.workflow, workflow.version, environment.id, `w4-session-${suffix}`)
  const session = await request<{ id: string }>(page, application.apiKey, `/applications/${application.slug}/sessions`, 'POST', {}, true, {
    'Idempotency-Key': `w4-session-create-${suffix}`,
  })
  let invocationIndex = 0
  const invoke = () => request<Invocation>(page, application.apiKey, `/workflows/${application.slug}/invoke`, 'POST', { input: {}, sessionId: session.id }, true, {
    'Idempotency-Key': `w4-session-${suffix}-${invocationIndex++}`,
  })
  const [first, second] = await Promise.all([invoke(), invoke()])
  expect(first.executionId).toBeTruthy()
  expect(second.executionId).toBeTruthy()
  const executionIds = new Set([first.executionId!, second.executionId!])
  let approvals: Approval[] = []
  await expect.poll(async () => {
    const result = await request<PageResponse<Approval>>(page, token, '/approvals?pageSize=100')
    approvals = result.items.filter((approval) => executionIds.has(approval.executionId))
    return approvals.length === 2 && approvals.every((approval) => approval.status === 'pending')
  }, { timeout: 180_000, intervals: [250, 500, 1_000, 2_000] }).toBeTruthy()
  for (const approval of approvals) {
    const claimed = await request<Approval>(page, token, `/approvals/${approval.id}/claim`, 'POST', { version: approval.version })
    await request<Approval>(page, token, `/approvals/${approval.id}/approve`, 'POST', { version: claimed.version, input: null })
  }
  const terminals = await Promise.all([waitInvocation(page, application.apiKey, first.id), waitInvocation(page, application.apiKey, second.id)])
  expect(terminals.map((value) => value.status).sort()).toEqual(['completed', 'failed'])
  expect(terminals.find((value) => value.status === 'failed')?.error?.primaryError?.code).toBe('SESSION_CONTEXT_VERSION_CONFLICT')
})

test('Workflow 5.0 propagates fail-fast, collected and Composite End errors', async ({ page }) => {
  const token = await login(page)
  const suffix = Date.now()
  const environment = (await request<Environment[]>(page, token, '/environments')).find((value) => value.code === 'development')!
  const terminalOutput = {
    reported_code: { value: reference('item', ['code']), schema: { type: 'string' }, required: true, sensitive: false },
  }
  const normalOutput = {
    success: { value: reference('outputs', [], 'normal'), schema: { type: 'object' }, required: true, sensitive: false },
  }
  const errorDefinition = (strategy: 'fail_fast' | 'collect', codes: string[]): Definition => ({
    schemaVersion: '6.0',
    start: { inputs: { type: 'object', properties: {}, additionalProperties: false }, contexts: {} },
    nodes: [
      node('normal', 'no_op', {}),
      ...codes.map((code, index) => node(`failure_${index}`, 'stop_and_error', { code, message: `${code} message` }, {
        settings: { onError: 'continue_error_output' },
      })),
    ],
    connections: [
      { id: 'start-normal', sourceNodeId: '__start__', sourceHandle: 'main', targetNodeId: 'normal', targetHandle: 'main', order: 0 },
      { id: 'normal-end', sourceNodeId: 'normal', sourceHandle: 'main', targetNodeId: '__end__', targetHandle: 'main', order: 0 },
      ...codes.flatMap((_, index) => [
        { id: `start-failure-${index}`, sourceNodeId: '__start__', sourceHandle: 'main', targetNodeId: `failure_${index}`, targetHandle: 'main', order: index + 1 },
        { id: `failure-${index}-end`, sourceNodeId: `failure_${index}`, sourceHandle: 'error', targetNodeId: '__end__', targetHandle: 'error', order: index },
      ]),
    ],
    end: { outputs: normalOutput, error: { strategy, collectWindowMs: 2_000, outputs: terminalOutput } },
    settings: { executionOrder: 'parallel' },
  })

  const invokeFailure = async (label: string, definition: Definition) => {
    const slugLabel = label.toLowerCase().replaceAll(' ', '-')
    const workflow = await createWorkflow(page, token, `W4 ${label} ${suffix}`, definition)
    const application = await deployApplication(page, token, workflow.workflow, workflow.version, environment.id, `w4-${slugLabel}-${suffix}`)
    const invocation = await request<Invocation>(page, application.apiKey, `/workflows/${application.slug}/invoke`, 'POST', { input: {}, responseMode: 'async' }, true, {
      'Idempotency-Key': `w4-${slugLabel}-${suffix}`,
    })
    return { workflow, terminal: await waitInvocation(page, application.apiKey, invocation.id) }
  }

  const failFast = await invokeFailure('Fail Fast', errorDefinition('fail_fast', ['FAIL_FAST']))
  expect(failFast.terminal).toMatchObject({
    status: 'failed',
    error: { primaryError: { code: 'FAIL_FAST' }, outputs: { reported_code: 'FAIL_FAST' } },
  })

  const collected = await invokeFailure('Collect', errorDefinition('collect', ['COLLECT_A', 'COLLECT_B']))
  expect(collected.terminal.status).toBe('failed')
  expect(collected.terminal.error?.errors).toHaveLength(2)
  expect((collected.terminal.error?.errors as Array<{ code: string }>).map((error) => error.code).sort()).toEqual(['COLLECT_A', 'COLLECT_B'])
  expect(collected.terminal.error?.outputs?.reported_code).toBe(collected.terminal.error?.primaryError?.code)

  const child = await createWorkflow(page, token, `W4 Error Child ${suffix}`, errorDefinition('fail_fast', ['CHILD_FAILED']))
  const childType = `workflow.${child.version.id.replaceAll('-', '')}`
  const parentDefinition: Definition = {
    schemaVersion: '6.0',
    start: { inputs: { type: 'object', properties: {}, additionalProperties: false }, contexts: {} },
    nodes: [node('child', childType, { workflowVersionId: child.version.id, inputs: {} }, {
      settings: { onError: 'continue_error_output' },
    })],
    connections: [
      { id: 'start-child', sourceNodeId: '__start__', sourceHandle: 'main', targetNodeId: 'child', targetHandle: 'main', order: 0 },
      { id: 'child-main-end', sourceNodeId: 'child', sourceHandle: 'main', targetNodeId: '__end__', targetHandle: 'main', order: 0 },
      { id: 'child-error-end', sourceNodeId: 'child', sourceHandle: 'error', targetNodeId: '__end__', targetHandle: 'error', order: 0 },
    ],
    end: {
      outputs: {
        success: { value: reference('outputs', ['success'], 'child'), schema: { type: 'object' }, required: true, sensitive: false },
      },
      error: { strategy: 'fail_fast', collectWindowMs: 5_000, outputs: terminalOutput },
    },
    settings: { executionOrder: 'parallel' },
  }
  const parent = await invokeFailure('Composite Error', parentDefinition)
  expect(parent.terminal).toMatchObject({ status: 'failed', error: { primaryError: { code: 'CHILD_FAILED' } } })
  const parentExecution = await waitExecution(page, token, parent.terminal.executionId!, ['failed'])
  const executions = await request<ExecutionPage>(page, token, '/executions?limit=100')
  expect(executions.items).toContainEqual(expect.objectContaining({ parentExecutionId: parentExecution.id, status: 'failed' }))
})
