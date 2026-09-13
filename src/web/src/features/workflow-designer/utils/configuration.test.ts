import { describe, expect, it } from 'vitest'

import type { NodeManifest, StudioDocument } from '../model/types'
import { configurationIssues } from './configuration'

function fixture(resourceType: 'credential' | 'model', resourceVersionId: string | null) {
  const nodeType = resourceType === 'credential' ? 'declarative_http' : 'agent'
  const document = {
    start: { inputs: { type: 'object', properties: {} }, contexts: {} },
    nodes: [{
      id: 'node', type: 'manifest', position: { x: 0, y: 0 },
      data: {
        editorKind: 'action', nodeType, typeVersion: 1, label: 'Node', key: 'node',
        parameters: resourceType === 'credential' ? { url: 'https://example.test' } : { sessionPolicy: { mode: 'invocation' } },
        contextWrites: [], settings: {}, disabled: false,
        resourceReferences: [{ bindingRole: resourceType, resourceType, resourceId: 'resource', resourceVersionId, operation: 'use' }],
      },
    }],
    edges: [], end: { outputs: {}, error: { outputs: {} } }, viewport: { x: 0, y: 0, zoom: 1 }, boundaryLayouts: {}, annotations: [], groups: [], settings: {},
  } as unknown as StudioDocument
  const manifest = {
    nodeType, version: 1, parameterSchema: { type: 'object', properties: {} },
    uiSchema: { fields: {}, resourceSelectors: [] },
    bindingSlots: [{ name: resourceType, resourceType, placement: 'inspector', required: true, multiple: false }],
  } as unknown as NodeManifest
  return configurationIssues(document, new Map([[`${nodeType}@1`, manifest]]))
}

describe('Studio resource version validation', () => {
  it('keeps Vault credentials unversioned while requiring immutable Model versions', () => {
    expect(fixture('credential', null).map((issue) => issue.code)).not.toContain('RESOURCE_VERSION_REQUIRED')
    expect(fixture('model', null).map((issue) => issue.code)).toContain('RESOURCE_VERSION_REQUIRED')
  })
})
