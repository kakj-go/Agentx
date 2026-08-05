import type { Edge, Node, Viewport } from '@xyflow/react'

export type PortKind = 'main' | 'error'
export type ResourceType = 'credential' | 'model' | 'mcp_server' | 'mcp_tool' | 'skill' | 'rag' | 'memory' | 'sandbox_profile'

export type NodePort = { name: string; kind: PortKind; required: boolean; variadic: boolean }
export type BindingSlot = { name: string; resourceType: ResourceType; required: boolean; multiple: boolean }
export type JsonSchemaProperty = {
  type?: string
  title?: string
  description?: string
  default?: unknown
  enum?: unknown[]
  minimum?: number
  maximum?: number
  items?: JsonSchemaProperty
  properties?: Record<string, JsonSchemaProperty>
  required?: string[]
  format?: string
}
export type ParameterSchema = { type?: string; required?: string[]; properties?: Record<string, JsonSchemaProperty>; additionalProperties?: boolean }
export type UiField = {
  control: string
  label?: string
  languageField?: string
  provider?: string
  options?: Array<{ value: string; label: string }>
  visibleWhen?: { field: string; equals: unknown }
}
export type NodeUiSchema = {
  order?: string[]
  fields?: Record<string, UiField>
  resourceSelectors?: unknown[]
}

export type NodeManifest = {
  protocolVersion: string
  nodeType: string
  version: number
  displayName: string
  description: string
  category: string
  keywords: string[]
  iconKey: string
  executionStyle: 'action' | 'trigger' | 'suspend' | 'sub_workflow'
  capability: string
  readiness: 'any' | 'all' | 'required'
  inputPorts: NodePort[]
  outputPorts: NodePort[]
  bindingSlots: BindingSlot[]
  parameterSchema: ParameterSchema
  uiSchema: NodeUiSchema
  providers: string[]
  credentials: Array<{ credentialType: string; required: boolean }>
  defaultTimeoutMs?: number | null
  retryPolicy: { retryable: boolean; maxAttempts: number; initialBackoffMs: number; maxBackoffMs: number }
  sandboxRequired: boolean
  supportsMock: boolean
  sideEffectLevel: 'none' | 'idempotent' | 'reversible' | 'irreversible'
}

export type ResourceReference = {
  bindingId?: string
  bindingRole?: string
  resourceType: ResourceType
  resourceId: string
  resourceVersionId?: string | null
  operation: 'view' | 'use' | 'read' | 'write' | 'manage'
}

export type DefinitionNode = {
  id: string
  type: string
  typeVersion: number
  name: string
  disabled: boolean
  parameters: Record<string, unknown>
  resourceReferences: ResourceReference[]
  settings: Record<string, unknown>
}
export type DefinitionConnection = { id: string; sourceNodeId: string; sourceHandle: string; targetNodeId: string; targetHandle: string; order: number }
export type WorkflowDefinition = { schemaVersion: '3.0'; nodes: DefinitionNode[]; connections: DefinitionConnection[]; settings: { executionOrder: 'deterministic' | 'parallel'; activationBudget: number; timeoutMs?: number | null } }

export type EditorDocument = {
  nodeLayouts: Array<{ nodeId: string; x: number; y: number; width?: number; height?: number; collapsed?: boolean }>
  bindingLayouts: Array<{ bindingId: string; x: number; y: number }>
  edges: Array<{ edgeId: string; labelPosition?: number }>
  bindingEdges: Array<{ edgeId: string; sourceBindingId: string; targetNodeId: string; targetSlot: string }>
  annotations: Array<{ id: string; text: string; x: number; y: number; color?: string }>
  groups: Array<{ id: string; label: string; nodeIds: string[]; color?: string }>
  viewport: Viewport
}

export type ActionNodeData = {
  editorKind: 'action'
  nodeType: string
  typeVersion: number
  label: string
  parameters: Record<string, unknown>
  resourceReferences: ResourceReference[]
  settings: Record<string, unknown>
  disabled: boolean
}
export type BindingNodeData = {
  editorKind: 'binding'
  bindingId: string
  bindingRole?: string
  resourceType: ResourceType
  resourceId?: string
  resourceVersionId?: string | null
  resourceName?: string
  operation: 'use' | 'read' | 'write'
  label: string
}
export type StudioNodeData = ActionNodeData | BindingNodeData
export type StudioNode = Node<StudioNodeData, 'manifest' | 'attachment'>
export type StudioEdgeData = { edgeKind: 'execution' | 'binding'; order?: number; targetSlot?: string }
export type StudioEdge = Edge<StudioEdgeData>

export type StudioDocument = { nodes: StudioNode[]; edges: StudioEdge[]; viewport: Viewport; annotations: EditorDocument['annotations']; groups: EditorDocument['groups'] }
export type ResourceOption = { value: string; label: string; versionId?: string | null }

export const emptyEditorDocument = (): EditorDocument => ({ nodeLayouts: [], bindingLayouts: [], edges: [], bindingEdges: [], annotations: [], groups: [], viewport: { x: 0, y: 0, zoom: 1 } })
