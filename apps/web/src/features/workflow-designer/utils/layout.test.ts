import { describe, expect, it } from 'vitest'

import type { StudioEdge, StudioNode } from '../model/types'
import { autoLayout, LARGE_GRAPH_LAYOUT_THRESHOLD } from './layout'

describe('large Workflow layout baseline', () => {
  it('lays out 600 nodes deterministically within the degradation budget', async () => {
    expect(LARGE_GRAPH_LAYOUT_THRESHOLD).toBeLessThan(600)
    const nodes = Array.from({ length: 600 }, (_, index) => actionNode(`node-${index}`))
    const edges = Array.from({ length: 599 }, (_, index): StudioEdge => ({ id: `edge-${index}`, source: `node-${index}`, target: `node-${index + 1}`, type: 'studio', data: { edgeKind: 'execution', order: 0 } }))
    const started = performance.now()
    const first = await autoLayout(nodes, edges)
    const elapsed = performance.now() - started
    const second = await autoLayout(nodes, edges)

    expect(elapsed).toBeLessThan(1_500)
    expect(first).toHaveLength(600)
    expect(first.map((node) => node.position)).toEqual(second.map((node) => node.position))
    expect(new Set(first.map((node) => `${node.position.x}:${node.position.y}`)).size).toBe(600)
  })
})

function actionNode(id: string): StudioNode { return { id, type: 'manifest', position: { x: 0, y: 0 }, data: { editorKind: 'action', nodeType: 'set', typeVersion: 1, label: id, key: id, parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false } } }
