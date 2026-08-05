import * as Tooltip from '@radix-ui/react-tooltip'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { ToastProvider } from '../../shared/ui/toast'
import { deserializeDraft, serializeStudio } from './model/serializer'
import type { NodeManifest, WorkflowDefinition } from './model/types'
import { useEditorStore } from './store/editor-store'
import { WorkflowCanvas } from './workflow-canvas'

const editorDocument = { nodeLayouts: [{ nodeId: 'trigger', x: 100, y: 100 }], bindingLayouts: [], edges: [], bindingEdges: [], annotations: [], groups: [], viewport: { x: 0, y: 0, zoom: 1 } }
const draft = {
  id: 'draft-1', workflowId: 'workflow-1', revision: 1, schemaVersion: '3.0', definitionHash: 'sha256:def', editorHash: 'sha256:editor', updatedAt: '2026-08-02T10:00:00Z', editorDocument,
  definition: { schemaVersion: '3.0', settings: { executionOrder: 'deterministic', activationBudget: 10000 }, nodes: [{ id: 'trigger', type: 'manual_trigger', typeVersion: 1, name: 'Manual Trigger', disabled: false, parameters: {}, resourceReferences: [], settings: {} }], connections: [] },
}

let requiredAgentBinding = false

const manifest = (nodeType: string, executionStyle = 'action'): NodeManifest => ({
  protocolVersion: '1.0', nodeType, version: 1, displayName: nodeType === 'agent' ? 'Agent' : 'Manual Trigger', description: '', category: nodeType === 'agent' ? 'ai' : 'triggers', keywords: [nodeType], iconKey: nodeType === 'agent' ? 'bot' : 'mouse-pointer-click', executionStyle: executionStyle as NodeManifest['executionStyle'], capability: nodeType === 'agent' ? 'agent' : 'builtin', readiness: 'any', inputPorts: nodeType === 'agent' ? [{ name: 'main', kind: 'main', required: true, variadic: false }] : [], outputPorts: [{ name: 'main', kind: 'main', required: false, variadic: false }], bindingSlots: nodeType === 'agent' ? [{ name: 'ai_model', resourceType: 'model', required: requiredAgentBinding, multiple: false }] : [], parameterSchema: { type: 'object', properties: {} }, uiSchema: {}, providers: [], credentials: [], retryPolicy: { retryable: false, maxAttempts: 1, initialBackoffMs: 0, maxBackoffMs: 0 }, sandboxRequired: false, supportsMock: true, sideEffectLevel: 'none',
})

function json(value: unknown, status = 200) { return new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }) }

describe('workflow studio shell', () => {
  beforeEach(() => {
    requiredAgentBinding = false
    localStorage.clear()
    useEditorStore.setState({ nodes: [], edges: [], viewport: { x: 0, y: 0, zoom: 1 }, annotations: [], groups: [], dirty: false, past: [], future: [], selectedId: undefined })
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = new URL(String(input), 'http://agentx.test').pathname
      if (path.endsWith('/draft') && init?.method === 'PUT') return json({ code: 'DRAFT_REVISION_CONFLICT', message: 'Draft changed', requestId: 'request-1' }, 409)
      if (path.endsWith('/draft')) return json(draft)
      if (path.endsWith('/node-definitions')) return json({ items: [{ nodeType: 'manual_trigger', version: 1 }, { nodeType: 'agent', version: 1 }] })
      if (path.includes('/node-definitions/manual_trigger/')) return json({ nodeType: 'manual_trigger', version: 1, manifest: manifest('manual_trigger', 'trigger') })
      if (path.includes('/node-definitions/agent/')) return json({ nodeType: 'agent', version: 1, manifest: manifest('agent') })
      if (path.endsWith('/workflows/workflow-1')) return json({ id: 'workflow-1', name: 'Conflict Workflow' })
      if (path.endsWith('/environments') || path.endsWith('/versions') || path.endsWith('/mcp/tools')) return json([])
      return json({ items: [], page: 1, pageSize: 100, total: 0 })
    }))
  })

  it('keeps local edits and offers all explicit conflict choices', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><Tooltip.Provider><ToastProvider><MemoryRouter initialEntries={['/workflows/workflow-1/editor']}><Routes><Route element={<WorkflowCanvas />} path="/workflows/:workflowId/editor" /></Routes></MemoryRouter></ToastProvider></Tooltip.Provider></QueryClientProvider>)

    fireEvent.click(await screen.findByRole('button', { name: /Agent/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))

    expect((await screen.findAllByText('Draft revision conflict')).length).toBeGreaterThan(0)
    expect(screen.getByRole('button', { name: 'Keep local copy' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Load server revision' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Overwrite server draft' })).toBeInTheDocument()
    await waitFor(() => expect(screen.getByText(/Unsaved/)).toBeInTheDocument())
  })

  it('keeps incomplete autosave validation non-blocking but reports it on explicit save', async () => {
    requiredAgentBinding = true
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><Tooltip.Provider><ToastProvider><MemoryRouter initialEntries={['/workflows/workflow-1/editor']}><Routes><Route element={<WorkflowCanvas />} path="/workflows/:workflowId/editor" /></Routes></MemoryRouter></ToastProvider></Tooltip.Provider></QueryClientProvider>)

    fireEvent.click(await screen.findByRole('button', { name: /Agent/ }))
    await new Promise((resolve) => window.setTimeout(resolve, 1600))
    expect(screen.queryByRole('dialog', { name: 'Validation failed' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    expect(await screen.findByRole('dialog', { name: 'Validation failed' })).toBeInTheDocument()
  })
})

describe('workflow studio serializer', () => {
  it('keeps layouts outside Definition 3.0 and compiles AI attachments into agent references', () => {
    const definition: WorkflowDefinition = {
      schemaVersion: '3.0', settings: { executionOrder: 'deterministic', activationBudget: 10000 }, connections: [],
      nodes: [{ id: 'agent-1', type: 'agent', typeVersion: 1, name: 'Agent', disabled: false, parameters: {}, settings: {}, resourceReferences: [{ bindingId: 'binding-1', bindingRole: 'ai_model', resourceType: 'model', resourceId: 'model-1', resourceVersionId: 'model-version-1', operation: 'use' }] }],
    }
    const source = { ...draft, definition, editorDocument: { ...editorDocument, nodeLayouts: [{ nodeId: 'agent-1', x: 410, y: 230 }], bindingLayouts: [{ bindingId: 'binding-1', x: 390, y: 410 }], bindingEdges: [{ edgeId: 'binding-edge-1', sourceBindingId: 'binding-1', targetNodeId: 'agent-1', targetSlot: 'ai_model' }] } }
    const serialized = serializeStudio(deserializeDraft(source))

    expect(serialized.definition.schemaVersion).toBe('3.0')
    expect(serialized.definition.nodes).toHaveLength(1)
    expect(serialized.definition.nodes[0]).not.toHaveProperty('position')
    expect(serialized.definition.nodes[0].resourceReferences[0]).toMatchObject({ bindingId: 'binding-1', bindingRole: 'ai_model', resourceId: 'model-1' })
    expect(serialized.editorDocument.bindingLayouts[0]).toMatchObject({ bindingId: 'binding-1', x: 390, y: 410 })
    expect(serialized.editorDocument.bindingEdges[0]).toMatchObject({ targetSlot: 'ai_model' })
  })
})
