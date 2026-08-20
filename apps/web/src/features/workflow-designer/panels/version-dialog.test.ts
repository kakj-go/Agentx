import { describe, expect, it } from 'vitest'

import type { EditorDocument, WorkflowDefinition } from '../model/types'
import { definitionDiff } from '../model/version-diff'

describe('VersionDialog diff', () => {
  it('separates runtime Definition changes from read-only Editor Document changes', () => {
    const before = definition('Original')
    const after = definition('Changed')
    const beforeEditor = editor(100)
    const afterEditor = editor(320)

    const diff = definitionDiff(before, after, beforeEditor, afterEditor)
    expect(diff.definitionChanges).toBe(1)
    expect(diff.editorChanges).toBe(1)
    expect(diff.nodeSummary).toBe('1 -> 1')
  })
})

function definition(name: string): WorkflowDefinition { return { schemaVersion: '5.0', start: { inputs: {}, contexts: {} }, nodes: [{ id: 'node-1', key: 'set', type: 'set', typeVersion: 1, name, disabled: false, parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {} }], connections: [], end: { outputs: {}, error: { strategy: 'fail_fast', collectWindowMs: 5000, outputs: {} } }, settings: { executionOrder: 'deterministic', activationBudget: 10_000 } } }
function editor(x: number): EditorDocument { return { nodeLayouts: [{ nodeId: 'node-1', x, y: 100 }], boundaryLayouts: [], bindingLayouts: [], edges: [], bindingEdges: [], annotations: [], groups: [], viewport: { x: 0, y: 0, zoom: 1 } } }
