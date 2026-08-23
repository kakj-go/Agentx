import * as Tooltip from '@radix-ui/react-tooltip'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { ToastProvider } from '../../shared/ui/toast'
import { i18n } from '../../app/i18n'
import { deserializeDraft, serializeStudio } from './model/serializer'
import type { NodeManifest, WorkflowDefinition } from './model/types'
import { useEditorStore } from './store/editor-store'
import { WorkflowCanvas } from './workflow-canvas'

const editorDocument = { nodeLayouts: [{ nodeId: 'trigger', x: 100, y: 100 }], boundaryLayouts: [], bindingLayouts: [], edges: [], bindingEdges: [], annotations: [], groups: [], viewport: { x: 0, y: 0, zoom: 1 } }
const draft = {
  id: 'draft-1', workflowId: 'workflow-1', revision: 1, schemaVersion: '5.0', definitionHash: 'sha256:def', editorHash: 'sha256:editor', updatedAt: '2026-08-02T10:00:00Z', editorDocument,
  definition: { schemaVersion: '5.0', start: { inputs: {}, contexts: {} }, settings: { executionOrder: 'deterministic', activationBudget: 10000 }, nodes: [{ id: 'trigger', key: 'source', type: 'set', typeVersion: 1, name: 'Set', disabled: false, parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {} }], connections: [], end: { outputs: {}, error: { strategy: 'fail_fast', collectWindowMs: 5000, outputs: {} } } },
}

let requiredAgentBinding = false
let saveConflict = false

const manifest = (nodeType: string, executionStyle = 'action'): NodeManifest => ({
  protocolVersion: '1.0', nodeType, version: 1, displayName: nodeType === 'agent' ? 'Agent' : 'Manual Trigger', description: '', category: nodeType === 'agent' ? 'ai' : 'triggers', keywords: [nodeType], iconKey: nodeType === 'agent' ? 'bot' : 'mouse-pointer-click', executionStyle: executionStyle as NodeManifest['executionStyle'], capability: nodeType === 'agent' ? 'agent' : 'builtin', readiness: 'any', inputPorts: nodeType === 'agent' ? [{ name: 'main', kind: 'main', required: true, variadic: false }] : [], outputPorts: [{ name: 'main', kind: 'main', required: false, variadic: false }], bindingSlots: nodeType === 'agent' ? [{ name: 'ai_model', resourceType: 'model', required: requiredAgentBinding, multiple: false }] : [], parameterSchema: { type: 'object', properties: {} }, uiSchema: { canvas: { role: nodeType === 'agent' ? 'agent' : 'trigger' } }, providers: [], credentials: [], retryPolicy: { retryable: false, maxAttempts: 1, initialBackoffMs: 0, maxBackoffMs: 0 }, sandboxRequired: false, supportsMock: true, sideEffectLevel: 'none',
})

function json(value: unknown, status = 200) { return new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }) }

describe('workflow studio shell', () => {
  beforeEach(() => {
    requiredAgentBinding = false
    saveConflict = false
    localStorage.clear()
    useEditorStore.setState({ nodes: [], edges: [], viewport: { x: 0, y: 0, zoom: 1 }, annotations: [], groups: [], settings: { executionOrder: 'deterministic', activationBudget: 10_000 }, dirty: false, past: [], future: [], selectedId: undefined })
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = new URL(String(input), 'http://agentx.test').pathname
      if (path.endsWith('/draft') && init?.method === 'PUT') {
        if (saveConflict) return json({ code: 'DRAFT_REVISION_CONFLICT', message: 'Draft changed', requestId: 'request-1' }, 409)
        return json({ ...draft, revision: draft.revision + 1 })
      }
      if (path.endsWith('/draft')) return json(draft)
      if (path.endsWith('/node-definitions')) return json({ items: [{ nodeType: 'set', version: 1 }, { nodeType: 'agent', version: 1 }] })
      if (path.includes('/node-definitions/set/')) return json({ nodeType: 'set', version: 1, manifest: manifest('set') })
      if (path.includes('/node-definitions/agent/')) return json({ nodeType: 'agent', version: 1, manifest: manifest('agent') })
      if (path.endsWith('/workflows/workflow-1')) return json({ id: 'workflow-1', name: 'Conflict Workflow' })
      if (path.endsWith('/environments') || path.endsWith('/versions') || path.endsWith('/mcp/tools')) return json([])
      return json({ items: [], page: 1, pageSize: 100, total: 0 })
    }))
  })

  it('keeps local edits and offers all explicit conflict choices', async () => {
    saveConflict = true
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><Tooltip.Provider><ToastProvider><MemoryRouter initialEntries={['/workflows/workflow-1/editor']}><Routes><Route element={<WorkflowCanvas />} path="/workflows/:workflowId/editor" /></Routes></MemoryRouter></ToastProvider></Tooltip.Provider></QueryClientProvider>)

    fireEvent.click(await screen.findByRole('button', { name: 'Search nodes' }))
    fireEvent.change(screen.getByRole('textbox', { name: 'Search nodes' }), { target: { value: 'agent' } })
    fireEvent.click(screen.getByTestId('palette-action-agent'))
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))

    expect((await screen.findAllByText('Draft revision conflict')).length).toBeGreaterThan(0)
    expect(screen.getByRole('button', { name: 'Keep local copy' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Load server revision' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Overwrite server draft' })).toBeInTheDocument()
    await waitFor(() => expect(screen.getByText(/Unsaved/)).toBeInTheDocument())
  })

  it('saves an incomplete draft and defers strict validation until execution', async () => {
    requiredAgentBinding = true
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><Tooltip.Provider><ToastProvider><MemoryRouter initialEntries={['/workflows/workflow-1/editor']}><Routes><Route element={<WorkflowCanvas />} path="/workflows/:workflowId/editor" /></Routes></MemoryRouter></ToastProvider></Tooltip.Provider></QueryClientProvider>)

    fireEvent.click(await screen.findByRole('button', { name: 'Search nodes' }))
    fireEvent.change(screen.getByRole('textbox', { name: 'Search nodes' }), { target: { value: 'agent' } })
    fireEvent.click(screen.getByTestId('palette-action-agent'))
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(screen.getByText(/Saved|已保存/)).toBeInTheDocument())
    expect(screen.queryByRole('dialog', { name: 'Validation failed' })).not.toBeInTheDocument()
  })

  it('does not submit or autosave a structurally invalid definition', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><Tooltip.Provider><ToastProvider><MemoryRouter initialEntries={['/workflows/workflow-1/editor']}><Routes><Route element={<WorkflowCanvas />} path="/workflows/:workflowId/editor" /></Routes></MemoryRouter></ToastProvider></Tooltip.Provider></QueryClientProvider>)

    await waitFor(() => expect(useEditorStore.getState().nodes).toHaveLength(1))
    vi.mocked(fetch).mockClear()
    act(() => {
      useEditorStore.getState().setEnd({
        outputs: { 'Invalid-name': { value: { kind: 'literal', value: '' }, schema: { type: 'string' }, required: false, sensitive: false } },
        error: { strategy: 'fail_fast', collectWindowMs: 5000, outputs: {} },
      })
    })
    const save = screen.getByRole('button', { name: 'Save' })
    await waitFor(() => expect(save).toBeEnabled())
    fireEvent.click(save)

    expect(await screen.findByText('INVALID_END_OUTPUT_KEY')).toBeInTheDocument()
    await new Promise((resolve) => window.setTimeout(resolve, 1600))
    const draftWrites = vi.mocked(fetch).mock.calls.filter(([input, init]) => new URL(String(input), 'http://agentx.test').pathname.endsWith('/draft') && init?.method === 'PUT')
    expect(draftWrites).toHaveLength(0)
  })

  it('locates a validation issue in the selected node details', async () => {
    requiredAgentBinding = true
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><Tooltip.Provider><ToastProvider><MemoryRouter initialEntries={['/workflows/workflow-1/editor']}><Routes><Route element={<WorkflowCanvas />} path="/workflows/:workflowId/editor" /></Routes></MemoryRouter></ToastProvider></Tooltip.Provider></QueryClientProvider>)

    fireEvent.click(await screen.findByRole('button', { name: 'Search nodes' }))
    fireEvent.change(screen.getByRole('textbox', { name: 'Search nodes' }), { target: { value: 'agent' } })
    fireEvent.click(screen.getByTestId('palette-action-agent'))
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(screen.getByText(/Saved|已保存/)).toBeInTheDocument())
    fireEvent.click(screen.getAllByRole('button', { name: 'Run' })[0])
    const issue = await screen.findByText('AI_BINDING_REQUIRED')
    expect(issue.closest('button')).toHaveTextContent('Connect the required ai_model attachment.')

    await i18n.changeLanguage('zh-CN')
    await waitFor(() => expect(issue.closest('button')).toHaveTextContent('请连接必需的 ai_model 附件。'))
    fireEvent.click(issue.closest('button')!)

    const field = document.querySelector<HTMLElement>('[data-field-path="resourceReferences.ai_model"]')
    await waitFor(() => expect(field).toHaveFocus())
    expect(field).toHaveClass('studio-field-located')
    await i18n.changeLanguage('en-US')
  })

})

describe('workflow studio serializer', () => {
  it('keeps layouts outside Definition 4.0 and compiles AI attachments into agent references', () => {
    const definition: WorkflowDefinition = {
      schemaVersion: '5.0', start: { inputs: {}, contexts: {} }, settings: { executionOrder: 'deterministic', activationBudget: 10000 }, connections: [], end: { outputs: {}, error: { strategy: 'fail_fast', collectWindowMs: 5000, outputs: {} } },
      nodes: [{ id: 'agent-1', key: 'agent', type: 'agent', typeVersion: 1, name: 'Agent', disabled: false, parameters: {}, outputProjection: {}, contextWrites: [], settings: {}, resourceReferences: [{ bindingId: 'binding-1', bindingRole: 'ai_model', resourceType: 'model', resourceId: 'model-1', resourceVersionId: 'model-version-1', operation: 'use' }] }],
    }
    const source = { ...draft, definition, editorDocument: { ...editorDocument, nodeLayouts: [{ nodeId: 'agent-1', x: 410, y: 230 }], bindingLayouts: [{ bindingId: 'binding-1', x: 390, y: 410 }], bindingEdges: [{ edgeId: 'binding-edge-1', sourceBindingId: 'binding-1', targetNodeId: 'agent-1', targetSlot: 'ai_model' }] } }
    const serialized = serializeStudio(deserializeDraft(source))

    expect(serialized.definition.schemaVersion).toBe('5.0')
    expect(serialized.definition.nodes).toHaveLength(1)
    expect(serialized.definition.nodes[0]).not.toHaveProperty('position')
    expect(serialized.definition.nodes[0].resourceReferences[0]).toMatchObject({ bindingId: 'binding-1', bindingRole: 'ai_model', resourceId: 'model-1' })
    expect(serialized.editorDocument.bindingLayouts[0]).toMatchObject({ bindingId: 'binding-1', x: 390, y: 410 })
    expect(serialized.editorDocument.bindingEdges[0]).toMatchObject({ targetSlot: 'ai_model' })
  })

  it('omits incomplete attachment nodes from autosave until they form a resource binding', () => {
    const document = deserializeDraft(draft)
    document.nodes.push({
      id: 'binding:pending-model',
      type: 'attachment',
      position: { x: 390, y: 410 },
      data: { editorKind: 'binding', bindingId: 'pending-model', bindingRole: 'ai_model', resourceType: 'model', operation: 'use', label: 'model' },
    })

    const serialized = serializeStudio(document)

    expect(serialized.definition.nodes[0].resourceReferences).toEqual([])
    expect(serialized.editorDocument.bindingLayouts).toEqual([])
    expect(serialized.editorDocument.bindingEdges).toEqual([])
  })

  it('normalizes connection order independently for every source port', () => {
    const document = deserializeDraft({
      ...draft,
      definition: {
        ...draft.definition,
        nodes: [
          ...draft.definition.nodes,
          { ...draft.definition.nodes[0], id: 'a' },
          { ...draft.definition.nodes[0], id: 'b' },
          { ...draft.definition.nodes[0], id: 'c' },
        ],
        connections: [
          { id: 'main-b', sourceNodeId: 'trigger', sourceHandle: 'main', targetNodeId: 'b', targetHandle: 'main', order: 8 },
          { id: 'error-a', sourceNodeId: 'trigger', sourceHandle: 'error', targetNodeId: 'a', targetHandle: 'main', order: 9 },
          { id: 'main-c', sourceNodeId: 'trigger', sourceHandle: 'main', targetNodeId: 'c', targetHandle: 'main', order: 2 },
        ],
      },
      editorDocument: { ...editorDocument, nodeLayouts: ['trigger', 'a', 'b', 'c'].map((nodeId, index) => ({ nodeId, x: index * 100, y: 100 })) },
    })
    const connections = serializeStudio(document).definition.connections
    expect(connections.find((edge) => edge.id === 'main-c')?.order).toBe(0)
    expect(connections.find((edge) => edge.id === 'main-b')?.order).toBe(1)
    expect(connections.find((edge) => edge.id === 'error-a')?.order).toBe(0)
  })
})
