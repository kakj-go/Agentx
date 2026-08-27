import { describe, expect, it } from 'vitest'

import type { NodeManifest, StudioEdge, StudioNode } from '../model/types'
import { inspectConnection } from './connections'
import { createGraphIndex, IncrementalGraphIndex, occupiedHandlesByNodeId, portKey, resolveIndexedPort } from './graph-index'

const manifest = (nodeType: string, variadic = false): NodeManifest => ({
  protocolVersion: '2.0', nodeType, version: 1, displayName: nodeType, description: '', category: 'actions', keywords: [], iconKey: 'box', executionStyle: 'action', capability: 'builtin', readiness: 'any',
  inputPorts: [{ name: 'main', kind: 'main', required: false, variadic }], outputPorts: [{ name: 'main', kind: 'main', required: false, variadic: true }, { name: 'error', kind: 'error', required: false, variadic: true }],
  bindingSlots: nodeType === 'agent' ? [{ name: 'mcp_tools', resourceType: 'mcp_tool', placement: 'canvas', required: false, multiple: true }] : [], parameterSchema: {}, uiSchema: {}, providers: [], credentials: [],
  retryPolicy: { retryable: false, maxAttempts: 1, initialBackoffMs: 0, maxBackoffMs: 0 }, sandboxRequired: false, supportsMock: true, sideEffectLevel: 'none',
})

const action = (id: string, nodeType = 'set'): StudioNode => ({ id, type: 'manifest', position: { x: 0, y: 0 }, data: { editorKind: 'action', nodeType, typeVersion: 1, label: id, key: id, parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false } })
const binding = (id: string): StudioNode => ({ id, type: 'attachment', position: { x: 0, y: 0 }, data: { editorKind: 'binding', bindingId: id, resourceType: 'mcp_tool', operation: 'use', label: id } })
const edge = (id: string, source: string, target: string, sourceHandle = 'main', targetHandle = 'main'): StudioEdge => ({ id, source, target, sourceHandle, targetHandle, type: 'studio', data: { edgeKind: source.startsWith('binding:') ? 'binding' : 'execution', targetSlot: source.startsWith('binding:') ? targetHandle.replace(/^binding:/, '') : undefined } })

describe('GraphIndex connection validation', () => {
  it('indexes nodes, ports, edges, bindings, and dynamic main handles', () => {
    const nodes = [action('source'), action('target', 'agent'), binding('binding:mcp')]
    const edges = [edge('flow', 'source', 'target'), edge('mcp', 'binding:mcp', 'target', 'resource', 'binding:mcp_tools')]
    const manifests = new Map([['set@1', manifest('set')], ['agent@1', manifest('agent')]])
    const index = createGraphIndex(nodes, edges, manifests)

    expect(index.nodeById.size).toBe(3)
    expect(index.edgesBySourcePort.get(portKey('source', 'main'))).toHaveLength(1)
    expect(index.bindingSummaryByNodeId.get('target')?.[0]).toMatchObject({ role: 'mcp_tools', resourceType: 'mcp_tool' })
    expect(occupiedHandlesByNodeId(index).get('source')).toBe('main')
    expect(occupiedHandlesByNodeId(index).get('target')).toBe('binding:mcp_tools')
    expect(resolveIndexedPort(index, 'target', 'main:3', 'input')?.port.name).toBe('main')
  })

  it('rejects self, duplicate, and incompatible connections and marks occupied inputs for replacement', () => {
    const nodes = [action('source'), action('other'), action('target'), action('agent', 'agent'), binding('binding:mcp')]
    const edges = [edge('existing', 'source', 'target'), edge('mcp', 'binding:mcp', 'agent', 'resource', 'binding:mcp_tools')]
    const manifests = new Map([['set@1', manifest('set')], ['agent@1', manifest('agent')]])
    const index = createGraphIndex(nodes, edges, manifests)

    expect(inspectConnection({ source: 'source', sourceHandle: 'main', target: 'source', targetHandle: 'main' }, index)).toMatchObject({ status: 'invalid', reason: 'self_connection' })
    expect(inspectConnection({ source: 'source', sourceHandle: 'main', target: 'target', targetHandle: 'main' }, index)).toMatchObject({ status: 'invalid', reason: 'duplicate_connection' })
    expect(inspectConnection({ source: 'source', sourceHandle: 'error', target: 'target', targetHandle: 'main' }, index)).toMatchObject({ status: 'invalid', reason: 'incompatible_port' })
    expect(resolveIndexedPort(index, 'other', 'main', 'output')).toMatchObject({ direction: 'output', port: { kind: 'main' } })
    expect(resolveIndexedPort(index, 'target', 'main', 'input')).toMatchObject({ direction: 'input', port: { kind: 'main' } })
    const occupied = inspectConnection({ source: 'other', sourceHandle: 'main', target: 'target', targetHandle: 'main' }, index)
    expect(occupied.reason).toBeUndefined()
    expect(occupied).toMatchObject({ status: 'occupied', replaceEdge: { id: 'existing' } })
    expect(inspectConnection({ source: 'binding:mcp', sourceHandle: 'resource', target: 'agent', targetHandle: 'binding:mcp_tools' }, index)).toMatchObject({ status: 'invalid', reason: 'duplicate_connection' })
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
