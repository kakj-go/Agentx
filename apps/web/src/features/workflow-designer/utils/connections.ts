import type { NodeManifest, StudioEdge, StudioNode } from '../model/types'

type ConnectionCandidate = {
  source: string
  sourceHandle?: string | null
  target: string
  targetHandle?: string | null
}

export function validateConnection(connection: ConnectionCandidate, nodes: StudioNode[], edges: StudioEdge[], manifests: Map<string, NodeManifest>) {
  const source = nodes.find((node) => node.id === connection.source)
  const target = nodes.find((node) => node.id === connection.target)
  if (!source || !target || source.id === target.id) return false
  if (source.data.editorKind === 'binding') {
    if (target.data.editorKind !== 'action' || connection.sourceHandle !== 'resource' || !connection.targetHandle?.startsWith('binding:')) return false
    const slotName = connection.targetHandle.slice('binding:'.length)
    const manifest = manifests.get(`${target.data.nodeType}@${target.data.typeVersion}`)
    const slot = manifest?.bindingSlots.find((item) => item.name === slotName)
    if (!slot || slot.resourceType !== source.data.resourceType) return false
    return slot.multiple || !edges.some((edge) => edge.data?.edgeKind === 'binding' && edge.target === target.id && edge.data.targetSlot === slotName)
  }
  if (target.data.editorKind !== 'action' || connection.targetHandle?.startsWith('binding:')) return false
  const sourceManifest = manifests.get(`${source.data.nodeType}@${source.data.typeVersion}`)
  const targetManifest = manifests.get(`${target.data.nodeType}@${target.data.typeVersion}`)
  return Boolean(sourceManifest?.outputPorts.some((port) => handleMatches(port.name, connection.sourceHandle)) && targetManifest?.inputPorts.some((port) => handleMatches(port.name, connection.targetHandle)))
}

const handleMatches = (declared: string, actual?: string | null) => declared === actual || (declared === 'main' && actual?.startsWith('main:')) || (declared === 'case' && actual?.startsWith('case:'))
