import { describe, expect, it } from 'vitest'

import type { StudioEdge, StudioNode } from '../model/types'
import { cloneStudioFragment, readStudioClipboard, writeStudioClipboard } from './studio-clipboard'

describe('Studio clipboard', () => {
  it('persists across Workflow pages and regenerates node, binding, and edge identities', () => {
    const action = actionNode('agent-1')
    const binding = bindingNode('binding-1')
    const edge: StudioEdge = { id: 'edge-1', source: binding.id, target: action.id, sourceHandle: 'resource', targetHandle: 'binding:mcp_tools', type: 'studio', data: { edgeKind: 'binding', targetSlot: 'mcp_tools' } }
    writeStudioClipboard('workflow-source', [action, binding], [edge])

    const stored = readStudioClipboard()
    expect(stored?.sourceWorkflowId).toBe('workflow-source')
    const [nodes, edges] = cloneStudioFragment(stored!)
    expect(nodes.map((node) => node.id)).not.toContain(action.id)
    expect(nodes.map((node) => node.id)).not.toContain(binding.id)
    const clonedBinding = nodes.find((node) => node.data.editorKind === 'binding')!
    const clonedAgent = nodes.find((node) => node.data.editorKind === 'action')!
    expect(clonedBinding.data.editorKind === 'binding' && clonedBinding.data.bindingId).not.toBe('binding-1')
    expect(clonedAgent.data.editorKind === 'action' && clonedAgent.data.parameters.sessionPolicy).toEqual({ mode: 'invocation' })
    expect(clonedAgent.data.editorKind === 'action' && clonedAgent.data.resourceReferences).toEqual([
      expect.objectContaining({ resourceType: 'model', resourceVersionId: 'model-version-1' }),
      expect.objectContaining({ resourceType: 'sandbox_profile', resourceVersionId: 'sandbox-version-1' }),
    ])
    expect(clonedAgent.data.editorKind === 'action' && clonedAgent.data.resourceReferences.every((reference) => reference.bindingId === undefined)).toBe(true)
    expect(edges[0].id).not.toBe(edge.id)
    expect(edges[0]).toMatchObject({ source: clonedBinding.id, target: nodes.find((node) => node.data.editorKind === 'action')?.id })
  })
})

function actionNode(id: string): StudioNode { return { id, type: 'manifest', position: { x: 0, y: 0 }, data: { editorKind: 'action', nodeType: 'agent', typeVersion: 2, label: 'Agent', key: 'agent', parameters: { sessionPolicy: { mode: 'invocation' } }, outputProjection: {}, contextWrites: [], resourceReferences: [{ resourceType: 'model', resourceId: 'model-1', resourceVersionId: 'model-version-1', operation: 'use' }, { resourceType: 'sandbox_profile', resourceId: 'sandbox-1', resourceVersionId: 'sandbox-version-1', operation: 'use' }], settings: {}, disabled: false } } }
function bindingNode(bindingId: string): StudioNode { return { id: `binding:${bindingId}`, type: 'attachment', position: { x: 0, y: 120 }, data: { editorKind: 'binding', bindingId, bindingRole: 'mcp_tools', resourceType: 'mcp_tool', resourceId: 'tool-1', operation: 'use', label: 'MCP Tool' } } }
