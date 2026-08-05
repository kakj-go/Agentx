import { SUPPORTED_CONTROLS } from '../forms/parameter-field'
import type { NodeManifest, StudioDocument } from '../model/types'

export type StudioIssue = { code: string; message: string; nodeId?: string; fieldPath?: string }

export function configurationIssues(document: StudioDocument, manifests: Map<string, NodeManifest>): StudioIssue[] {
  const issues: StudioIssue[] = []
  for (const node of document.nodes) {
    if (node.data.editorKind === 'binding') {
      if (!node.data.resourceId) issues.push({ code: 'ATTACHMENT_RESOURCE_REQUIRED', nodeId: node.id, fieldPath: 'resourceId', message: 'Select a resource for this AI attachment.' })
      continue
    }
    const manifest = manifests.get(`${node.data.nodeType}@${node.data.typeVersion}`)
    if (!manifest) {
      issues.push({ code: 'UNKNOWN_NODE_VERSION', nodeId: node.id, fieldPath: 'typeVersion', message: `${node.data.nodeType}@${node.data.typeVersion} is unavailable in the Node Catalog.` })
      continue
    }
    for (const name of Object.keys(manifest.parameterSchema.properties ?? {})) {
      const control = manifest.uiSchema.fields?.[name]?.control
      if (!control || !SUPPORTED_CONTROLS.has(control)) issues.push({ code: 'UNSUPPORTED_UI_CONTROL', nodeId: node.id, fieldPath: `parameters.${name}`, message: `Field '${name}' uses unsupported UI control '${control ?? 'missing'}'.` })
    }
    for (const name of manifest.parameterSchema.required ?? []) {
      const value = node.data.parameters[name]
      if (value === undefined || value === null || value === '') issues.push({ code: 'REQUIRED_PARAMETER_MISSING', nodeId: node.id, fieldPath: `parameters.${name}`, message: `Field '${name}' is required.` })
    }
    for (const selector of resourceSelectors(manifest)) {
      if (selector.required && !node.data.resourceReferences.some((reference) => !reference.bindingId && reference.resourceType === selector.resourceType)) issues.push({ code: 'RESOURCE_REQUIRED', nodeId: node.id, fieldPath: 'resourceReferences', message: `Select a ${selector.resourceType} resource.` })
    }
    for (const slot of manifest.bindingSlots) {
      if (slot.required && !document.edges.some((edge) => edge.data?.edgeKind === 'binding' && edge.target === node.id && edge.data.targetSlot === slot.name)) issues.push({ code: 'AI_BINDING_REQUIRED', nodeId: node.id, fieldPath: `resourceReferences.${slot.name}`, message: `Connect the required ${slot.name} attachment.` })
    }
  }
  return issues
}

function resourceSelectors(manifest: NodeManifest): Array<{ resourceType: string; required?: boolean }> {
  const selectors = manifest.uiSchema.resourceSelectors
  return Array.isArray(selectors) ? selectors.filter((value): value is { resourceType: string; required?: boolean } => Boolean(value && typeof value === 'object' && typeof (value as { resourceType?: unknown }).resourceType === 'string')) : []
}
