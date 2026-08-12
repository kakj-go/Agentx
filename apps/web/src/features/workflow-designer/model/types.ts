import type { Edge, Node, Viewport } from "@xyflow/react";

export type PortKind = "main" | "error";
export type ResourceType =
  | "credential"
  | "model"
  | "mcp_server"
  | "mcp_tool"
  | "skill"
  | "rag"
  | "memory"
  | "sandbox_profile";

export type NodePort = {
  name: string;
  kind: PortKind;
  required: boolean;
  variadic: boolean;
};
export type BindingSlot = {
  name: string;
  resourceType: ResourceType;
  required: boolean;
  multiple: boolean;
};
export type JsonSchemaProperty = {
  type?: string;
  title?: string;
  description?: string;
  default?: unknown;
  enum?: unknown[];
  minimum?: number;
  maximum?: number;
  multipleOf?: number;
  minLength?: number;
  maxLength?: number;
  pattern?: string;
  minItems?: number;
  maxItems?: number;
  uniqueItems?: boolean;
  items?: JsonSchemaProperty;
  properties?: Record<string, JsonSchemaProperty>;
  additionalProperties?: boolean | JsonSchemaProperty;
  required?: string[];
  format?: string;
  templatable?: boolean;
  allowedNamespaces?: Array<
    "inputs" | "outputs" | "contexts" | "execution" | "item" | "loop"
  >;
  expectedType?: string;
  multiline?: boolean;
  richText?: boolean;
};
export type ParameterSchema = {
  type?: string;
  required?: string[];
  properties?: Record<string, JsonSchemaProperty>;
  additionalProperties?: boolean;
  "x-agentx-workflowId"?: string;
  "x-agentx-workflowVersionId"?: string;
  "x-agentx-versionNumber"?: number;
  "x-agentx-contextContract"?: Record<string, ContextDefinition>;
};
export type CanvasNodeRole =
  | "default"
  | "trigger"
  | "branch"
  | "flow"
  | "merge"
  | "loop"
  | "suspend"
  | "approval"
  | "sub_workflow"
  | "agent"
  | "code"
  | "error_handler";
export type CanvasNodeFamily = "compact" | "agent" | "attachment" | "editor";
export type ConnectionInteractionState =
  | { status: "idle" | "cancelled" | "committed" }
  | {
      status: "source-hover" | "connecting";
      nodeId: string;
      handleId?: string | null;
    }
  | { status: "compatible" | "incompatible" | "occupied"; reason?: string };
export type UiField = {
  control: string;
  label?: string;
  unit?: string;
  languageField?: string;
  provider?: string;
  options?: Array<{ value: string; label: string }>;
  visibleWhen?: { field: string; equals: unknown };
};
export type NodeUiSchema = {
  order?: string[];
  fields?: Record<string, UiField>;
  resourceSelectors?: unknown[];
  canvas?: { role?: CanvasNodeRole };
};

export type NodeManifest = {
  protocolVersion: string;
  nodeType: string;
  version: number;
  displayName: string;
  description: string;
  category: string;
  keywords: string[];
  iconKey: string;
  executionStyle: "action" | "trigger" | "suspend" | "sub_workflow";
  capability: string;
  readiness: "any" | "all" | "required";
  inputPorts: NodePort[];
  outputPorts: NodePort[];
  bindingSlots: BindingSlot[];
  parameterSchema: ParameterSchema;
  outputSchema?: unknown;
  outputPortSchemas?: Record<string, unknown>;
  outputCardinality?: Record<
    string,
    "zero_or_one" | "exactly_one" | "zero_or_many" | "many"
  >;
  expressionCapabilities?: {
    namespaces?: string[];
    supportsCurrent?: boolean;
    supportsFirstLast?: boolean;
    supportsAll?: boolean;
    supportsRunSelection?: boolean;
  };
  contextReadCapability?: boolean;
  contextWriteCapability?: boolean;
  outputProjectionSchema?: unknown;
  artifactOutputSchema?: unknown;
  uiSchema: NodeUiSchema;
  providers: string[];
  credentials: Array<{ credentialType: string; required: boolean }>;
  defaultTimeoutMs?: number | null;
  retryPolicy: {
    retryable: boolean;
    maxAttempts: number;
    initialBackoffMs: number;
    maxBackoffMs: number;
  };
  sandboxRequired: boolean;
  supportsMock: boolean;
  sideEffectLevel: "none" | "idempotent" | "reversible" | "irreversible";
  localizations?: Record<string, NodeManifestLocalization>;
};

export type NodeManifestLocalization = {
  displayName?: string;
  description?: string;
  keywords?: string[];
  inputPortLabels?: Record<string, string>;
  outputPortLabels?: Record<string, string>;
  bindingSlotLabels?: Record<string, string>;
  parameterLabels?: Record<string, string>;
  parameterDescriptions?: Record<string, string>;
  parameterPlaceholders?: Record<string, string>;
  parameterEnumOptions?: Record<string, Record<string, string>>;
};

export type ResourceReference = {
  bindingId?: string;
  bindingRole?: string;
  resourceType: ResourceType;
  resourceId: string;
  resourceVersionId?: string | null;
  operation: "view" | "use" | "read" | "write" | "manage";
};

export type DefinitionNode = {
  id: string;
  key: string;
  type: string;
  typeVersion: number;
  name: string;
  disabled: boolean;
  parameters: Record<string, unknown>;
  outputProjection: OutputProjection;
  contextWrites: ContextWrite[];
  resourceReferences: ResourceReference[];
  settings: Record<string, unknown>;
};
export type OutputProjectionField = {
  expression: string;
  schema: unknown;
  sensitive: boolean;
};
export type OutputProjection = Record<string, Record<string, OutputProjectionField>>;
export type DefinitionConnection = {
  id: string;
  sourceNodeId: string;
  sourceHandle: string;
  targetNodeId: string;
  targetHandle: string;
  order: number;
};
export type WorkflowSettings = {
  executionOrder: "deterministic" | "parallel";
  activationBudget: number;
  timeoutMs?: number | null;
};
export type ContextDefinition = {
  title?: string;
  description?: string;
  schema: unknown;
  default: unknown;
  mutable: boolean;
  sensitive: boolean;
  clientWritable: boolean;
  scope: "execution_tree" | "session";
  maxSize?: number | null;
  mergePolicy:
    "replace" | "append" | "merge_object" | "increment" | "reject_conflict";
};
export type ContextWrite = {
  operation:
    | "set"
    | "set_if_absent"
    | "delete"
    | "append"
    | "merge_object"
    | "increment"
    | "min"
    | "max"
    | "compare_and_set";
  path: string;
  value: unknown;
};
export type WorkflowOutput = {
  expression: string;
  schema: unknown;
  required: boolean;
  sensitive: boolean;
};
export type EndErrorStrategy = "fail_fast" | "collect";
export type WorkflowErrorEnd = {
  strategy: EndErrorStrategy;
  collectWindowMs: number;
  outputs: Record<string, WorkflowOutput>;
};
export type WorkflowStart = {
  inputs: unknown;
  contexts: Record<string, ContextDefinition>;
};
export type WorkflowEnd = { outputs: Record<string, WorkflowOutput>; error: WorkflowErrorEnd };
export type WorkflowDefinition = {
  schemaVersion: "4.0";
  start: WorkflowStart;
  nodes: DefinitionNode[];
  connections: DefinitionConnection[];
  end: WorkflowEnd;
  settings: WorkflowSettings;
};

export type EditorDocument = {
  nodeLayouts: Array<{
    nodeId: string;
    x: number;
    y: number;
    width?: number;
    height?: number;
    collapsed?: boolean;
  }>;
  boundaryLayouts: Array<{ boundary: "start" | "end"; x: number; y: number }>;
  bindingLayouts: Array<{ bindingId: string; x: number; y: number }>;
  edges: Array<{ edgeId: string; labelPosition?: number }>;
  bindingEdges: Array<{
    edgeId: string;
    sourceBindingId: string;
    targetNodeId: string;
    targetSlot: string;
  }>;
  annotations: Array<{
    id: string;
    text: string;
    x: number;
    y: number;
    width?: number;
    height?: number;
    color?: string;
  }>;
  groups: Array<{
    id: string;
    label: string;
    nodeIds: string[];
    collapsed?: boolean;
    color?: string;
  }>;
  viewport: Viewport;
};

export type ActionNodeData = {
  editorKind: "action";
  nodeType: string;
  typeVersion: number;
  label: string;
  key: string;
  parameters: Record<string, unknown>;
  outputProjection: OutputProjection;
  contextWrites: ContextWrite[];
  resourceReferences: ResourceReference[];
  settings: Record<string, unknown>;
  disabled: boolean;
};
export type BindingNodeData = {
  editorKind: "binding";
  bindingId: string;
  bindingRole?: string;
  resourceType: ResourceType;
  resourceId?: string;
  resourceVersionId?: string | null;
  resourceName?: string;
  operation: "use" | "read" | "write";
  label: string;
};
export type GroupNodeData = {
  editorKind: "group";
  groupId: string;
  label: string;
  collapsed: boolean;
  color?: string;
  memberCount: number;
  onToggle: () => void;
  onRemove: () => void;
};
export type AnnotationNodeData = {
  editorKind: "annotation";
  annotationId: string;
  text: string;
  color?: string;
  onChange: (patch: Partial<EditorDocument["annotations"][number]>) => void;
  onRemove: () => void;
  onResizeStart: () => void;
  onResize: (frame: {
    x: number;
    y: number;
    width: number;
    height: number;
  }) => void;
  onResizeEnd: () => void;
};
export type BoundaryNodeData = {
  editorKind: "boundary";
  boundary: "start" | "end";
  label: string;
};
export type StudioNodeData = ActionNodeData | BindingNodeData;
export type StudioNode = Node<StudioNodeData, "manifest" | "attachment">;
export type CanvasNodeData =
  StudioNodeData | GroupNodeData | AnnotationNodeData | BoundaryNodeData;
export type CanvasNode = Node<
  CanvasNodeData,
  "manifest" | "attachment" | "group" | "annotation" | "boundary"
>;
export type StudioEdgeData = {
  edgeKind: "execution" | "binding";
  order?: number;
  targetSlot?: string;
  sourcePortKind?: PortKind;
  runtimeStatus?: string;
};
export type StudioEdge = Edge<StudioEdgeData>;

export type StudioDocument = {
  start: WorkflowStart;
  nodes: StudioNode[];
  edges: StudioEdge[];
  end: WorkflowEnd;
  viewport: Viewport;
  boundaryLayouts: EditorDocument["boundaryLayouts"];
  annotations: EditorDocument["annotations"];
  groups: EditorDocument["groups"];
  settings: WorkflowSettings;
};
export type ResourceOption = {
  value: string;
  label: string;
  resourceType?: ResourceType;
  operation?: "view" | "use" | "read" | "write" | "manage";
  versionId?: string | null;
  detail?: string;
  status?: string;
  accessState?: "authorized" | "grantable" | "requestable" | "pending" | "rejected" | "unavailable";
  pendingRequestId?: string | null;
  requirements?: Array<{
    resourceType: string;
    resourceId: string;
    resourceVersionId?: string | null;
    operation: string;
    name?: string | null;
    requiredByResourceId?: string | null;
    ownerDepartmentId?: string | null;
    authorized: boolean;
    active: boolean;
  }>;
};
export type ResourceRequestContext = {
  sourceNodeId?: string;
  message?: string;
};
export type ReferenceNamespace =
  | "inputs"
  | "outputs"
  | "contexts"
  | "item"
  | "execution"
  | "loop";
export type ReferenceEntry = {
  id: string;
  label: string;
  path: string;
  expression?: string;
  type?: string;
  cardinality?: "zero_or_one" | "exactly_one" | "many" | "zero_or_many";
  nullable?: boolean;
  sensitive?: boolean;
  scope?: "execution_tree" | "session";
  example?: unknown;
  disabledReason?: string;
  children: ReferenceEntry[];
};
export type ReferenceCatalog = Record<
  "inputs" | "outputs" | "contexts",
  ReferenceEntry[]
> &
  Partial<Record<"item" | "execution" | "loop", ReferenceEntry[]>>;

export const emptyEditorDocument = (): EditorDocument => ({
  nodeLayouts: [],
  boundaryLayouts: [],
  bindingLayouts: [],
  edges: [],
  bindingEdges: [],
  annotations: [],
  groups: [],
  viewport: { x: 0, y: 0, zoom: 1 },
});
