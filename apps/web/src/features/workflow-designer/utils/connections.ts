import type { NodeManifest, StudioEdge, StudioNode } from '../model/types'
import { createGraphIndex, portKey, resolveIndexedPort, type GraphIndex } from './graph-index'

type ConnectionCandidate = {
  source: string
  sourceHandle?: string | null
  target: string
  targetHandle?: string | null
}

export type ConnectionValidation = { status: 'valid' | 'invalid' | 'occupied'; replaceEdge?: StudioEdge; reason?: string }

/** Edit-only iteration entry chip id: `${loopId}::iteration-start` (plan5 §4.4). */
export const ITERATION_CHIP_SUFFIX = '::iteration-start'
export const iterationChipId = (loopId: string) => `${loopId}${ITERATION_CHIP_SUFFIX}`
export const isIterationChipId = (nodeId: string | null | undefined) => Boolean(nodeId && nodeId.endsWith(ITERATION_CHIP_SUFFIX))
export const loopIdOfIterationChip = (nodeId: string) => nodeId.slice(0, -ITERATION_CHIP_SUFFIX.length)
export const ITERATION_END_SUFFIX = '::iteration-end'
export const iterationEndId = (loopId: string) => `${loopId}${ITERATION_END_SUFFIX}`
export const isIterationEndId = (nodeId: string | null | undefined) => Boolean(nodeId && nodeId.endsWith(ITERATION_END_SUFFIX))
export const loopIdOfIterationEnd = (nodeId: string) => nodeId.slice(0, -ITERATION_END_SUFFIX.length)

/**
 * Chip handles are pure view anchors. They normalize only for connection
 * inspection and are never persisted into the Definition.
 */
export function normalizeIterationChip<T extends ConnectionCandidate>(connection: T): T {
  if (!isIterationChipId(connection.source)) return connection
  return { ...connection, source: loopIdOfIterationChip(connection.source), sourceHandle: 'main' }
}

export const nodeParentId = (node: StudioNode) => node.data.editorKind === 'action' ? node.data.parentId : undefined

/** Runtime edges never cross a container boundary; body entry is implicit. */
function crossesContainerBoundary(sourceParent: string | undefined, targetParent: string | undefined) {
  return sourceParent !== targetParent
}

export function inspectConnection(rawConnection: ConnectionCandidate, index: GraphIndex): ConnectionValidation {
  if (isIterationChipId(rawConnection.source)) {
    const loopId = loopIdOfIterationChip(rawConnection.source)
    const target = index.nodeById.get(rawConnection.target)
    if (!target || nodeParentId(target) !== loopId) return invalid('container_edge_crosses_boundary')
    const targetPort = resolveIndexedPort(index, target.id, rawConnection.targetHandle, 'input')
    if (targetPort?.direction !== 'input' || !('kind' in targetPort.port) || targetPort.port.kind !== 'main') return invalid('incompatible_port')
    const hasBodyPredecessor = [...index.edgesByTargetPort.values()].flat().some((edge) => {
      const predecessor = index.nodeById.get(edge.source)
      return edge.target === target.id && predecessor && nodeParentId(predecessor) === loopId
    })
    return hasBodyPredecessor ? invalid('loop_start_requires_body_entry') : { status: 'valid' }
  }
  if (isIterationEndId(rawConnection.target)) {
    const loopId = loopIdOfIterationEnd(rawConnection.target)
    const source = index.nodeById.get(rawConnection.source)
    if (!source || nodeParentId(source) !== loopId) return invalid('container_edge_crosses_boundary')
    const sourcePort = resolveIndexedPort(index, source.id, rawConnection.sourceHandle, 'output')
    const targetKind = rawConnection.targetHandle === 'error' ? 'error' : 'main'
    if (sourcePort?.direction !== 'output' || !('kind' in sourcePort.port) || sourcePort.port.kind !== targetKind) return invalid('incompatible_port')
    const hasBodySuccessor = [...index.edgesBySourcePort.values()].flat().some((edge) => {
      const successor = index.nodeById.get(edge.target)
      return edge.source === source.id && successor && nodeParentId(successor) === loopId
    })
    return hasBodySuccessor ? invalid('loop_end_requires_body_sink') : { status: 'valid' }
  }
  const connection = normalizeIterationChip(rawConnection)
  const sourceIsStart = connection.source === '__start__'
  if (sourceIsStart) {
    if (connection.sourceHandle !== 'main') return invalid('invalid_start_port')
    const target = index.nodeById.get(connection.target)
    if (!target || (target.data.editorKind !== 'action' && target.data.editorKind !== 'exit')) return invalid('invalid_target')
    if (nodeParentId(target)) return invalid('container_edge_crosses_boundary')
    const targetPort = resolveIndexedPort(index, target.id, connection.targetHandle, 'input')
    return targetPort?.direction === 'input' && 'kind' in targetPort.port && targetPort.port.kind === 'main' ? { status: 'valid' } : invalid('incompatible_port')
  }
  const source = index.nodeById.get(connection.source)
  const target = index.nodeById.get(connection.target)
  if (!source || !target) return invalid('missing_node')
  if (source.id === target.id) return invalid('self_connection')
  if (crossesContainerBoundary(nodeParentId(source), nodeParentId(target))) return invalid('container_edge_crosses_boundary')
  if (index.edgesBySourcePort.get(portKey(connection.source, connection.sourceHandle))?.some((edge) => edge.target === connection.target && edge.targetHandle === connection.targetHandle)) return invalid('duplicate_connection')
  if (target.data.editorKind !== 'action' && target.data.editorKind !== 'exit') return invalid('invalid_target')
  if (source.data.editorKind === 'exit') return invalid('invalid_source')
  const sourcePort = resolveIndexedPort(index, source.id, connection.sourceHandle, 'output')
  const targetPort = resolveIndexedPort(index, target.id, connection.targetHandle, 'input')
  if (sourcePort?.direction !== 'output' || targetPort?.direction !== 'input' || !('kind' in sourcePort.port) || !('kind' in targetPort.port) || sourcePort.port.kind !== targetPort.port.kind) return invalid('incompatible_port')
  const occupied = index.edgesByTargetPort.get(portKey(target.id, connection.targetHandle))?.[0]
  return occupied && !targetPort.port.variadic ? { status: 'occupied', replaceEdge: occupied } : { status: 'valid' }
}

export function validateConnection(connection: ConnectionCandidate, nodes: StudioNode[], edges: StudioEdge[], manifests: Map<string, NodeManifest>) {
  return inspectConnection(connection, createGraphIndex(nodes, edges, manifests)).status !== 'invalid'
}

const invalid = (reason: string): ConnectionValidation => ({ status: 'invalid', reason })
