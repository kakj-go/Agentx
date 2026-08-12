import type { NodeManifest, StudioEdge, StudioNode } from '../model/types'
import { createGraphIndex, portKey, resolveIndexedPort, type GraphIndex } from './graph-index'

type ConnectionCandidate = {
  source: string
  sourceHandle?: string | null
  target: string
  targetHandle?: string | null
}

export type ConnectionValidation = { status: 'valid' | 'invalid' | 'occupied'; replaceEdge?: StudioEdge; reason?: string }

export function inspectConnection(connection: ConnectionCandidate, index: GraphIndex): ConnectionValidation {
  const sourceIsStart = connection.source === '__start__'
  const targetIsEnd = connection.target === '__end__'
  if (sourceIsStart || targetIsEnd) {
    if (sourceIsStart && targetIsEnd) return connection.sourceHandle === 'main' && connection.targetHandle === 'main' ? { status: 'valid' } : invalid('invalid_boundary_port')
    if (sourceIsStart) {
      if (connection.sourceHandle !== 'main') return invalid('invalid_start_port')
      const target = index.nodeById.get(connection.target)
      if (!target || target.data.editorKind !== 'action') return invalid('invalid_target')
      const targetPort = resolveIndexedPort(index, target.id, connection.targetHandle, 'input')
      return targetPort?.direction === 'input' && 'kind' in targetPort.port && targetPort.port.kind === 'main' ? { status: 'valid' } : invalid('incompatible_port')
    }
    const source = index.nodeById.get(connection.source)
    if (!source || source.data.editorKind !== 'action') return invalid('invalid_source')
    const sourcePort = resolveIndexedPort(index, source.id, connection.sourceHandle, 'output')
    if (sourcePort?.direction !== 'output' || !('kind' in sourcePort.port)) return invalid('incompatible_port')
    if (connection.targetHandle === 'main' && sourcePort.port.kind === 'main') return { status: 'valid' }
    if (connection.targetHandle === 'error' && sourcePort.port.kind === 'error') return { status: 'valid' }
    return invalid('incompatible_port')
  }
  const source = index.nodeById.get(connection.source)
  const target = index.nodeById.get(connection.target)
  if (!source || !target) return invalid('missing_node')
  if (source.id === target.id) return invalid('self_connection')
  if (index.edgesBySourcePort.get(portKey(connection.source, connection.sourceHandle))?.some((edge) => edge.target === connection.target && edge.targetHandle === connection.targetHandle)) return invalid('duplicate_connection')
  if (source.data.editorKind === 'binding') {
    const targetPort = resolveIndexedPort(index, target.id, connection.targetHandle, 'binding')
    if (target.data.editorKind !== 'action' || connection.sourceHandle !== 'resource' || targetPort?.direction !== 'binding' || !('resourceType' in targetPort.port) || targetPort.port.resourceType !== source.data.resourceType) return invalid('incompatible_binding')
    const occupied = index.edgesByTargetPort.get(portKey(target.id, connection.targetHandle))?.[0]
    return occupied && !targetPort.port.multiple ? { status: 'occupied', replaceEdge: occupied } : { status: 'valid' }
  }
  if (target.data.editorKind !== 'action') return invalid('invalid_target')
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
