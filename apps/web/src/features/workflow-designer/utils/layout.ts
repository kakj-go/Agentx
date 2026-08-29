import type { NodeManifest, StudioEdge, StudioNode } from '../model/types'
import { canvasNodeMetrics, canvasNodeRole } from '../nodes/node-appearance'

export const LARGE_GRAPH_LAYOUT_THRESHOLD = 250

export async function autoLayout(nodes: StudioNode[], edges: StudioEdge[], manifests?: Map<string, NodeManifest>) {
  if (nodes.length >= LARGE_GRAPH_LAYOUT_THRESHOLD) return largeGraphLayout(nodes, edges, manifests)
  const { default: ELK } = await import('elkjs/lib/elk.bundled.js')
  const elk = new ELK()
  const graph = await elk.layout({
    id: 'root',
    layoutOptions: { 'elk.algorithm': 'layered', 'elk.direction': 'RIGHT', 'elk.spacing.nodeNode': '48', 'elk.layered.spacing.nodeNodeBetweenLayers': '90' },
    children: nodes.map((node) => {
      if (node.data.editorKind === 'binding') {
        const metrics = canvasNodeMetrics('default', { kind: 'binding' })
        return { id: node.id, width: metrics.width, height: metrics.height }
      }
      if (node.data.editorKind === 'exit') {
        return { id: node.id, width: 160, height: 80 }
      }
      const manifest = manifests?.get(`${node.data.nodeType}@${node.data.typeVersion}`)
      const metrics = canvasNodeMetrics(canvasNodeRole(manifest), {
        inputs: manifest?.inputPorts.length,
        outputs: manifest?.outputPorts.length,
        bindings: manifest?.bindingSlots.filter((slot) => slot.placement === 'canvas').length,
        richHeight: node.height ?? node.measured?.height,
      })
      return { id: node.id, width: metrics.width, height: metrics.height }
    }),
    edges: edges.map((edge) => ({ id: edge.id, sources: [edge.source], targets: [edge.target] })),
  })
  const positions = new Map(graph.children?.map((node) => [node.id, { x: node.x ?? 0, y: node.y ?? 0 }]))
  placeBindingsBelowTargets(nodes, edges, positions, manifests)
  return nodes.map((node) => ({ ...node, position: positions.get(node.id) ?? node.position }))
}

export function largeGraphLayout(nodes: StudioNode[], edges: StudioEdge[], manifests?: Map<string, NodeManifest>) {
  const actions = nodes.filter((node) => node.data.editorKind === 'action' || node.data.editorKind === 'exit')
  const actionById = new Map(actions.map((node) => [node.id, node]))
  const actionIds = new Set(actions.map((node) => node.id))
  const outgoing = new Map<string, string[]>()
  const indegree = new Map(actions.map((node) => [node.id, 0]))
  const levels = new Map(actions.map((node) => [node.id, 0]))
  for (const edge of edges) {
    if (edge.data?.edgeKind === 'binding' || !actionIds.has(edge.source) || !actionIds.has(edge.target)) continue
    const targets = outgoing.get(edge.source)
    if (targets) targets.push(edge.target)
    else outgoing.set(edge.source, [edge.target])
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
    const bucket = buckets.get(level)
    if (bucket) bucket.push(node.id)
    else buckets.set(level, [node.id])
  }
  const positions = new Map<string, { x: number; y: number }>()
  for (const [level, ids] of buckets) for (const [index, id] of ids.entries()) {
    const node = actionById.get(id)
    const manifest = node?.data.editorKind === 'action' ? manifests?.get(`${node.data.nodeType}@${node.data.typeVersion}`) : undefined
    const metrics = canvasNodeMetrics(canvasNodeRole(manifest), {
      inputs: manifest?.inputPorts.length,
      outputs: manifest?.outputPorts.length,
      bindings: manifest?.bindingSlots.filter((slot) => slot.placement === 'canvas').length,
      richHeight: node?.height ?? node?.measured?.height,
    })
    positions.set(id, { x: 80 + level * 280, y: 60 + index * Math.max(120, metrics.height + 32) })
  }
  placeBindingsBelowTargets(nodes, edges, positions, manifests)
  return nodes.map((node) => ({ ...node, position: positions.get(node.id) ?? node.position }))
}

function placeBindingsBelowTargets(nodes: StudioNode[], edges: StudioEdge[], positions: Map<string, { x: number; y: number }>, manifests?: Map<string, NodeManifest>) {
  const bindingIndex = new Map<string, number>()
  const nodeById = new Map(nodes.map((node) => [node.id, node]))
  const bindingEdgeBySource = new Map(edges.filter((edge) => edge.data?.edgeKind === 'binding').map((edge) => [edge.source, edge]))
  for (const node of nodes.filter((item) => item.data.editorKind === 'binding')) {
    const edge = bindingEdgeBySource.get(node.id)
    const targetNode = edge ? nodeById.get(edge.target) : undefined
    const target = edge ? positions.get(edge.target) : undefined
    const key = target && edge ? edge.target : 'unbound'
    const index = bindingIndex.get(key) ?? 0
    bindingIndex.set(key, index + 1)
    const attachment = canvasNodeMetrics('default', { kind: 'binding' })
    if (!target || !targetNode || targetNode.data.editorKind !== 'action') {
      positions.set(node.id, { x: 80 + index * (attachment.width + 40), y: 520 })
      continue
    }
    const manifest = manifests?.get(`${targetNode.data.nodeType}@${targetNode.data.typeVersion}`)
    const targetMetrics = canvasNodeMetrics(canvasNodeRole(manifest), { richHeight: targetNode.height ?? targetNode.measured?.height })
    positions.set(node.id, { x: target.x + index * (attachment.width + 16), y: target.y + targetMetrics.height + 72 })
  }
}
