import type { StudioEdge } from '../model/types'

export function proxyEdges(edges: StudioEdge[], collapsedByMember: Map<string, string>): StudioEdge[] {
  return edges.flatMap((edge) => {
    const source = collapsedByMember.get(edge.source)
    const target = collapsedByMember.get(edge.target)
    if (source && source === target) return []
    return [{ ...edge, source: source ?? edge.source, target: target ?? edge.target, sourceHandle: source ? 'group-out' : edge.sourceHandle, targetHandle: target ? 'group-in' : edge.targetHandle }]
  })
}
