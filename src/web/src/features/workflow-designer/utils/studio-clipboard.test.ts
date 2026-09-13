import { describe, expect, it } from 'vitest'

import type { StudioEdge, StudioNode } from '../model/types'
import { cloneStudioFragment, readStudioClipboard, writeStudioClipboard } from './studio-clipboard'

describe('Studio clipboard', () => {
  it('persists across Workflow pages and regenerates node and edge identities', () => {
    const source = actionNode('agent-1')
    const other = actionNode('agent-2', 480, 120)
    const edge: StudioEdge = { id: 'edge-1', source: source.id, target: other.id, sourceHandle: 'main', targetHandle: 'main', type: 'studio', data: { edgeKind: 'execution' } }
    writeStudioClipboard('workflow-source', [source, other], [edge])

    const stored = readStudioClipboard()
    expect(stored?.sourceWorkflowId).toBe('workflow-source')
    const [nodes, edges] = cloneStudioFragment(stored!)
    expect(nodes.map((node) => node.id)).not.toContain(source.id)
    expect(nodes.map((node) => node.id)).not.toContain(other.id)
    const clonedAgent = nodes.find((node) => node.data.editorKind === 'action')!
    expect(clonedAgent.data.editorKind === 'action' && clonedAgent.data.parameters.sessionPolicy).toEqual({ mode: 'invocation' })
    expect(nodes.map((node) => node.position)).toEqual([{ x: 36, y: 36 }, { x: 516, y: 156 }])
    expect(edges[0].id).not.toBe(edge.id)
    expect(edges[0]).toMatchObject({ source: nodes[0].id, target: nodes[1].id, sourceHandle: 'main', targetHandle: 'main' })
  })
})

function actionNode(id: string, x = 0, y = 0): StudioNode { return { id, type: 'manifest', position: { x, y }, data: { editorKind: 'action', nodeType: 'agent', typeVersion: 2, label: 'Agent', key: 'agent', parameters: { sessionPolicy: { mode: 'invocation' } }, contextWrites: [], resourceReferences: [{ bindingRole: 'model', resourceType: 'model', resourceId: 'model-1', resourceVersionId: 'model-version-1', operation: 'use' }], settings: {}, disabled: false } } }
