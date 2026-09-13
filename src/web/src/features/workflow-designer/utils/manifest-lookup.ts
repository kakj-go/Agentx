import type { NodeManifest, StudioNode } from '../model/types'

export function manifestForNode(manifests: Map<string, NodeManifest>, node: StudioNode) {
  if (node.data.editorKind !== 'action') return undefined
  const exact = manifests.get(`node:${node.id}`)
  if (exact) return exact
  if (node.data.nodeType === 'sub_workflow' && typeof node.data.parameters.workflowVersionId === 'string') {
    const derived = `workflow.${node.data.parameters.workflowVersionId.replaceAll('-', '')}@${node.data.typeVersion}`
    const manifest = manifests.get(derived)
    if (manifest) return manifest
  }
  return manifests.get(`${node.data.nodeType}@${node.data.typeVersion}`)
}

