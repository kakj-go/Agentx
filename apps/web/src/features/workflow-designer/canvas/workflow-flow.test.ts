import { describe, expect, it } from 'vitest'

import type { StudioEdge } from '../model/types'
import { proxyEdges } from '../utils/group-edges'
import { containerEscapeOverflow, findDropContainer, indexContainers, loopBoundaryLinks, materializeRuntimeEdges } from './workflow-flow'

describe('collapsed Group edge proxies', () => {
  it('hides internal edges and aggregates external handles onto the proxy node', () => {
    const edges: StudioEdge[] = [
      { id: 'internal', source: 'a', target: 'b', sourceHandle: 'main', targetHandle: 'main', data: { edgeKind: 'execution', order: 0 } },
      { id: 'outgoing', source: 'b', target: 'c', sourceHandle: 'main', targetHandle: 'main', data: { edgeKind: 'execution', order: 1 } },
      { id: 'incoming', source: 'd', target: 'a', sourceHandle: 'error', targetHandle: 'main', data: { edgeKind: 'execution', order: 0 } },
    ]
    const collapsed = new Map([['a', 'group:g'], ['b', 'group:g']])

    expect(proxyEdges(edges, collapsed)).toEqual([
      expect.objectContaining({ id: 'outgoing', source: 'group:g', sourceHandle: 'group-out', target: 'c' }),
      expect.objectContaining({ id: 'incoming', source: 'd', target: 'group:g', targetHandle: 'group-in' }),
    ])
  })

  it('reuses unrelated edge objects when one node runtime status changes', () => {
    const edges: StudioEdge[] = [
      { id: 'affected', source: 'a', target: 'b', data: { edgeKind: 'execution' } },
      { id: 'unrelated', source: 'c', target: 'd', data: { edgeKind: 'execution' } },
    ]
    const cache = new Map()
    const before = materializeRuntimeEdges(edges, new Map(), cache)
    const after = materializeRuntimeEdges(edges, new Map([['a', 'running']]), cache)
    expect(after[0]).not.toBe(before[0])
    expect(after[0].data?.runtimeStatus).toBe('running')
    expect(after[1]).toBe(before[1])
  })
})

describe('loop container view projection', () => {
  const loop = { id: 'loop-1', type: 'manifest' as const, position: { x: 600, y: 120 }, width: 470, height: 300, data: { editorKind: 'action' as const, nodeType: 'loop_over_items', typeVersion: 1, label: 'Loop', key: 'loop', parameters: { parallelism: 4 }, contextWrites: [], resourceReferences: [], settings: {}, disabled: false } }
  const body = { id: 'body-1', type: 'manifest' as const, position: { x: 40, y: 130 }, data: { editorKind: 'action' as const, nodeType: 'set', typeVersion: 1, label: 'Body', key: 'body', parentId: 'loop-1', parameters: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false } }
  const free = { id: 'free-1', type: 'manifest' as const, position: { x: 60, y: 40 }, data: { editorKind: 'action' as const, nodeType: 'set', typeVersion: 1, label: 'Free', key: 'free', parameters: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false } }

  it('keeps empty loops in container mode and adds owned children', () => {
    const containers = indexContainers([loop, body, free])
    expect(containers.has('loop-1')).toBe(true)
    expect(containers.get('loop-1')?.childIds.has('body-1')).toBe(true)
    expect(indexContainers([loop, free]).size).toBe(1)
  })

  it('claims palette drops inside the container bounding box in relative coordinates', () => {
    const nodes = [loop, body, free]
    expect(findDropContainer(nodes, { x: 700, y: 200 })).toBe('loop-1')
    expect(findDropContainer(nodes, { x: 620, y: 440 })).toBeUndefined()
    // Loop manifests never nest, even when dropped inside a container.
    expect(findDropContainer([loop, { ...loop, id: 'loop-2', position: { x: 700, y: 200 } }, body], { x: 700, y: 200 })).toBe('loop-1')
  })

  it('reports how far a dragged child escapes its container', () => {
    const container = { ...loop, width: 470, height: 300 }
    expect(containerEscapeOverflow({ position: { x: 40, y: 130 }, width: 240, height: 120 }, container)).toBe(0)
    expect(containerEscapeOverflow({ position: { x: -70, y: 0 }, width: 240, height: 120 }, container)).toBe(70)
    expect(containerEscapeOverflow({ position: { x: 400, y: 0 }, width: 240, height: 120 }, container)).toBe(170)
  })

  it('derives view-only boundary links around the real body graph', () => {
    const edges: StudioEdge[] = [{ id: 'inside', source: 'body-1', sourceHandle: 'main', target: 'body-2', targetHandle: 'main', data: { edgeKind: 'execution', order: 0 } }]
    const containers = indexContainers([loop, body, { ...body, id: 'body-2', position: { x: 320, y: 130 }, data: { ...body.data, key: 'body_2' } }])
    const links = loopBoundaryLinks(containers.get('loop-1')!, edges, new Map(), 470)

    expect(links.map((link) => [link.id, link.kind])).toEqual([['entry:body-1', 'entry'], ['main:body-2', 'main']])
  })
})
