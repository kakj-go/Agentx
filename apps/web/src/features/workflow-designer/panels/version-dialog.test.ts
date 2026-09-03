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

function definition(name: string): WorkflowDefinition { return { schemaVersion: '8.0', start: { inputs: {}, contexts: {} }, nodes: [{ id: 'node-1', key: 'set', type: 'set', typeVersion: 1, name, disabled: false, protected: false, parameters: {}, contextWrites: [], resourceReferences: [], settings: {} }], connections: [], end: { completion: "first_return", outputs: {}, error: { outputs: { } } }, settings: { executionOrder: 'deterministic', activationBudget: 10_000 } } }
function editor(x: number): EditorDocument { return { nodeLayouts: [{ nodeId: 'node-1', x, y: 100 }], boundaryLayouts: [], edges: [], annotations: [], groups: [], viewport: { x: 0, y: 0, zoom: 1 } } }
