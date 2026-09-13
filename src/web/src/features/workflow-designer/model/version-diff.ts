import type { EditorDocument, WorkflowDefinition } from './types'

export function definitionDiff(before: WorkflowDefinition | undefined, after: WorkflowDefinition, beforeEditor: EditorDocument | undefined, afterEditor: EditorDocument) {
  const beforeNodes = new Map((before?.nodes ?? []).map((node) => [node.id, JSON.stringify(node)]))
  const afterNodes = new Map(after.nodes.map((node) => [node.id, JSON.stringify(node)]))
  let changedNodes = 0
  for (const [id, value] of afterNodes) if (beforeNodes.get(id) !== value) changedNodes++
  for (const id of beforeNodes.keys()) if (!afterNodes.has(id)) changedNodes++
  const beforeConnections = JSON.stringify(before?.connections ?? [])
  const afterConnections = JSON.stringify(after.connections)
  const settingsChanged = JSON.stringify(before?.settings ?? {}) !== JSON.stringify(after.settings)
  const connectionChanged = beforeConnections !== afterConnections
  const editorChanges = JSON.stringify(beforeEditor ?? {}) === JSON.stringify(afterEditor) ? 0 : 1
  return {
    nodeSummary: `${beforeNodes.size} -> ${afterNodes.size}`,
    connectionSummary: `${before?.connections.length ?? 0} -> ${after.connections.length}`,
    definitionChanges: changedNodes + Number(connectionChanged) + Number(settingsChanged),
    editorChanges,
  }
}
