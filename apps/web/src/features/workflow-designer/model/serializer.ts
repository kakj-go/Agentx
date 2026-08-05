import type { WorkflowDraft } from '../../../shared/api/types'
import type { ActionNodeData, BindingNodeData, EditorDocument, StudioDocument, StudioEdge, StudioNode, WorkflowDefinition } from './types'
import { emptyEditorDocument } from './types'

export function deserializeDraft(value: WorkflowDraft): StudioDocument {
  const definition = value.definition as WorkflowDefinition
  const editor = normalizeEditor(value.editorDocument)
  const layouts = new Map(editor.nodeLayouts.map((layout) => [layout.nodeId, layout]))
  const bindingLayouts = new Map(editor.bindingLayouts.map((layout) => [layout.bindingId, layout]))
  const nodes: StudioNode[] = definition.nodes.map((item, index) => {
    const layout = layouts.get(item.id)
    return {
      id: item.id,
      type: 'manifest',
      position: { x: layout?.x ?? 120 + index * 260, y: layout?.y ?? 180 },
      width: layout?.width,
      height: layout?.height,
      data: { editorKind: 'action', nodeType: item.type, typeVersion: item.typeVersion, label: item.name, disabled: item.disabled, parameters: item.parameters ?? {}, resourceReferences: item.resourceReferences ?? [], settings: item.settings ?? {} },
    }
  })
  const seen = new Set<string>()
  for (const node of definition.nodes) {
    for (const reference of node.resourceReferences ?? []) {
      if (!reference.bindingId || seen.has(reference.bindingId)) continue
      seen.add(reference.bindingId)
      const layout = bindingLayouts.get(reference.bindingId)
      const data: BindingNodeData = { editorKind: 'binding', bindingId: reference.bindingId, bindingRole: reference.bindingRole, resourceType: reference.resourceType, resourceId: reference.resourceId, resourceVersionId: reference.resourceVersionId, operation: reference.operation === 'write' ? 'write' : reference.operation === 'read' ? 'read' : 'use', label: reference.resourceType.replaceAll('_', ' ') }
      nodes.push({ id: bindingNodeId(reference.bindingId), type: 'attachment', position: { x: layout?.x ?? 120, y: layout?.y ?? 380 }, data })
    }
  }
  const executionEdges: StudioEdge[] = definition.connections.map((connection) => ({ id: connection.id, source: connection.sourceNodeId, sourceHandle: connection.sourceHandle, target: connection.targetNodeId, targetHandle: connection.targetHandle, data: { edgeKind: 'execution', order: connection.order }, type: 'studio' }))
  const bindingEdges: StudioEdge[] = editor.bindingEdges.filter((edge) => seen.has(edge.sourceBindingId)).map((edge) => ({ id: edge.edgeId, source: bindingNodeId(edge.sourceBindingId), sourceHandle: 'resource', target: edge.targetNodeId, targetHandle: `binding:${edge.targetSlot}`, data: { edgeKind: 'binding', targetSlot: edge.targetSlot }, type: 'studio' }))
  return { nodes, edges: [...executionEdges, ...bindingEdges], viewport: editor.viewport, annotations: editor.annotations, groups: editor.groups }
}

export function serializeStudio(document: StudioDocument): { definition: WorkflowDefinition; editorDocument: EditorDocument } {
  const actionNodes = document.nodes.filter((node): node is StudioNode & { data: ActionNodeData } => node.data.editorKind === 'action')
  const bindingNodes = new Map(document.nodes.filter((node): node is StudioNode & { data: BindingNodeData } => node.data.editorKind === 'binding').map((node) => [node.id, node]))
  const bindingByTarget = new Map<string, Array<ActionNodeData['resourceReferences'][number]>>()
  for (const edge of document.edges.filter((item) => item.data?.edgeKind === 'binding')) {
    const binding = bindingNodes.get(edge.source)
    const role = edge.data?.targetSlot ?? edge.targetHandle?.replace(/^binding:/, '')
    if (!binding?.data.resourceId || !role) continue
    const references = bindingByTarget.get(edge.target) ?? []
    references.push({ bindingId: binding.data.bindingId, bindingRole: role, resourceType: binding.data.resourceType, resourceId: binding.data.resourceId, resourceVersionId: binding.data.resourceVersionId, operation: binding.data.operation })
    bindingByTarget.set(edge.target, references)
  }
  const connections = document.edges.filter((edge) => edge.data?.edgeKind !== 'binding').map((edge) => ({ id: edge.id, sourceNodeId: edge.source, sourceHandle: edge.sourceHandle ?? 'main', targetNodeId: edge.target, targetHandle: edge.targetHandle ?? 'main', order: edge.data?.order ?? 0 }))
  const grouped = new Map<string, typeof connections>()
  for (const connection of connections) { const values = grouped.get(connection.sourceNodeId) ?? []; values.push(connection); grouped.set(connection.sourceNodeId, values) }
  for (const values of grouped.values()) values.sort((a, b) => a.order - b.order || a.id.localeCompare(b.id)).forEach((value, index) => { value.order = index })
  return {
    definition: {
      schemaVersion: '3.0',
      settings: { executionOrder: 'deterministic', activationBudget: 10_000 },
      nodes: actionNodes.map((node) => ({ id: node.id, type: node.data.nodeType, typeVersion: node.data.typeVersion, name: node.data.label, disabled: node.data.disabled, parameters: node.data.parameters, resourceReferences: [...node.data.resourceReferences.filter((reference) => !reference.bindingId), ...(bindingByTarget.get(node.id) ?? [])], settings: node.data.settings })),
      connections,
    },
    editorDocument: {
      nodeLayouts: actionNodes.map((node) => ({ nodeId: node.id, x: node.position.x, y: node.position.y, width: node.measured?.width, height: node.measured?.height })),
      bindingLayouts: [...bindingNodes.values()].map((node) => ({ bindingId: node.data.bindingId, x: node.position.x, y: node.position.y })),
      edges: connections.map((edge) => ({ edgeId: edge.id })),
      bindingEdges: document.edges.filter((edge) => edge.data?.edgeKind === 'binding').map((edge) => ({ edgeId: edge.id, sourceBindingId: bindingNodes.get(edge.source)?.data.bindingId ?? '', targetNodeId: edge.target, targetSlot: edge.data?.targetSlot ?? edge.targetHandle?.replace(/^binding:/, '') ?? '' })).filter((edge) => edge.sourceBindingId && edge.targetSlot),
      annotations: document.annotations,
      groups: document.groups,
      viewport: document.viewport,
    },
  }
}

function normalizeEditor(value: unknown): EditorDocument {
  const base = emptyEditorDocument()
  if (!value || typeof value !== 'object') return base
  return { ...base, ...(value as Partial<EditorDocument>), viewport: { ...base.viewport, ...((value as Partial<EditorDocument>).viewport ?? {}) } }
}

export const bindingNodeId = (bindingId: string) => `binding:${bindingId}`
