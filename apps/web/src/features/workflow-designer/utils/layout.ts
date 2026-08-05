import type { StudioEdge, StudioNode } from '../model/types'

export const LARGE_GRAPH_LAYOUT_THRESHOLD = 250

export async function autoLayout(nodes: StudioNode[], edges: StudioEdge[]) {
  if (nodes.length >= LARGE_GRAPH_LAYOUT_THRESHOLD) return largeGraphLayout(nodes, edges)
  const { default: ELK } = await import('elkjs/lib/elk.bundled.js')
  const elk = new ELK()
  const graph = await elk.layout({
    id: 'root',
    layoutOptions: { 'elk.algorithm': 'layered', 'elk.direction': 'RIGHT', 'elk.spacing.nodeNode': '48', 'elk.layered.spacing.nodeNodeBetweenLayers': '90' },
    children: nodes.map((node) => ({ id: node.id, width: node.data.editorKind === 'binding' ? 180 : 220, height: node.data.editorKind === 'binding' ? 58 : 88 })),
    edges: edges.map((edge) => ({ id: edge.id, sources: [edge.source], targets: [edge.target] })),
  })
  const positions = new Map(graph.children?.map((node) => [node.id, { x: node.x ?? 0, y: node.y ?? 0 }]))
  return nodes.map((node) => ({ ...node, position: positions.get(node.id) ?? node.position }))
}

export function largeGraphLayout(nodes: StudioNode[], edges: StudioEdge[]) {
  const actions = nodes.filter((node) => node.data.editorKind === 'action')
  const actionIds = new Set(actions.map((node) => node.id))
  const outgoing = new Map<string, string[]>()
  const indegree = new Map(actions.map((node) => [node.id, 0]))
  const levels = new Map(actions.map((node) => [node.id, 0]))
  for (const edge of edges) {
    if (edge.data?.edgeKind === 'binding' || !actionIds.has(edge.source) || !actionIds.has(edge.target)) continue
    outgoing.set(edge.source, [...(outgoing.get(edge.source) ?? []), edge.target])
    indegree.set(edge.target, (indegree.get(edge.target) ?? 0) + 1)
  }
  const queue = actions.filter((node) => indegree.get(node.id) === 0).map((node) => node.id)
  for (let cursor = 0; cursor < queue.length; cursor++) {
    const source = queue[cursor]
    for (const target of outgoing.get(source) ?? []) {
      levels.set(target, Math.max(levels.get(target) ?? 0, (levels.get(source) ?? 0) + 1))
      const remaining = (indegree.get(target) ?? 1) - 1
      indegree.set(target, remaining)
      if (remaining === 0) queue.push(target)
    }
  }
  const buckets = new Map<number, string[]>()
  for (const node of actions) {
    const level = levels.get(node.id) ?? 0
    buckets.set(level, [...(buckets.get(level) ?? []), node.id])
  }
  const positions = new Map<string, { x: number; y: number }>()
  for (const [level, ids] of buckets) for (const [index, id] of ids.entries()) positions.set(id, { x: 80 + level * 280, y: 60 + index * 120 })
  const bindingIndex = new Map<string, number>()
  for (const node of nodes.filter((item) => item.data.editorKind === 'binding')) {
    const edge = edges.find((item) => item.data?.edgeKind === 'binding' && item.source === node.id)
    const target = edge ? positions.get(edge.target) : undefined
    const index = target && edge ? bindingIndex.get(edge.target) ?? 0 : bindingIndex.get('unbound') ?? 0
    const key = target && edge ? edge.target : 'unbound'
    bindingIndex.set(key, index + 1)
    positions.set(node.id, target ? { x: target.x, y: target.y + 104 + index * 70 } : { x: 80 + index * 200, y: 520 })
  }
  return nodes.map((node) => ({ ...node, position: positions.get(node.id) ?? node.position }))
}
