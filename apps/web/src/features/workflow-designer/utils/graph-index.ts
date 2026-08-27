import type { BindingSlot, NodeManifest, NodePort, StudioEdge, StudioNode } from '../model/types'

export type IndexedPort = {
  direction: 'input' | 'output' | 'binding'
  nodeId: string
  port: NodePort | BindingSlot
}

export type NodeBindingSummary = { role: string; resourceType: string; label: string }

export type GraphIndex = {
  nodeById: Map<string, StudioNode>
  portByHandle: Map<string, IndexedPort>
  edgesBySourcePort: Map<string, StudioEdge[]>
  edgesByTargetPort: Map<string, StudioEdge[]>
  bindingSummaryByNodeId: Map<string, NodeBindingSummary[]>
}

export const portKey = (nodeId: string, handle?: string | null) => `${nodeId}\u0000${handle ?? ''}`
export const portHandleKey = (nodeId: string, direction: IndexedPort['direction'], handle?: string | null) => `${nodeId}\u0000${direction}\u0000${handle ?? ''}`

function emptyIndex(): GraphIndex {
  return { nodeById: new Map(), portByHandle: new Map(), edgesBySourcePort: new Map(), edgesByTargetPort: new Map(), bindingSummaryByNodeId: new Map() }
}

/** Keeps connection lookups stable while applying only changed nodes and edges. */
export class IncrementalGraphIndex {
  readonly value: GraphIndex = emptyIndex()
  private readonly nodeRefs = new Map<string, StudioNode>()
  private readonly nodePortSignatures = new Map<string, string>()
  private readonly nodePortKeys = new Map<string, string[]>()
  private readonly edgeRefs = new Map<string, StudioEdge>()
  private readonly edgeSignatures = new Map<string, string>()
  private syncedManifests?: Map<string, NodeManifest>

  sync(nodes: StudioNode[], edges: StudioEdge[], manifests: Map<string, NodeManifest>) {
    const manifestChanged = this.syncedManifests !== manifests
    this.syncedManifests = manifests
    const nextNodeIds = new Set(nodes.map((node) => node.id))
    for (const id of this.nodeRefs.keys()) {
      if (nextNodeIds.has(id)) continue
      this.removeNode(id)
    }
    for (const node of nodes) {
      const previous = this.nodeRefs.get(node.id)
      const signature = nodePortSignature(node, manifests)
      if (previous && previous === node && !manifestChanged) continue
      this.value.nodeById.set(node.id, node)
      this.nodeRefs.set(node.id, node)
      if (this.nodePortSignatures.get(node.id) !== signature || manifestChanged) {
        this.removeNodePorts(node.id)
        this.addNodePorts(node, manifests)
        this.nodePortSignatures.set(node.id, signature)
      }
      if (node.data.editorKind === 'binding' && (!previous || nodeBindingSignature(previous) !== nodeBindingSignature(node))) {
        for (const edge of this.edgeRefs.values()) if (edge.source === node.id && edge.data?.edgeKind === 'binding') this.rebuildBindingSummary(edge.target)
      }
    }

    const nextEdgeIds = new Set(edges.map((edge) => edge.id))
    for (const id of this.edgeRefs.keys()) if (!nextEdgeIds.has(id)) this.removeEdge(id)
    for (const edge of edges) {
      const signature = edgeSignature(edge)
      if (this.edgeRefs.has(edge.id) && this.edgeSignatures.get(edge.id) === signature) continue
      if (this.edgeRefs.has(edge.id)) this.removeEdge(edge.id)
      this.addEdge(edge)
      this.edgeSignatures.set(edge.id, signature)
    }
    return this.value
  }

  private removeNode(id: string) {
    this.value.nodeById.delete(id)
    this.nodeRefs.delete(id)
    this.nodePortSignatures.delete(id)
    this.removeNodePorts(id)
    this.value.bindingSummaryByNodeId.delete(id)
    for (const edge of [...this.edgeRefs.values()]) if (edge.source === id || edge.target === id) this.removeEdge(edge.id)
  }

  private removeNodePorts(id: string) {
    for (const key of this.nodePortKeys.get(id) ?? []) this.value.portByHandle.delete(key)
    this.nodePortKeys.delete(id)
  }

  private addNodePorts(node: StudioNode, manifests: Map<string, NodeManifest>) {
    const keys: string[] = []
    if (node.data.editorKind === 'binding') {
      const key = portHandleKey(node.id, 'output', 'resource')
      this.value.portByHandle.set(key, { direction: 'output', nodeId: node.id, port: { name: 'resource', kind: 'main', required: false, variadic: true } })
      keys.push(key)
    } else {
      const manifest = manifests.get(`${node.data.nodeType}@${node.data.typeVersion}`)
      for (const port of manifest?.inputPorts ?? []) keys.push(this.addPort(node.id, 'input', port))
      for (const port of manifest?.outputPorts ?? []) keys.push(this.addPort(node.id, 'output', port))
      for (const slot of manifest?.bindingSlots.filter((candidate) => candidate.placement === 'canvas') ?? []) keys.push(this.addPort(node.id, 'binding', slot, `binding:${slot.name}`))
    }
    this.nodePortKeys.set(node.id, keys)
  }

  private addPort(nodeId: string, direction: IndexedPort['direction'], port: NodePort | BindingSlot, handle = port.name) {
    const key = portHandleKey(nodeId, direction, handle)
    this.value.portByHandle.set(key, { direction, nodeId, port })
    return key
  }

  private addEdge(edge: StudioEdge) {
    this.edgeRefs.set(edge.id, edge)
    append(this.value.edgesBySourcePort, portKey(edge.source, edge.sourceHandle), edge)
    append(this.value.edgesByTargetPort, portKey(edge.target, edge.targetHandle), edge)
    if (edge.data?.edgeKind === 'binding') this.rebuildBindingSummary(edge.target)
  }

  private removeEdge(id: string) {
    const edge = this.edgeRefs.get(id)
    if (!edge) return
    this.edgeRefs.delete(id)
    this.edgeSignatures.delete(id)
    remove(this.value.edgesBySourcePort, portKey(edge.source, edge.sourceHandle), id)
    remove(this.value.edgesByTargetPort, portKey(edge.target, edge.targetHandle), id)
    if (edge.data?.edgeKind === 'binding') this.rebuildBindingSummary(edge.target)
  }

  private rebuildBindingSummary(nodeId: string) {
    const summaries: NodeBindingSummary[] = []
    for (const edge of this.edgeRefs.values()) {
      if (edge.data?.edgeKind !== 'binding' || edge.target !== nodeId) continue
      const binding = this.value.nodeById.get(edge.source)
      if (binding?.data.editorKind !== 'binding') continue
      summaries.push({ role: edge.data.targetSlot ?? binding.data.bindingRole ?? 'resource', resourceType: binding.data.resourceType, label: binding.data.resourceName ?? binding.data.label })
    }
    if (summaries.length) this.value.bindingSummaryByNodeId.set(nodeId, summaries)
    else this.value.bindingSummaryByNodeId.delete(nodeId)
  }
}

export function createGraphIndex(nodes: StudioNode[], edges: StudioEdge[], manifests: Map<string, NodeManifest>): GraphIndex {
  const controller = new IncrementalGraphIndex()
  return controller.sync(nodes, edges, manifests)
}

export function occupiedHandlesByNodeId(index: GraphIndex) {
  const handles = new Map<string, Set<string>>()
  for (const edges of index.edgesBySourcePort.values()) {
    const edge = edges[0]
    if (edge?.sourceHandle) appendHandle(handles, edge.source, edge.sourceHandle)
  }
  for (const edges of index.edgesByTargetPort.values()) {
    const edge = edges[0]
    if (edge?.data?.edgeKind === 'binding' && edge.targetHandle) appendHandle(handles, edge.target, edge.targetHandle)
  }
  return new Map([...handles].map(([nodeId, values]) => [nodeId, [...values].sort().join('\u0001')]))
}

export function resolveIndexedPort(index: GraphIndex, nodeId: string, handle: string | null | undefined, direction: IndexedPort['direction']) {
  const exact = index.portByHandle.get(portHandleKey(nodeId, direction, handle))
  if (exact || !handle) return exact
  const declared = handle.startsWith('main:') ? 'main' : handle.startsWith('case:') ? 'case' : undefined
  return declared ? index.portByHandle.get(portHandleKey(nodeId, direction, declared)) : undefined
}

function nodePortSignature(node: StudioNode, manifests: Map<string, NodeManifest>) {
  if (node.data.editorKind === 'binding') return `binding:${node.data.resourceType}`
  const manifest = manifests.get(`${node.data.nodeType}@${node.data.typeVersion}`)
  return JSON.stringify([node.data.editorKind, node.data.nodeType, node.data.typeVersion, manifest?.inputPorts.map((port) => [port.name, port.kind, port.variadic]), manifest?.outputPorts.map((port) => [port.name, port.kind, port.variadic]), manifest?.bindingSlots.map((slot) => [slot.name, slot.resourceType, slot.placement, slot.multiple])])
}

function nodeBindingSignature(node: StudioNode) {
  return node.data.editorKind === 'binding' ? `${node.data.resourceType}:${node.data.bindingRole}:${node.data.resourceName}:${node.data.label}` : ''
}

function edgeSignature(edge: StudioEdge) {
  return JSON.stringify({ id: edge.id, source: edge.source, sourceHandle: edge.sourceHandle, target: edge.target, targetHandle: edge.targetHandle, type: edge.type, data: edge.data })
}

function append<T>(map: Map<string, T[]>, key: string, value: T) {
  const values = map.get(key)
  if (values) values.push(value)
  else map.set(key, [value])
}

function remove<T extends { id: string }>(map: Map<string, T[]>, key: string, id: string) {
  const values = map.get(key)?.filter((value) => value.id !== id)
  if (values?.length) map.set(key, values)
  else map.delete(key)
}

function appendHandle(map: Map<string, Set<string>>, nodeId: string, handle: string) {
  const values = map.get(nodeId)
  if (values) values.add(handle)
  else map.set(nodeId, new Set([handle]))
}
