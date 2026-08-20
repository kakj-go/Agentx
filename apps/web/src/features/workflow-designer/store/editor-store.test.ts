import { beforeEach, describe, expect, it } from 'vitest'

import { useEditorStore } from './editor-store'

describe('editor store node creation', () => {
  beforeEach(() => {
    useEditorStore.setState({ nodes: [], edges: [], viewport: { x: 0, y: 0, zoom: 1 }, annotations: [], groups: [], settings: { executionOrder: 'deterministic', activationBudget: 10_000 }, dirty: false, past: [], future: [], selectedId: undefined })
  })

  it('selects a newly added action so it can be configured immediately', () => {
    useEditorStore.getState().addAction({ editorKind: 'action', nodeType: 'mcp_tool', typeVersion: 1, label: 'mcp tool', key: 'mcp_tool', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false })

    const state = useEditorStore.getState()
    expect(state.selectedId).toBe(state.nodes[0].id)
    expect(state.nodes[0].selected).toBe(true)
  })

  it('keeps stable upstream output selectors when a node key changes', () => {
    const source = useEditorStore.getState().addAction({ editorKind: 'action', nodeType: 'set', typeVersion: 1, label: 'Source', key: 'source', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false })
    const reference = { kind: 'reference', selector: { namespace: 'outputs', sourceNodeId: source, port: 'main', run: { kind: 'current' }, item: { kind: 'first' }, path: ['value'] }, missingPolicy: { kind: 'error' } }
    const target = useEditorStore.getState().addAction({ editorKind: 'action', nodeType: 'set', typeVersion: 1, label: 'Target', key: 'target', parameters: { value: reference }, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false })
    useEditorStore.getState().updateNode(source, { key: 'renamed' })
    const node = useEditorStore.getState().nodes.find((item) => item.id === target)
    expect(node?.data.editorKind === 'action' && node.data.parameters.value).toEqual(reference)
  })

  it('selects a newly added AI attachment so its resource can be chosen immediately', () => {
    useEditorStore.getState().addBinding({ editorKind: 'binding', bindingId: 'model-binding', bindingRole: 'ai_model', resourceType: 'model', operation: 'use', label: 'model' })

    const state = useEditorStore.getState()
    expect(state.selectedId).toBe('binding:model-binding')
    expect(state.nodes[0].selected).toBe(true)
  })

  it('aligns a multi-selection as one undoable command', () => {
    const data = { editorKind: 'action' as const, nodeType: 'set', typeVersion: 1, label: 'Set', key: 'set', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }
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
    const data = { editorKind: 'action' as const, nodeType: 'set', typeVersion: 1, label: 'Set', key: 'set', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }
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

  it('keeps viewport navigation outside document history', () => {
    useEditorStore.getState().beginEdit()
    useEditorStore.getState().setViewport({ x: -180, y: 90, zoom: 0.7 })
    expect(useEditorStore.getState().viewport).toEqual({ x: -180, y: 90, zoom: 0.7 })

    useEditorStore.getState().commitEdit()
    expect(useEditorStore.getState().past).toHaveLength(0)
    useEditorStore.getState().undo()
    expect(useEditorStore.getState().viewport).toEqual({ x: -180, y: 90, zoom: 0.7 })
  })

  it('adds missing boundary layouts on the first drag and restores them with undo', () => {
    useEditorStore.setState({ boundaryLayouts: [], past: [], future: [], dirty: false })

    useEditorStore.getState().beginEdit({ boundary: 'start' })
    useEditorStore.getState().updateBoundaryPosition('start', { x: 180, y: 120 })
    useEditorStore.getState().commitEdit()

    expect(useEditorStore.getState().boundaryLayouts).toEqual([{ boundary: 'start', x: 180, y: 120 }])
    expect(useEditorStore.getState().past).toHaveLength(1)
    expect(useEditorStore.getState().dirty).toBe(true)
    useEditorStore.getState().undo()
    expect(useEditorStore.getState().boundaryLayouts).toEqual([])
    useEditorStore.getState().redo()
    expect(useEditorStore.getState().boundaryLayouts).toEqual([{ boundary: 'start', x: 180, y: 120 }])
  })

  it('stores Start and End boundary positions independently', () => {
    useEditorStore.setState({ boundaryLayouts: [], past: [], future: [] })

    useEditorStore.getState().updateBoundaryPosition('start', { x: 80, y: 140 })
    useEditorStore.getState().updateBoundaryPosition('end', { x: 640, y: 260 })

    expect(useEditorStore.getState().boundaryLayouts).toEqual([
      { boundary: 'start', x: 80, y: 140 },
      { boundary: 'end', x: 640, y: 260 },
    ])
  })

  it('adds and connects a quick-add node as one undoable command', () => {
    const data = { editorKind: 'action' as const, nodeType: 'set', typeVersion: 1, label: 'Set', key: 'set', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }
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
    const data = { editorKind: 'action' as const, nodeType: 'set', typeVersion: 1, label: 'Set', key: 'set', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }
    useEditorStore.getState().addAction(data, { x: 80, y: 40 })
    useEditorStore.getState().addAction(data, { x: 260, y: 40 })
    useEditorStore.setState((state) => ({ nodes: state.nodes.map((node) => ({ ...node, selected: true })), selectedId: undefined, past: [], future: [] }))
    useEditorStore.getState().addGroup('Movable')
    const groupId = useEditorStore.getState().groups[0].id
    useEditorStore.setState({ past: [], future: [] })
    useEditorStore.getState().beginEdit()
    useEditorStore.getState().moveGroup(groupId, { x: 40, y: 25 })
    useEditorStore.getState().commitEdit()
    expect(useEditorStore.getState().nodes.map((node) => node.position)).toEqual([{ x: 120, y: 65 }, { x: 300, y: 65 }])
    useEditorStore.getState().undo()
    expect(useEditorStore.getState().nodes.map((node) => node.position)).toEqual([{ x: 80, y: 40 }, { x: 260, y: 40 }])

    useEditorStore.getState().addAnnotation({ x: 20, y: 20 })
    const annotationId = useEditorStore.getState().annotations[0].id
    useEditorStore.setState({ past: [], future: [] })
    useEditorStore.getState().beginEdit({ annotationIds: [annotationId] })
    useEditorStore.getState().updateAnnotationFrame(annotationId, { width: 360, height: 220 })
    useEditorStore.getState().commitEdit()
    useEditorStore.getState().undo()
    expect(useEditorStore.getState().annotations[0]).toMatchObject({ width: 240, height: 160 })
  })

  it('keeps error policy changes in the same undo transaction', () => {
    const data = { editorKind: 'action' as const, nodeType: 'set', typeVersion: 1, label: 'Set', key: 'set', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }
    const sourceId = useEditorStore.getState().addAction(data)
    const targetId = useEditorStore.getState().addAction(data)
    useEditorStore.setState({ past: [], future: [] })
    useEditorStore.getState().connect({ source: sourceId, sourceHandle: 'error', target: targetId, targetHandle: 'error' }, { edgeKind: 'execution', sourcePortKind: 'error' })
    const source = useEditorStore.getState().nodes.find((node) => node.id === sourceId)
    expect(source?.data.editorKind === 'action' && source.data.settings.onError).toBe('continue_error_output')
    useEditorStore.getState().undo()
    const restored = useEditorStore.getState().nodes.find((node) => node.id === sourceId)
    expect(restored?.data.editorKind === 'action' && restored.data.settings.onError).toBeUndefined()
  })

  it('deletes selected edges as one undoable command', () => {
    const data = { editorKind: 'action' as const, nodeType: 'set', typeVersion: 1, label: 'Set', key: 'set', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }
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

  it('keeps position frames out of GraphIndex revisions and reconnects as one command', () => {
    const data = { editorKind: 'action' as const, nodeType: 'set', typeVersion: 1, label: 'Set', key: 'set', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }
    const sourceId = useEditorStore.getState().addAction(data)
    const firstTarget = useEditorStore.getState().addAction(data)
    const secondTarget = useEditorStore.getState().addAction(data)
    useEditorStore.getState().connect({ source: sourceId, sourceHandle: 'main', target: firstTarget, targetHandle: 'main' }, { edgeKind: 'execution', sourcePortKind: 'main' })
    const edgeId = useEditorStore.getState().edges[0].id
    const revision = useEditorStore.getState().graphRevision

    useEditorStore.getState().onNodesChange([{ id: sourceId, type: 'position', position: { x: 420, y: 120 }, dragging: true }])
    expect(useEditorStore.getState().graphRevision).toBe(revision)
    useEditorStore.setState({ past: [], future: [] })
    useEditorStore.getState().reconnectEdge(edgeId, { source: sourceId, sourceHandle: 'main', target: secondTarget, targetHandle: 'main' })
    expect(useEditorStore.getState().edges[0].target).toBe(secondTarget)
    expect(useEditorStore.getState().past).toHaveLength(1)
    useEditorStore.getState().undo()
    expect(useEditorStore.getState().edges[0].target).toBe(firstTarget)
  })

  it('stores only the dragged node in a large-document history patch', () => {
    const data = { editorKind: 'action' as const, nodeType: 'set', typeVersion: 1, label: 'Set', key: 'set', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }
    const nodes: import('../model/types').StudioNode[] = Array.from({ length: 1_000 }, (_, index) => ({ id: `node-${index}`, type: 'manifest', position: { x: index, y: 0 }, data }))
    useEditorStore.setState({ nodes, past: [], future: [], graphRevision: 7 })
    useEditorStore.getState().beginEdit({ nodeIds: ['node-500'] })
    useEditorStore.getState().onNodesChange([{ id: 'node-500', type: 'position', position: { x: 900, y: 100 }, dragging: true }])
    useEditorStore.getState().commitEdit()

    expect(useEditorStore.getState().past[0].nodes).toHaveLength(1)
    expect(useEditorStore.getState().past[0].nodes[0].id).toBe('node-500')
    expect(useEditorStore.getState().graphRevision).toBe(7)
  })
})
