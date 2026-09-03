import { describe, expect, it } from 'vitest'

import type { StudioEdge, StudioNode } from '../model/types'
import { autoLayout, LARGE_GRAPH_LAYOUT_THRESHOLD, LOOP_CONTAINER_MIN_HEIGHT, LOOP_CONTAINER_MIN_WIDTH, LOOP_CONTAINER_PADDING } from './layout'

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

function actionNode(id: string): StudioNode { return { id, type: 'manifest', position: { x: 0, y: 0 }, data: { editorKind: 'action', nodeType: 'set', typeVersion: 1, label: id, key: id, parameters: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false } } }

describe('container-aware auto layout', () => {
  it('sizes each loop container around its laid-out body', async () => {
    const loop: StudioNode = loopNode('loop')
    const first: StudioNode = actionNode('body-1')
    const second: StudioNode = actionNode('body-2')
    const outside: StudioNode = actionNode('outside')
    const nodes = [
      { ...loop, position: { x: 600, y: 80 } },
      { ...first, data: { ...first.data, parentId: 'loop' } },
      { ...second, data: { ...second.data, parentId: 'loop' } },
      outside,
    ]
    const edges = [
      edgeOf('fanout', 'loop', 'body-1'),
      edgeOf('chain', 'body-1', 'body-2'),
      edgeOf('after', 'loop', 'outside'),
    ]

    const layouted = await autoLayout(nodes, edges)
    const byId = new Map(layouted.map((node) => [node.id, node]))
    const bodyFirst = byId.get('body-1')!
    const bodySecond = byId.get('body-2')!
    const container = byId.get('loop')!

    // Children keep container-relative coordinates below the header and chip.
    expect(bodyFirst.position.x).toBeGreaterThanOrEqual(LOOP_CONTAINER_PADDING.left)
    expect(bodyFirst.position.y).toBeGreaterThanOrEqual(LOOP_CONTAINER_PADDING.top)
    expect(bodySecond.position.x).toBeGreaterThan(bodyFirst.position.x)

    // Loose bound: the frame must cover the content plus padding.
    expect(container.width ?? 0).toBeGreaterThanOrEqual(bodySecond.position.x + 240 + LOOP_CONTAINER_PADDING.right)
    expect(container.height ?? 0).toBeGreaterThanOrEqual(bodySecond.position.y + LOOP_CONTAINER_PADDING.bottom)
    expect(container.width ?? 0).toBeGreaterThanOrEqual(LOOP_CONTAINER_MIN_WIDTH)
    expect(container.height ?? 0).toBeGreaterThanOrEqual(LOOP_CONTAINER_MIN_HEIGHT)

    // Roots are laid out on the outer canvas and stay apart from the body frame.
    const freeNode = byId.get('outside')!
    expect(Math.abs(freeNode.position.x - bodyFirst.position.x)).toBeGreaterThan(50)
  })
})

function edgeOf(id: string, source: string, target: string): StudioEdge { return { id, source, target, sourceHandle: 'main', targetHandle: 'main', type: 'studio', data: { edgeKind: 'execution', order: 0 } } }

function loopNode(id: string): StudioNode {
  return { id, type: 'manifest', position: { x: 0, y: 0 }, data: { editorKind: 'action', nodeType: 'loop_over_items', typeVersion: 1, label: id, key: id, parameters: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false } }
}
