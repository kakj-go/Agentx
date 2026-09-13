import type { StudioDocument } from '../model/types'

export function includedActionNodeIds(document: StudioDocument, mode: 'full' | 'single_node' | 'to_node' | 'from_node', target?: string) {
  const actions = document.nodes.filter((node) => node.data.editorKind === 'action' && !node.data.disabled).map((node) => node.id)
  if (mode === 'full') return new Set(actions)
  if (!target || !actions.includes(target)) return new Set<string>()
  if (mode === 'single_node') return new Set([target])
  const edges = document.edges.filter((edge) => edge.data?.edgeKind === 'execution')
  const neighbors = new Map<string, string[]>()
  for (const edge of edges) {
    const from = mode === 'to_node' ? edge.target : edge.source
    const to = mode === 'to_node' ? edge.source : edge.target
    neighbors.set(from, [...(neighbors.get(from) ?? []), to])
  }
  const included = new Set<string>()
  const queue = [target]
  while (queue.length) {
    const node = queue.shift()!
    if (included.has(node)) continue
    included.add(node)
    queue.push(...(neighbors.get(node) ?? []))
  }
  return included
}
