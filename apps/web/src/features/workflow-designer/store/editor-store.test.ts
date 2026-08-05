import { beforeEach, describe, expect, it } from 'vitest'

import { useEditorStore } from './editor-store'

describe('editor store node creation', () => {
  beforeEach(() => {
    useEditorStore.setState({ nodes: [], edges: [], viewport: { x: 0, y: 0, zoom: 1 }, annotations: [], groups: [], dirty: false, past: [], future: [], selectedId: undefined })
  })

  it('selects a newly added action so it can be configured immediately', () => {
    useEditorStore.getState().addAction({ editorKind: 'action', nodeType: 'mcp_tool', typeVersion: 1, label: 'mcp tool', parameters: {}, resourceReferences: [], settings: {}, disabled: false })

    const state = useEditorStore.getState()
    expect(state.selectedId).toBe(state.nodes[0].id)
    expect(state.nodes[0].selected).toBe(true)
  })

  it('selects a newly added AI attachment so its resource can be chosen immediately', () => {
    useEditorStore.getState().addBinding({ editorKind: 'binding', bindingId: 'model-binding', bindingRole: 'ai_model', resourceType: 'model', operation: 'use', label: 'model' })

    const state = useEditorStore.getState()
    expect(state.selectedId).toBe('binding:model-binding')
    expect(state.nodes[0].selected).toBe(true)
  })

  it('aligns a multi-selection as one undoable command', () => {
    const data = { editorKind: 'action' as const, nodeType: 'set', typeVersion: 1, label: 'Set', parameters: {}, resourceReferences: [], settings: {}, disabled: false }
    useEditorStore.getState().addAction(data, { x: 80, y: 40 })
    useEditorStore.getState().addAction(data, { x: 260, y: 160 })
    useEditorStore.setState((state) => ({ nodes: state.nodes.map((node) => ({ ...node, selected: true })), selectedId: undefined, past: [], future: [] }))

    useEditorStore.getState().alignSelected('left')
    expect(useEditorStore.getState().nodes.map((node) => node.position.x)).toEqual([80, 80])
    expect(useEditorStore.getState().past).toHaveLength(1)
    useEditorStore.getState().undo()
    expect(useEditorStore.getState().nodes.map((node) => node.position.x)).toEqual([80, 260])
  })
})
