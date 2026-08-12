import { describe, expect, it } from 'vitest'

import type { StudioEdge, StudioNode } from '../model/types'
import { cloneStudioFragment, readStudioClipboard, writeStudioClipboard } from './studio-clipboard'

describe('Studio clipboard', () => {
  it('persists across Workflow pages and regenerates node, binding, and edge identities', () => {
    const action = actionNode('agent-1')
    const binding = bindingNode('binding-1')
    const edge: StudioEdge = { id: 'edge-1', source: binding.id, target: action.id, sourceHandle: 'resource', targetHandle: 'binding:ai_model', type: 'studio', data: { edgeKind: 'binding', targetSlot: 'ai_model' } }
    writeStudioClipboard('workflow-source', [action, binding], [edge])

    const stored = readStudioClipboard()
    expect(stored?.sourceWorkflowId).toBe('workflow-source')
    const [nodes, edges] = cloneStudioFragment(stored!)
    expect(nodes.map((node) => node.id)).not.toContain(action.id)
    expect(nodes.map((node) => node.id)).not.toContain(binding.id)
    const clonedBinding = nodes.find((node) => node.data.editorKind === 'binding')!
    expect(clonedBinding.data.editorKind === 'binding' && clonedBinding.data.bindingId).not.toBe('binding-1')
    expect(edges[0].id).not.toBe(edge.id)
    expect(edges[0]).toMatchObject({ source: clonedBinding.id, target: nodes.find((node) => node.data.editorKind === 'action')?.id })
  })
})

function actionNode(id: string): StudioNode { return { id, type: 'manifest', position: { x: 0, y: 0 }, data: { editorKind: 'action', nodeType: 'agent', typeVersion: 1, label: 'Agent', key: 'agent', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false } } }
function bindingNode(bindingId: string): StudioNode { return { id: `binding:${bindingId}`, type: 'attachment', position: { x: 0, y: 120 }, data: { editorKind: 'binding', bindingId, bindingRole: 'ai_model', resourceType: 'model', resourceId: 'model-1', operation: 'use', label: 'Model' } } }
