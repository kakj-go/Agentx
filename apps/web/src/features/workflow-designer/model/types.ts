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
export type CanvasNodeRole = 'default' | 'trigger' | 'branch' | 'flow' | 'merge' | 'loop' | 'suspend' | 'approval' | 'sub_workflow' | 'agent' | 'code' | 'error_handler'
export type CanvasNodeFamily = 'compact' | 'agent' | 'attachment' | 'editor'
export type ConnectionInteractionState =
  | { status: 'idle' | 'cancelled' | 'committed' }
  | { status: 'source-hover' | 'connecting'; nodeId: string; handleId?: string | null }
  | { status: 'compatible' | 'incompatible' | 'occupied'; reason?: string }
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
  canvas?: { role?: CanvasNodeRole }
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
  localizations?: Record<string, NodeManifestLocalization>
}

export type NodeManifestLocalization = {
  displayName?: string
  description?: string
  keywords?: string[]
  inputPortLabels?: Record<string, string>
  outputPortLabels?: Record<string, string>
  bindingSlotLabels?: Record<string, string>
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
export type WorkflowSettings = { executionOrder: 'deterministic' | 'parallel'; activationBudget: number; timeoutMs?: number | null; primaryOutputNodeId?: string | null }
export type WorkflowDefinition = { schemaVersion: '3.0'; nodes: DefinitionNode[]; connections: DefinitionConnection[]; settings: WorkflowSettings }

export type EditorDocument = {
  nodeLayouts: Array<{ nodeId: string; x: number; y: number; width?: number; height?: number; collapsed?: boolean }>
  bindingLayouts: Array<{ bindingId: string; x: number; y: number }>
  edges: Array<{ edgeId: string; labelPosition?: number }>
  bindingEdges: Array<{ edgeId: string; sourceBindingId: string; targetNodeId: string; targetSlot: string }>
  annotations: Array<{ id: string; text: string; x: number; y: number; width?: number; height?: number; color?: string }>
  groups: Array<{ id: string; label: string; nodeIds: string[]; collapsed?: boolean; color?: string }>
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
export type GroupNodeData = { editorKind: 'group'; groupId: string; label: string; collapsed: boolean; color?: string; memberCount: number; onToggle: () => void; onRemove: () => void }
export type AnnotationNodeData = { editorKind: 'annotation'; annotationId: string; text: string; color?: string; onChange: (patch: Partial<EditorDocument['annotations'][number]>) => void; onRemove: () => void; onResizeStart: () => void; onResize: (frame: { x: number; y: number; width: number; height: number }) => void; onResizeEnd: () => void }
export type StudioNodeData = ActionNodeData | BindingNodeData
export type StudioNode = Node<StudioNodeData, 'manifest' | 'attachment'>
export type CanvasNodeData = StudioNodeData | GroupNodeData | AnnotationNodeData
export type CanvasNode = Node<CanvasNodeData, 'manifest' | 'attachment' | 'group' | 'annotation'>
export type StudioEdgeData = { edgeKind: 'execution' | 'binding'; order?: number; targetSlot?: string; sourcePortKind?: PortKind; runtimeStatus?: string }
export type StudioEdge = Edge<StudioEdgeData>

export type StudioDocument = { nodes: StudioNode[]; edges: StudioEdge[]; viewport: Viewport; annotations: EditorDocument['annotations']; groups: EditorDocument['groups']; settings: WorkflowSettings }
export type ResourceOption = { value: string; label: string; versionId?: string | null }

export const emptyEditorDocument = (): EditorDocument => ({ nodeLayouts: [], bindingLayouts: [], edges: [], bindingEdges: [], annotations: [], groups: [], viewport: { x: 0, y: 0, zoom: 1 } })
