import { SUPPORTED_CONTROLS } from '../forms/parameter-field'
import type { NodeManifest, StudioDocument } from '../model/types'
import { definitionIssues } from './definition-validation'

export type StudioIssue = { code: string; message: string; nodeId?: string; fieldPath?: string; values?: Record<string, string> }

export function configurationIssues(document: StudioDocument, manifests: Map<string, NodeManifest>): StudioIssue[] {
  const issues: StudioIssue[] = definitionIssues(document)
  for (const node of document.nodes) {
    if (node.data.editorKind === 'exit') continue
    const manifest = manifests.get(`${node.data.nodeType}@${node.data.typeVersion}`)
    if (!manifest) {
      issues.push({ code: 'UNKNOWN_NODE_VERSION', nodeId: node.id, fieldPath: 'typeVersion', message: `${node.data.nodeType}@${node.data.typeVersion} is unavailable in the Node Catalog.`, values: { nodeTypeVersion: `${node.data.nodeType}@${node.data.typeVersion}` } })
      continue
    }
    for (const name of Object.keys(manifest.parameterSchema.properties ?? {})) {
      const control = manifest.uiSchema.fields?.[name]?.control
      if (!control || !SUPPORTED_CONTROLS.has(control)) issues.push({ code: 'UNSUPPORTED_UI_CONTROL', nodeId: node.id, fieldPath: `parameters.${name}`, message: `Field '${name}' uses unsupported UI control '${control ?? 'missing'}'.`, values: { field: name, control: control ?? 'missing' } })
    }
    for (const name of manifest.parameterSchema.required ?? []) {
      const value = node.data.parameters[name]
      if (value === undefined || value === null || value === '') issues.push({ code: 'REQUIRED_PARAMETER_MISSING', nodeId: node.id, fieldPath: `parameters.${name}`, message: `Field '${name}' is required.`, values: { field: name } })
    }
    for (const selector of resourceSelectors(manifest).filter((candidate) => !candidate.bindingRole)) {
      if (selector.required && !node.data.resourceReferences.some((reference) => !reference.bindingRole && reference.resourceType === selector.resourceType)) issues.push({ code: 'RESOURCE_REQUIRED', nodeId: node.id, fieldPath: 'resourceReferences', message: `Select a ${selector.resourceType} resource.`, values: { resourceType: selector.resourceType } })
    }
    for (const slot of manifest.bindingSlots) {
      const references = node.data.resourceReferences.filter((reference) => reference.bindingRole === slot.name)
      if (slot.required && references.length === 0) issues.push({ code: 'RESOURCE_REQUIRED', nodeId: node.id, fieldPath: `resourceReferences.${slot.name}`, message: `Select the required ${slot.name} resource.`, values: { slot: slot.name } })
      if (!slot.multiple && references.length > 1) issues.push({ code: 'RESOURCE_MULTIPLE_NOT_ALLOWED', nodeId: node.id, fieldPath: `resourceReferences.${slot.name}`, message: `${slot.name} accepts one resource.`, values: { slot: slot.name } })
      if (slot.resourceType !== 'credential' && references.some((reference) => !reference.resourceVersionId)) issues.push({ code: 'RESOURCE_VERSION_REQUIRED', nodeId: node.id, fieldPath: `resourceReferences.${slot.name}`, message: `${slot.name} must use an exact resource version.`, values: { slot: slot.name } })
    }
  }
  return issues
}

function resourceSelectors(manifest: NodeManifest): Array<{ bindingRole?: string; resourceType: string; required?: boolean }> {
  const selectors = manifest.uiSchema.resourceSelectors
  return Array.isArray(selectors) ? selectors.filter((value): value is { bindingRole?: string; resourceType: string; required?: boolean } => Boolean(value && typeof value === 'object' && typeof (value as { resourceType?: unknown }).resourceType === 'string')) : []
}
