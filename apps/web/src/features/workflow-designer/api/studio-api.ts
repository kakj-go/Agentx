import type { components } from '../../../shared/api/generated'
import { apiRequest, jsonBody } from '../../../shared/api/client'
import type { NodeManifest } from '../model/types'

export type NodeDefinitionSummary = components['schemas']['NodeDefinitionSummary']
export type ExecutionEvent = components['schemas']['ExecutionEventResponse']
export type ExecutionCommand = components['schemas']['ExecutionCommandResponse']
export type DebugOverlay = components['schemas']['DebugOverlayResponse']

export async function loadNodeCatalog() {
  const page = await apiRequest<{ items: NodeDefinitionSummary[] }>('/node-definitions?pageSize=100')
  const details = await Promise.all(page.items.map((item) => apiRequest<components['schemas']['NodeDefinitionDetail']>(`/node-definitions/${encodeURIComponent(item.nodeType)}/versions/${item.version}`)))
  return details.map((detail) => ({ ...detail, manifest: detail.manifest as NodeManifest }))
}

export const saveDraft = (workflowId: string, expectedRevision: number, definition: unknown, editorDocument: unknown) => apiRequest<components['schemas']['DraftResponse']>(`/workflows/${workflowId}/draft`, { method: 'PUT', headers: { 'Idempotency-Key': crypto.randomUUID() }, body: jsonBody({ expectedRevision, definition, editorDocument }) })
export const validateDraft = (workflowId: string, definition: unknown, editorDocument: unknown) => apiRequest<components['schemas']['ValidateDraftResponse']>(`/workflows/${workflowId}/draft/validate`, { method: 'POST', body: jsonBody({ definition, editorDocument }) })
export const startDebugExecution = (workflowId: string, request: components['schemas']['DebugExecutionRequest']) => apiRequest<ExecutionCommand>(`/workflows/${workflowId}/debug-executions`, { method: 'POST', body: jsonBody(request) })
export const loadExecutionEvents = (executionId: string, after = 0) => apiRequest<components['schemas']['ExecutionEventListResponse']>(`/executions/${executionId}/events?after=${after}&limit=200`)
export const cancelExecution = (executionId: string) => apiRequest<void>(`/executions/${executionId}/cancel`, { method: 'POST' })
export const saveOverlay = (workflowId: string, nodeId: string, kind: string, payload: unknown) => apiRequest<DebugOverlay>(`/workflows/${workflowId}/debug-overlays/${encodeURIComponent(nodeId)}`, { method: 'PUT', body: jsonBody({ kind, payload }) })
export const deleteOverlay = (workflowId: string, nodeId: string) => apiRequest<void>(`/workflows/${workflowId}/debug-overlays/${encodeURIComponent(nodeId)}`, { method: 'DELETE' })
