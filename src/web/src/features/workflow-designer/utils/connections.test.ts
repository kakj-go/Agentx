import { describe, expect, it } from 'vitest'

import type { NodeManifest, StudioEdge, StudioNode } from '../model/types'
import { createGraphIndex } from './graph-index'
import { inspectConnection, isIterationChipId, iterationChipId, iterationEndId, loopIdOfIterationChip, normalizeIterationChip } from './connections'

function port(name: string, kind: 'main' | 'error', variadic = false) {
  return { name, kind, required: false, variadic }
}

function manifest(nodeType: string): NodeManifest {
  return {
    protocolVersion: '1.0',
    nodeType,
    version: 1,
    displayName: nodeType,
    description: '',
    category: 'logic',
    keywords: [],
    iconKey: 'box',
    executionStyle: 'action',
    capability: 'utility',
    readiness: 'all',
    inputPorts: [port('main', 'main')],
    outputPorts: [port('main', 'main'), port('error', 'error')],
    bindingSlots: [],
    parameterSchema: {},
    uiSchema: {},
    providers: [],
    credentials: [],
    retryPolicy: { retryable: false, maxAttempts: 1, initialBackoffMs: 0, maxBackoffMs: 0 },
    sandboxRequired: false,
    supportsMock: false,
    sideEffectLevel: 'none',
  }
}

function actionNode(id: string, nodeType = 'set', parentId?: string): StudioNode {
  return {
    id,
    type: 'manifest',
    position: { x: 0, y: 0 },
    data: { editorKind: 'action', nodeType, typeVersion: 1, label: id, key: id, parentId, parameters: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false },
  }
}

function exitNode(id: string): StudioNode {
  return { id, type: 'exit', position: { x: 0, y: 0 }, data: { editorKind: 'exit', key: id, label: id, protected: false, parameters: { outputs: {}, errorOutputs: {} } } }
}

const LOOP = 'loop-1'
const CHIP = iterationChipId(LOOP)

function studio() {
  const manifests = new Map<string, NodeManifest>([
    ['set@1', manifest('set')],
    ['loop_over_items@1', manifest('loop_over_items')],
  ])
  const nodes = [
    actionNode(LOOP, 'loop_over_items'),
    actionNode('body-a', 'set', LOOP),
    actionNode('body-b', 'set', LOOP),
    actionNode('outside', 'set'),
    exitNode('end'),
  ]
  const edges: StudioEdge[] = []
  return { manifests, nodes, edges }
}

describe('iteration chip identity', () => {
  it('derives chip ids from loop ids and recognizes them by suffix', () => {
    expect(iterationChipId(LOOP)).toBe(`${LOOP}::iteration-start`)
    expect(isIterationChipId(CHIP)).toBe(true)
    expect(isIterationChipId(LOOP)).toBe(false)
    expect(isIterationChipId(null)).toBe(false)
    expect(loopIdOfIterationChip(CHIP)).toBe(LOOP)
  })

  it('rewrites chip connection sources onto the loop node main port', () => {
    const normalized = normalizeIterationChip({ source: CHIP, sourceHandle: 'main', target: 'body-a', targetHandle: 'main' })
    expect(normalized).toEqual({ source: LOOP, sourceHandle: 'main', target: 'body-a', targetHandle: 'main' })
    expect(normalizeIterationChip({ source: 'outside', sourceHandle: 'main', target: 'end', targetHandle: 'main' }).source).toBe('outside')
  })
})

describe('container boundary connection validation', () => {
  it('rejects persisted container-to-body edges because entry is implicit', () => {
    const { manifests, nodes, edges } = studio()
    const result = inspectConnection({ source: LOOP, sourceHandle: 'main', target: 'body-b', targetHandle: 'main' }, createGraphIndex(nodes, edges, manifests))
    expect(result).toMatchObject({ status: 'invalid', reason: 'container_edge_crosses_boundary' })
  })

  it('accepts edges between nodes of the same container', () => {
    const { manifests, nodes, edges } = studio()
    const result = inspectConnection({ source: 'body-a', sourceHandle: 'main', target: 'body-b', targetHandle: 'main' }, createGraphIndex(nodes, edges, manifests))
    expect(result.status).toBe('valid')
  })

  it('rejects connections from inside a container to the outside', () => {
    const { manifests, nodes, edges } = studio()
    const result = inspectConnection({ source: 'body-a', sourceHandle: 'main', target: 'outside', targetHandle: 'main' }, createGraphIndex(nodes, edges, manifests))
    expect(result).toMatchObject({ status: 'invalid', reason: 'container_edge_crosses_boundary' })
  })

  it('rejects connections from a body node to an exit node outside', () => {
    const { manifests, nodes, edges } = studio()
    const result = inspectConnection({ source: 'body-a', sourceHandle: 'main', target: 'end', targetHandle: 'main' }, createGraphIndex(nodes, edges, manifests))
    expect(result).toMatchObject({ status: 'invalid', reason: 'container_edge_crosses_boundary' })
  })

  it('rejects connections from outside into a container body that do not start at the container', () => {
    const { manifests, nodes, edges } = studio()
    const result = inspectConnection({ source: 'outside', sourceHandle: 'main', target: 'body-a', targetHandle: 'main' }, createGraphIndex(nodes, edges, manifests))
    expect(result).toMatchObject({ status: 'invalid', reason: 'container_edge_crosses_boundary' })
  })

  it('rejects start boundary edges that jump directly into a container body', () => {
    const { manifests, nodes, edges } = studio()
    const result = inspectConnection({ source: '__start__', sourceHandle: 'main', target: 'body-a', targetHandle: 'main' }, createGraphIndex(nodes, edges, manifests))
    expect(result).toMatchObject({ status: 'invalid', reason: 'container_edge_crosses_boundary' })
  })

  it('accepts chip-origin connections as non-persisted view anchors', () => {
    const { manifests, nodes, edges } = studio()
    const index = createGraphIndex(nodes, edges, manifests)
    expect(inspectConnection({ source: CHIP, sourceHandle: 'main', target: 'body-b', targetHandle: 'main' }, index).status).toBe('valid')
    expect(inspectConnection({ source: CHIP, sourceHandle: 'main', target: 'body-a', targetHandle: 'main' }, index).status).toBe('valid')
  })

  it('accepts body outputs into the edit-only end chip and rejects outside sources', () => {
    const { manifests, nodes, edges } = studio()
    const index = createGraphIndex(nodes, edges, manifests)
    expect(inspectConnection({ source: 'body-a', sourceHandle: 'main', target: iterationEndId(LOOP), targetHandle: 'main' }, index).status).toBe('valid')
    expect(inspectConnection({ source: 'body-a', sourceHandle: 'error', target: iterationEndId(LOOP), targetHandle: 'error' }, index).status).toBe('valid')
    expect(inspectConnection({ source: 'outside', sourceHandle: 'main', target: iterationEndId(LOOP), targetHandle: 'main' }, index)).toMatchObject({ status: 'invalid', reason: 'container_edge_crosses_boundary' })
  })

  it('accepts boundary gestures only for derived body entries and sinks', () => {
    const { manifests, nodes } = studio()
    const edges: StudioEdge[] = [{ id: 'inside', source: 'body-a', sourceHandle: 'main', target: 'body-b', targetHandle: 'main', data: { edgeKind: 'execution', order: 0 } }]
    const index = createGraphIndex(nodes, edges, manifests)

    expect(inspectConnection({ source: CHIP, sourceHandle: 'main', target: 'body-b', targetHandle: 'main' }, index)).toMatchObject({ status: 'invalid', reason: 'loop_start_requires_body_entry' })
    expect(inspectConnection({ source: 'body-a', sourceHandle: 'main', target: iterationEndId(LOOP), targetHandle: 'main' }, index)).toMatchObject({ status: 'invalid', reason: 'loop_end_requires_body_sink' })
    expect(inspectConnection({ source: CHIP, sourceHandle: 'main', target: 'body-a', targetHandle: 'main' }, index).status).toBe('valid')
    expect(inspectConnection({ source: 'body-b', sourceHandle: 'main', target: iterationEndId(LOOP), targetHandle: 'main' }, index).status).toBe('valid')
  })
})
