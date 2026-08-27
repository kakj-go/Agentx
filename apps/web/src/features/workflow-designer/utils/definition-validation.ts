import type { DefinitionConnection, DynamicValue, ResourceReference, StudioDocument } from "../model/types";
import { serializeStudio } from "../model/serializer";

import type { StudioIssue } from "./configuration";

export const REFERENCE_KEY_PATTERN = /^[a-z_][a-z0-9_]{0,127}$/;
const NODE_TYPE_PATTERN = /^[a-z0-9_.-]{1,128}$/;
const START_NODE_ID = "__start__";
const END_NODE_ID = "__end__";

export function isReferenceKey(value: string) {
  return REFERENCE_KEY_PATTERN.test(value);
}

export function isDynamicValueEmpty(value: DynamicValue) {
  if (value.kind === "literal") return value.value == null || (typeof value.value === "string" && !value.value.trim());
  if (value.kind === "template") return value.segments.every((segment) => segment.kind === "text" && !segment.text.trim());
  return false;
}

export function definitionIssues(document: StudioDocument): StudioIssue[] {
  const issues: StudioIssue[] = [];
  const { definition } = serializeStudio(document);
  if (!isUnsignedInteger(definition.settings.activationBudget, 32) || definition.settings.activationBudget === 0) {
    issues.push(workflowIssue("INVALID_ACTIVATION_BUDGET", "settings.activationBudget", "Activation budget must be greater than zero."));
  }
  if (!isObject(definition.start.inputs)) {
    issues.push(workflowIssue("INVALID_INPUT_SCHEMA", "start.inputs", "Start inputs must be a JSON Schema object."));
  }
  for (const [name, context] of Object.entries(definition.start.contexts)) {
    if (!isReferenceKey(name)) issues.push(workflowIssue("INVALID_CONTEXT_KEY", `start.contexts.${name}`, "Global variable names may contain only lowercase letters, digits, and underscores."));
    if (!isObject(context.schema)) issues.push(workflowIssue("INVALID_CONTEXT_SCHEMA", `start.contexts.${name}.schema`, "Global variable schema must be an object."));
    if (context.maxSize != null && (!Number.isSafeInteger(context.maxSize) || context.maxSize <= 0)) issues.push(workflowIssue("INVALID_CONTEXT_MAX_SIZE", `start.contexts.${name}.maxSize`, "Global variable maximum size must be greater than zero."));
  }
  validateOutputs(definition.end.outputs, "end.outputs", issues);
  validateOutputs(definition.end.error.outputs, "end.error.outputs", issues);
  if (!Number.isInteger(definition.end.error.collectWindowMs) || definition.end.error.collectWindowMs < 100 || definition.end.error.collectWindowMs > 60_000) {
    issues.push(workflowIssue("INVALID_ERROR_COLLECT_WINDOW", "end.error.collectWindowMs", "Error collection window must be between 100 and 60000 milliseconds."));
  }

  const ids = new Set<string>();
  const keys = new Set<string>();
  for (const node of definition.nodes) {
    const nodeIssue = (code: string, fieldPath: string, message: string): StudioIssue => ({ code, nodeId: node.id, fieldPath, message });
    if (node.id === START_NODE_ID || node.id === END_NODE_ID) issues.push(nodeIssue("RESERVED_NODE_ID", "id", "Node id is reserved for a Workflow boundary."));
    else if (!node.id || utf8Length(node.id) > 128) issues.push(nodeIssue("INVALID_NODE_ID", "id", "Node id must contain 1 to 128 bytes."));
    else if (ids.has(node.id)) issues.push(nodeIssue("DUPLICATE_NODE_ID", "id", "Node id must be unique."));
    else ids.add(node.id);

    if (!isReferenceKey(node.key)) issues.push(nodeIssue("INVALID_NODE_KEY", "key", "Node key may contain only lowercase letters, digits, and underscores."));
    else if (keys.has(node.key)) issues.push(nodeIssue("DUPLICATE_NODE_KEY", "key", "Node key must be unique within the workflow."));
    keys.add(node.key);

    if (node.type === "manual_trigger" || node.type === "remote_trigger") issues.push(nodeIssue("TRIGGER_NODE_REMOVED", "type", "Trigger nodes are not supported in Workflow Definition 6.0."));
    if (!NODE_TYPE_PATTERN.test(node.type)) issues.push(nodeIssue("INVALID_NODE_TYPE", "type", "Node type may contain only lowercase letters, digits, dots, underscores, and hyphens."));
    if (!node.name.trim() || [...node.name].length > 160) issues.push(nodeIssue("INVALID_NODE_NAME", "name", "Node name must contain 1 to 160 characters."));
    if (!isUnsignedInteger(node.typeVersion, 32) || node.typeVersion === 0) issues.push(nodeIssue("UNSUPPORTED_NODE_VERSION", "typeVersion", "Node type version must be greater than zero."));
    const maxTries = node.settings.maxTries ?? 1;
    if (typeof maxTries !== "number" || !isUnsignedInteger(maxTries, 16) || maxTries === 0) issues.push(nodeIssue("INVALID_MAX_TRIES", "settings.maxTries", "Maximum attempts must be greater than zero."));

    for (const [port, fields] of Object.entries(node.outputProjection)) {
      if (!port.trim()) issues.push(nodeIssue("INVALID_PROJECTION_PORT", "outputProjection", "Projection output port is required."));
      for (const [name, field] of Object.entries(fields)) {
        const fieldPath = `outputProjection.${port}.${name}`;
        if (!isReferenceKey(name)) issues.push(nodeIssue("INVALID_PROJECTION_FIELD_KEY", fieldPath, "Projection field names may contain only lowercase letters, digits, and underscores."));
        if (!isObject(field.schema)) issues.push(nodeIssue("INVALID_PROJECTION_SCHEMA", `${fieldPath}.schema`, "Projection field schema must be an object."));
      }
    }
    if (isResourceNode(node.type)) {
      for (const reference of node.resourceReferences) {
        if (!referenceMatchesNode(node.type, reference)) issues.push(nodeIssue("INVALID_RESOURCE_REFERENCE", "resourceReferences", "Resource type or operation does not match the node type."));
      }
    }
    if (node.type === "agent") {
      const sessionMode = (node.parameters.sessionPolicy as { mode?: unknown } | undefined)?.mode;
      if (sessionMode !== "application_session" && sessionMode !== "invocation") issues.push(nodeIssue("AGENT_SESSION_POLICY_INVALID", "parameters.sessionPolicy", "Select application_session or invocation explicitly."));
      const modelReferences = node.resourceReferences.filter((reference) => !reference.bindingId && !reference.bindingRole && reference.resourceType === "model");
      const sandboxReferences = node.resourceReferences.filter((reference) => !reference.bindingId && !reference.bindingRole && reference.resourceType === "sandbox_profile");
      const memoryReferences = node.resourceReferences.filter((reference) => reference.bindingRole === "long_term_memory");
      if (modelReferences.length !== 1) issues.push(nodeIssue("AGENT_MODEL_REQUIRED", "resourceReferences.model", "Agent requires exactly one internal Model reference."));
      if (sandboxReferences.length > 1 || memoryReferences.length > 1) issues.push(nodeIssue("AGENT_RESOURCE_SLOT_INVALID", "resourceReferences", "Agent accepts at most one Workspace Sandbox and one Long-term Memory attachment."));
      for (const reference of node.resourceReferences) {
        const inspector = reference.resourceType === "model" || reference.resourceType === "sandbox_profile";
        if (inspector && (reference.bindingId || reference.bindingRole || !reference.resourceVersionId)) issues.push(nodeIssue("AGENT_RESOURCE_SLOT_INVALID", "resourceReferences", "Inspector resources must use an exact version and must not have bindingId or bindingRole."));
        if (!inspector && (!reference.bindingId || !reference.bindingRole)) issues.push(nodeIssue("AGENT_RESOURCE_SLOT_INVALID", "resourceReferences", "Canvas attachments require bindingId and bindingRole."));
      }
    }
  }

  validateConnections(definition.connections, ids, issues);
  return issues;
}

function validateOutputs(outputs: StudioDocument["end"]["outputs"], path: string, issues: StudioIssue[]) {
  for (const [name, output] of Object.entries(outputs)) {
    if (!isReferenceKey(name)) issues.push(workflowIssue("INVALID_END_OUTPUT_KEY", `${path}.${name}`, "End output names may contain only lowercase letters, digits, and underscores."));
    if (!isObject(output.schema)) issues.push(workflowIssue("INVALID_END_OUTPUT_SCHEMA", `${path}.${name}.schema`, "End output schema must be an object."));
    if (isDynamicValueEmpty(output.value)) issues.push(workflowIssue("END_OUTPUT_EXPRESSION_REQUIRED", `${path}.${name}.value`, "End output expressions are required."));
  }
}

function validateConnections(connections: DefinitionConnection[], nodeIds: Set<string>, issues: StudioIssue[]) {
  const ids = new Set<string>();
  const orders = new Set<string>();
  for (const connection of connections) {
    if (!connection.id || ids.has(connection.id)) issues.push(workflowIssue("INVALID_CONNECTION_ID", "connections", "Connection id must be present and unique."));
    ids.add(connection.id);
    if (!connection.sourceHandle || !connection.targetHandle) issues.push(workflowIssue("INVALID_CONNECTION_HANDLE", "connections", "Connection handles are required."));
    const orderKey = `${connection.sourceNodeId}\u0000${connection.sourceHandle}\u0000${connection.order}`;
    if (orders.has(orderKey)) issues.push(workflowIssue("DUPLICATE_CONNECTION_ORDER", "connections", "Connection order must be unique for each source node and source handle."));
    orders.add(orderKey);

    const sourceIsStart = connection.sourceNodeId === START_NODE_ID;
    const sourceIsEnd = connection.sourceNodeId === END_NODE_ID;
    const targetIsStart = connection.targetNodeId === START_NODE_ID;
    const targetIsEnd = connection.targetNodeId === END_NODE_ID;
    if (sourceIsEnd || targetIsStart) {
      issues.push(workflowIssue("INVALID_BOUNDARY_DIRECTION", "connections", "Start can only be a connection source and End can only be a connection target."));
      continue;
    }
    if (sourceIsStart && connection.sourceHandle !== "main") issues.push(workflowIssue("INVALID_START_PORT", "connections", "Start only exposes the main output port."));
    if (targetIsEnd && connection.targetHandle !== "main" && connection.targetHandle !== "error") issues.push(workflowIssue("INVALID_END_PORT", "connections", "End only accepts main or error input ports."));
    if (sourceIsStart && targetIsEnd && connection.targetHandle !== "main") issues.push(workflowIssue("INVALID_DIRECT_ERROR_CONNECTION", "connections", "Start can only connect directly to End.main."));
    if ((!sourceIsStart && !nodeIds.has(connection.sourceNodeId)) || (!targetIsEnd && !nodeIds.has(connection.targetNodeId))) issues.push(workflowIssue("DANGLING_CONNECTION", "connections", "Connection references a missing node."));
  }
}

function referenceMatchesNode(nodeType: string, reference: ResourceReference) {
  if (nodeType === "model") return reference.resourceType === "model" && reference.operation === "use";
  if (nodeType === "mcp_tool") return reference.resourceType === "mcp_tool" && reference.operation === "use";
  if (nodeType === "skill") return reference.resourceType === "skill" && reference.operation === "use";
  if (nodeType === "rag") return reference.resourceType === "rag" && (reference.operation === "read" || reference.operation === "write");
  if (nodeType === "memory") return reference.resourceType === "memory" && (reference.operation === "read" || reference.operation === "write");
  if (nodeType === "code") return (reference.resourceType === "sandbox_profile" || reference.resourceType === "credential") && reference.operation === "use";
  if (nodeType !== "agent") return false;
  if (!reference.bindingId && !reference.bindingRole && reference.resourceType === "model") return reference.operation === "use" && Boolean(reference.resourceVersionId);
  if (!reference.bindingId && !reference.bindingRole && reference.resourceType === "sandbox_profile") return reference.operation === "use" && Boolean(reference.resourceVersionId);
  const role = reference.bindingRole;
  if (role === "mcp_tools") return Boolean(reference.bindingId) && reference.resourceType === "mcp_tool" && reference.operation === "use";
  if (role === "skills") return Boolean(reference.bindingId) && reference.resourceType === "skill" && reference.operation === "use";
  if (role === "knowledge") return Boolean(reference.bindingId) && reference.resourceType === "rag" && reference.operation === "read";
  return role === "long_term_memory" && Boolean(reference.bindingId) && reference.resourceType === "memory" && (reference.operation === "read" || reference.operation === "write");
}

function isResourceNode(nodeType: string) {
  return ["model", "mcp_tool", "skill", "rag", "memory", "agent", "code"].includes(nodeType);
}

function isObject(value: unknown): value is Record<string, unknown> {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}

function isUnsignedInteger(value: number, bits: 16 | 32) {
  const maximum = bits === 16 ? 0xffff : 0xffffffff;
  return Number.isInteger(value) && value >= 0 && value <= maximum;
}

function utf8Length(value: string) {
  return new TextEncoder().encode(value).length;
}

function workflowIssue(code: string, fieldPath: string, message: string): StudioIssue {
  return { code, fieldPath, message };
}
