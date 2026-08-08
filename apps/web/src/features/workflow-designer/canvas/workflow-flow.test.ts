import { describe, expect, it } from 'vitest'

import type { StudioEdge } from '../model/types'
import { proxyEdges } from '../utils/group-edges'
import { materializeRuntimeEdges } from './workflow-flow'

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
