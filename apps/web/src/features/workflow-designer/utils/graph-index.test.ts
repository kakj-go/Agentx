import { describe, expect, it } from 'vitest'

import type { NodeManifest, ResourceReference, StudioEdge, StudioNode } from '../model/types'
import { inspectConnection } from './connections'
import { createGraphIndex, IncrementalGraphIndex, occupiedHandlesByNodeId, portKey, resolveIndexedPort } from './graph-index'

const manifest = (nodeType: string, variadic = false): NodeManifest => ({
  protocolVersion: '2.0', nodeType, version: 1, displayName: nodeType, description: '', category: 'actions', keywords: [], iconKey: 'box', executionStyle: 'action', capability: 'builtin', readiness: 'any',
  inputPorts: [{ name: 'main', kind: 'main', required: false, variadic }], outputPorts: [{ name: 'main', kind: 'main', required: false, variadic: true }, { name: 'error', kind: 'error', required: false, variadic: true }],
  bindingSlots: nodeType === 'agent' ? [{ name: 'mcp_tools', resourceType: 'mcp_tool', placement: 'inspector', required: false, multiple: true }] : [], parameterSchema: {}, uiSchema: {}, providers: [], credentials: [],
  retryPolicy: { retryable: false, maxAttempts: 1, initialBackoffMs: 0, maxBackoffMs: 0 }, sandboxRequired: false, supportsMock: true, sideEffectLevel: 'none',
})

const action = (id: string, nodeType = 'set', resourceReferences: ResourceReference[] = []): StudioNode => ({ id, type: 'manifest', position: { x: 0, y: 0 }, data: { editorKind: 'action', nodeType, typeVersion: 1, label: id, key: id, parameters: {}, contextWrites: [], resourceReferences, settings: {}, disabled: false } })
const edge = (id: string, source: string, target: string, sourceHandle = 'main', targetHandle = 'main'): StudioEdge => ({ id, source, target, sourceHandle, targetHandle, type: 'studio', data: { edgeKind: 'execution' } })

describe('GraphIndex connection validation', () => {
  it('indexes nodes, ports, edges, and dynamic main handles', () => {
    const references: ResourceReference[] = [{ bindingRole: 'mcp_tools', resourceType: 'mcp_tool', resourceId: 'tool-1', operation: 'use' }]
    const nodes = [action('source'), action('target', 'agent', references)]
    const edges = [edge('flow', 'source', 'target')]
    const manifests = new Map([['set@1', manifest('set')], ['agent@1', manifest('agent')]])
    const index = createGraphIndex(nodes, edges, manifests)

    expect(index.nodeById.size).toBe(2)
    expect(index.edgesBySourcePort.get(portKey('source', 'main'))).toHaveLength(1)
    expect(occupiedHandlesByNodeId(index).get('source')).toBe('main')
    expect(occupiedHandlesByNodeId(index).get('target')).toBeUndefined()
    expect(resolveIndexedPort(index, 'target', 'main:3', 'input')?.port.name).toBe('main')
  })

  it('normalizes variadic branch handles (case:, decision:) to their declared ports', () => {
    const approval = manifest('approval')
    approval.outputPorts = [{ name: 'decision', kind: 'main', required: false, variadic: true }, { name: 'timed_out', kind: 'main', required: false, variadic: false }, { name: 'error', kind: 'error', required: false, variadic: false }]
    const index = createGraphIndex([action('gate', 'approval')], [], new Map([['approval@1', approval]]))

    expect(resolveIndexedPort(index, 'gate', 'case:c1', 'output')).toBeUndefined()
    expect(resolveIndexedPort(index, 'gate', 'decision:approved', 'output')?.port.name).toBe('decision')
    expect(resolveIndexedPort(index, 'gate', 'timed_out', 'output')?.port.name).toBe('timed_out')
  })

  it('derives attachment summaries from the node resource references', () => {
    const references: ResourceReference[] = [
      { bindingRole: 'mcp_tools', resourceType: 'mcp_tool', resourceId: 'tool-1', operation: 'use' },
      { bindingRole: 'knowledge', resourceType: 'rag', resourceId: 'sop-1', operation: 'read' },
      { resourceType: 'model', resourceId: 'gpt-4o', resourceVersionId: 'v1', operation: 'use' },
    ]
    const nodes = [action('agent', 'agent', references)]
    const manifests = new Map([['agent@1', manifest('agent')]])
    const controller = new IncrementalGraphIndex()
    const index = controller.sync(nodes, [], manifests)

    expect(index.bindingSummaryByNodeId.get('agent')).toEqual([
      { role: 'mcp_tools', resourceType: 'mcp_tool', label: 'tool-1' },
      { role: 'knowledge', resourceType: 'rag', label: 'sop-1' },
    ])

    controller.sync([action('agent', 'agent', [])], [], manifests)
    expect(index.bindingSummaryByNodeId.has('agent')).toBe(false)
  })

  it('rejects self, duplicate, and incompatible connections and marks occupied inputs for replacement', () => {
    const nodes = [action('source'), action('other'), action('target')]
    const edges = [edge('existing', 'source', 'target')]
    const manifests = new Map([['set@1', manifest('set')]])
    const index = createGraphIndex(nodes, edges, manifests)

    expect(inspectConnection({ source: 'source', sourceHandle: 'main', target: 'source', targetHandle: 'main' }, index)).toMatchObject({ status: 'invalid', reason: 'self_connection' })
    expect(inspectConnection({ source: 'source', sourceHandle: 'main', target: 'target', targetHandle: 'main' }, index)).toMatchObject({ status: 'invalid', reason: 'duplicate_connection' })
    expect(inspectConnection({ source: 'source', sourceHandle: 'error', target: 'target', targetHandle: 'main' }, index)).toMatchObject({ status: 'invalid', reason: 'incompatible_port' })
    expect(resolveIndexedPort(index, 'other', 'main', 'output')).toMatchObject({ direction: 'output', port: { kind: 'main' } })
    expect(resolveIndexedPort(index, 'target', 'main', 'input')).toMatchObject({ direction: 'input', port: { kind: 'main' } })
    const occupied = inspectConnection({ source: 'other', sourceHandle: 'main', target: 'target', targetHandle: 'main' }, index)
    expect(occupied.reason).toBeUndefined()
    expect(occupied).toMatchObject({ status: 'occupied', replaceEdge: { id: 'existing' } })
  })

  it('updates only changed structures and ignores position frames', () => {
    const nodes = [action('source'), action('target'), action('unrelated')]
    const manifests = new Map([['set@1', manifest('set')]])
    const controller = new IncrementalGraphIndex()
    const index = controller.sync(nodes, [edge('first', 'source', 'target')], manifests)
    const sourcePort = resolveIndexedPort(index, 'source', 'main', 'output')
    const unrelatedPort = resolveIndexedPort(index, 'unrelated', 'main', 'input')
    const unrelatedTargets = index.edgesByTargetPort.get(portKey('target', 'main'))

    const moved = [{ ...nodes[0], position: { x: 400, y: 120 } }, nodes[1], nodes[2]]
    expect(controller.sync(moved, [edge('first', 'source', 'target')], manifests)).toBe(index)
    expect(resolveIndexedPort(index, 'source', 'main', 'output')).toBe(sourcePort)
    expect(resolveIndexedPort(index, 'unrelated', 'main', 'input')).toBe(unrelatedPort)
    expect(index.edgesByTargetPort.get(portKey('target', 'main'))).toBe(unrelatedTargets)

    controller.sync(moved, [edge('first', 'source', 'target'), edge('second', 'source', 'unrelated')], manifests)
    expect(index.edgesBySourcePort.get(portKey('source', 'main'))).toHaveLength(2)
    expect(resolveIndexedPort(index, 'unrelated', 'main', 'input')).toBe(unrelatedPort)
  })
})
