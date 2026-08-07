import { beforeEach, describe, expect, it } from 'vitest'

import { useEditorStore } from './editor-store'

describe('editor store node creation', () => {
  beforeEach(() => {
    useEditorStore.setState({ nodes: [], edges: [], viewport: { x: 0, y: 0, zoom: 1 }, annotations: [], groups: [], settings: { executionOrder: 'deterministic', activationBudget: 10_000 }, dirty: false, past: [], future: [], selectedId: undefined })
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

  it('keeps annotation and group edits in the same undo history', () => {
    const data = { editorKind: 'action' as const, nodeType: 'set', typeVersion: 1, label: 'Set', parameters: {}, resourceReferences: [], settings: {}, disabled: false }
    useEditorStore.getState().addAction(data, { x: 80, y: 40 })
    useEditorStore.getState().addAction(data, { x: 260, y: 40 })
    useEditorStore.setState((state) => ({ nodes: state.nodes.map((node) => ({ ...node, selected: true })), selectedId: undefined, past: [], future: [] }))
    useEditorStore.getState().addAnnotation({ x: 20, y: 20 })
    const annotationId = useEditorStore.getState().annotations[0].id
    useEditorStore.getState().updateAnnotation(annotationId, { width: 300, height: 180 })
    useEditorStore.getState().addGroup('Main')
    const groupId = useEditorStore.getState().groups[0].id
    useEditorStore.getState().toggleGroup(groupId)
    expect(useEditorStore.getState().groups[0].collapsed).toBe(true)
    useEditorStore.getState().undo()
    expect(useEditorStore.getState().groups[0].collapsed).toBe(false)
    useEditorStore.getState().undo()
    expect(useEditorStore.getState().annotations[0]).toMatchObject({ width: 300, height: 180 })
  })

  it('undoes viewport navigation from the checkpoint created at move start', () => {
    useEditorStore.getState().beginEdit()
    useEditorStore.getState().setViewport({ x: -180, y: 90, zoom: 0.7 })
    expect(useEditorStore.getState().viewport).toEqual({ x: -180, y: 90, zoom: 0.7 })

    useEditorStore.getState().undo()
    expect(useEditorStore.getState().viewport).toEqual({ x: 0, y: 0, zoom: 1 })
    useEditorStore.getState().redo()
    expect(useEditorStore.getState().viewport).toEqual({ x: -180, y: 90, zoom: 0.7 })
  })

  it('adds and connects a quick-add node as one undoable command', () => {
    const data = { editorKind: 'action' as const, nodeType: 'set', typeVersion: 1, label: 'Set', parameters: {}, resourceReferences: [], settings: {}, disabled: false }
    const sourceId = useEditorStore.getState().addAction(data, { x: 20, y: 40 })
    useEditorStore.setState({ past: [], future: [] })

    useEditorStore.getState().addConnectedAction(data, { nodeId: sourceId, handleId: 'main', targetHandle: 'main' }, { x: 260, y: 40 })
    expect(useEditorStore.getState().nodes).toHaveLength(2)
    expect(useEditorStore.getState().edges[0]).toMatchObject({ source: sourceId, sourceHandle: 'main', targetHandle: 'main' })
    expect(useEditorStore.getState().past).toHaveLength(1)

    useEditorStore.getState().undo()
    expect(useEditorStore.getState().nodes).toHaveLength(1)
    expect(useEditorStore.getState().edges).toHaveLength(0)
  })

  it('undoes a Group move and Sticky Note resize from their gesture checkpoints', () => {
    const data = { editorKind: 'action' as const, nodeType: 'set', typeVersion: 1, label: 'Set', parameters: {}, resourceReferences: [], settings: {}, disabled: false }
    useEditorStore.getState().addAction(data, { x: 80, y: 40 })
    useEditorStore.getState().addAction(data, { x: 260, y: 40 })
    useEditorStore.setState((state) => ({ nodes: state.nodes.map((node) => ({ ...node, selected: true })), selectedId: undefined, past: [], future: [] }))
    useEditorStore.getState().addGroup('Movable')
    const groupId = useEditorStore.getState().groups[0].id
    useEditorStore.setState({ past: [], future: [] })
    useEditorStore.getState().beginEdit()
    useEditorStore.getState().moveGroup(groupId, { x: 40, y: 25 })
    expect(useEditorStore.getState().nodes.map((node) => node.position)).toEqual([{ x: 120, y: 65 }, { x: 300, y: 65 }])
    useEditorStore.getState().undo()
    expect(useEditorStore.getState().nodes.map((node) => node.position)).toEqual([{ x: 80, y: 40 }, { x: 260, y: 40 }])

    useEditorStore.getState().addAnnotation({ x: 20, y: 20 })
    const annotationId = useEditorStore.getState().annotations[0].id
    useEditorStore.setState({ past: [], future: [] })
    useEditorStore.getState().beginEdit()
    useEditorStore.getState().updateAnnotationFrame(annotationId, { width: 360, height: 220 })
    useEditorStore.getState().undo()
    expect(useEditorStore.getState().annotations[0]).toMatchObject({ width: 240, height: 160 })
  })

  it('keeps primary output and error policy changes in the same undo transaction', () => {
    const data = { editorKind: 'action' as const, nodeType: 'set', typeVersion: 1, label: 'Set', parameters: {}, resourceReferences: [], settings: {}, disabled: false }
    const sourceId = useEditorStore.getState().addAction(data)
    const targetId = useEditorStore.getState().addAction(data)
    useEditorStore.getState().setPrimaryOutput(sourceId)
    useEditorStore.setState({ past: [], future: [] })

    useEditorStore.getState().connect({ source: sourceId, sourceHandle: 'error', target: targetId, targetHandle: 'error' }, { edgeKind: 'execution', sourcePortKind: 'error' })
    const source = useEditorStore.getState().nodes.find((node) => node.id === sourceId)
    expect(source?.data.editorKind === 'action' && source.data.settings.onError).toBe('continue_error_output')
    expect(useEditorStore.getState().settings.primaryOutputNodeId).toBe(sourceId)
    useEditorStore.getState().undo()
    const restored = useEditorStore.getState().nodes.find((node) => node.id === sourceId)
    expect(restored?.data.editorKind === 'action' && restored.data.settings.onError).toBeUndefined()

    useEditorStore.getState().connect({ source: sourceId, sourceHandle: 'main', target: targetId, targetHandle: 'main' }, { edgeKind: 'execution', sourcePortKind: 'main' })
    expect(useEditorStore.getState().settings.primaryOutputNodeId).toBeUndefined()
    useEditorStore.getState().undo()
    expect(useEditorStore.getState().settings.primaryOutputNodeId).toBe(sourceId)
  })

  it('deletes selected edges as one undoable command', () => {
    const data = { editorKind: 'action' as const, nodeType: 'set', typeVersion: 1, label: 'Set', parameters: {}, resourceReferences: [], settings: {}, disabled: false }
    const sourceId = useEditorStore.getState().addAction(data)
    const targetId = useEditorStore.getState().addAction(data)
    useEditorStore.getState().connect({ source: sourceId, sourceHandle: 'main', target: targetId, targetHandle: 'main' }, { edgeKind: 'execution', sourcePortKind: 'main' })
    useEditorStore.setState((state) => ({ edges: state.edges.map((edge) => ({ ...edge, selected: true })), past: [], future: [] }))

    useEditorStore.getState().removeSelectedEdges()
    expect(useEditorStore.getState().edges).toHaveLength(0)
    expect(useEditorStore.getState().past).toHaveLength(1)
    useEditorStore.getState().undo()
    expect(useEditorStore.getState().edges).toHaveLength(1)
  })
})
