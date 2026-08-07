import { addEdge, applyEdgeChanges, applyNodeChanges, type Connection, type EdgeChange, type NodeChange, type Viewport } from '@xyflow/react'
import { create } from 'zustand'

import type { ActionNodeData, BindingNodeData, StudioDocument, StudioEdge, StudioNode } from '../model/types'

type EditorState = StudioDocument & {
  selectedId?: string
  dirty: boolean
  past: Snapshot[]
  future: Snapshot[]
  hydrate: (value: StudioDocument) => void
  onNodesChange: (changes: NodeChange<StudioNode>[]) => void
  onEdgesChange: (changes: EdgeChange<StudioEdge>[]) => void
  removeSelectedEdges: () => void
  connect: (connection: Connection, data: StudioEdge['data']) => void
  addAction: (data: ActionNodeData, position?: { x: number; y: number }) => string
  addConnectedAction: (data: ActionNodeData, source: { nodeId: string; handleId: string; targetHandle: string }, position: { x: number; y: number }) => string
  addBinding: (data: BindingNodeData, position?: { x: number; y: number }) => string
  addAnnotation: (position?: { x: number; y: number }, text?: string) => void
  updateAnnotation: (id: string, patch: Partial<StudioDocument['annotations'][number]>) => void
  updateAnnotationFrame: (id: string, patch: Partial<Pick<StudioDocument['annotations'][number], 'x' | 'y' | 'width' | 'height'>>) => void
  removeAnnotation: (id: string) => void
  addGroup: (label?: string) => void
  toggleGroup: (id: string) => void
  removeGroup: (id: string) => void
  moveGroup: (id: string, delta: { x: number; y: number }) => void
  select: (id?: string) => void
  updateNode: (id: string, data: Partial<ActionNodeData> | Partial<BindingNodeData>) => void
  setPrimaryOutput: (id?: string) => void
  removeSelected: () => void
  setViewport: (viewport: Viewport) => void
  replaceNodes: (nodes: StudioNode[]) => void
  alignSelected: (direction: 'left' | 'top') => void
  beginEdit: () => void
  paste: (nodes: StudioNode[], edges: StudioEdge[]) => void
  markSaved: () => void
  undo: () => void
  redo: () => void
}

type Snapshot = Pick<StudioDocument, 'nodes' | 'edges' | 'viewport' | 'annotations' | 'groups' | 'settings'>

const copy = (state: EditorState): Snapshot => ({
  nodes: structuredClone(state.nodes),
  edges: structuredClone(state.edges),
  viewport: { ...state.viewport },
  annotations: structuredClone(state.annotations),
  groups: structuredClone(state.groups),
  settings: structuredClone(state.settings),
})
const checkpoint = (state: EditorState) => ({ past: [...state.past.slice(-49), copy(state)], future: [], dirty: true })

export const useEditorStore = create<EditorState>((set) => ({
  nodes: [], edges: [], viewport: { x: 0, y: 0, zoom: 1 }, annotations: [], groups: [], settings: { executionOrder: 'deterministic', activationBudget: 10_000 }, dirty: false, past: [], future: [],
  hydrate: (value) => set({ ...value, dirty: false, selectedId: undefined, past: [], future: [] }),
  onNodesChange: (changes) => set((state) => ({ nodes: applyNodeChanges(changes, state.nodes), dirty: state.dirty || changes.some((change) => change.type !== 'select') })),
  onEdgesChange: (changes) => set((state) => ({ ...checkpoint(state), edges: applyEdgeChanges(changes, state.edges) })),
  removeSelectedEdges: () => set((state) => {
    if (!state.edges.some((edge) => edge.selected)) return state
    return { ...checkpoint(state), edges: state.edges.filter((edge) => !edge.selected) }
  }),
  connect: (connection, data) => set((state) => {
    const sourceId = connection.source ?? ''
    const clearPrimary = data?.sourcePortKind === 'main' && state.settings.primaryOutputNodeId === sourceId
    const nodes = data?.sourcePortKind === 'error'
      ? state.nodes.map((node) => node.id === sourceId && node.data.editorKind === 'action' ? { ...node, data: { ...node.data, settings: { ...node.data.settings, onError: 'continue_error_output' } } } : node)
      : state.nodes
    return {
      ...checkpoint(state),
      nodes,
      settings: clearPrimary ? { ...state.settings, primaryOutputNodeId: undefined } : state.settings,
      edges: addEdge({ ...connection, id: crypto.randomUUID(), type: 'studio', data }, state.edges),
    }
  }),
  addAction: (data, position) => {
    const id = crypto.randomUUID()
    set((state) => {
      const count = state.nodes.filter((node) => node.data.editorKind === 'action').length
      const nextPosition = position ?? { x: 60 + (count % 3) * 250, y: 60 + Math.floor(count / 3) * 170 }
      return { ...checkpoint(state), selectedId: id, nodes: [...state.nodes.map((node) => ({ ...node, selected: false })), { id, type: 'manifest', position: nextPosition, data, selected: true }] }
    })
    return id
  },
  addConnectedAction: (data, source, position) => {
    const id = crypto.randomUUID()
    set((state) => {
      const order = state.edges.filter((edge) => edge.source === source.nodeId && edge.data?.edgeKind === 'execution').length
      const sourcePortKind = source.handleId === 'error' ? 'error' : 'main'
      const edge: StudioEdge = { id: crypto.randomUUID(), source: source.nodeId, sourceHandle: source.handleId, target: id, targetHandle: source.targetHandle, type: 'studio', data: { edgeKind: 'execution', order, sourcePortKind } }
      const sourceNodes = sourcePortKind === 'error' ? state.nodes.map((node) => node.id === source.nodeId && node.data.editorKind === 'action' ? { ...node, data: { ...node.data, settings: { ...node.data.settings, onError: 'continue_error_output' } } } : node) : state.nodes
      return {
        ...checkpoint(state),
        selectedId: id,
        settings: sourcePortKind === 'main' && state.settings.primaryOutputNodeId === source.nodeId ? { ...state.settings, primaryOutputNodeId: undefined } : state.settings,
        nodes: [...sourceNodes.map((node) => ({ ...node, selected: false })), { id, type: 'manifest', position, data, selected: true }],
        edges: [...state.edges, edge],
      }
    })
    return id
  },
  addBinding: (data, position) => {
    const id = `binding:${data.bindingId}`
    set((state) => {
      const count = state.nodes.filter((node) => node.data.editorKind === 'binding').length
      const nextPosition = position ?? { x: 80 + count * 210, y: 410 }
      return { ...checkpoint(state), selectedId: id, nodes: [...state.nodes.map((node) => ({ ...node, selected: false })), { id, type: 'attachment', position: nextPosition, data, selected: true }] }
    })
    return id
  },
  addAnnotation: (position, text = 'New note') => set((state) => {
    const id = crypto.randomUUID()
    const next = position ?? { x: 120, y: 120 }
    return { ...checkpoint(state), annotations: [...state.annotations, { id, text, x: next.x, y: next.y, width: 240, height: 160 }] }
  }),
  updateAnnotation: (id, patch) => set((state) => ({ ...checkpoint(state), annotations: state.annotations.map((annotation) => annotation.id === id ? { ...annotation, ...patch } : annotation) })),
  updateAnnotationFrame: (id, patch) => set((state) => ({ annotations: state.annotations.map((annotation) => annotation.id === id ? { ...annotation, ...patch } : annotation), dirty: true })),
  removeAnnotation: (id) => set((state) => ({ ...checkpoint(state), annotations: state.annotations.filter((annotation) => annotation.id !== id) })),
  addGroup: (label = 'Group') => set((state) => {
    const grouped = new Set(state.groups.flatMap((group) => group.nodeIds))
    const nodeIds = state.nodes.filter((node) => (node.selected || node.id === state.selectedId) && !grouped.has(node.id)).map((node) => node.id)
    if (nodeIds.length < 2) return state
    return { ...checkpoint(state), groups: [...state.groups, { id: crypto.randomUUID(), label, nodeIds, collapsed: false }] }
  }),
  toggleGroup: (id) => set((state) => ({ ...checkpoint(state), groups: state.groups.map((group) => group.id === id ? { ...group, collapsed: !group.collapsed } : group) })),
  removeGroup: (id) => set((state) => ({ ...checkpoint(state), groups: state.groups.filter((group) => group.id !== id) })),
  moveGroup: (id, delta) => set((state) => {
    const memberIds = new Set(state.groups.find((group) => group.id === id)?.nodeIds ?? [])
    if (!memberIds.size || (!delta.x && !delta.y)) return state
    return { nodes: state.nodes.map((node) => memberIds.has(node.id) ? { ...node, position: { x: node.position.x + delta.x, y: node.position.y + delta.y } } : node), dirty: true }
  }),
  select: (selectedId) => set({ selectedId }),
  updateNode: (id, data) => set((state) => ({
    ...checkpoint(state),
    settings: 'disabled' in data && data.disabled === true && state.settings.primaryOutputNodeId === id ? { ...state.settings, primaryOutputNodeId: undefined } : state.settings,
    nodes: state.nodes.map((node) => node.id === id ? { ...node, data: { ...node.data, ...data } as typeof node.data } : node),
  })),
  setPrimaryOutput: (id) => set((state) => ({ ...checkpoint(state), settings: { ...state.settings, primaryOutputNodeId: id } })),
  removeSelected: () => set((state) => {
    if (state.selectedId?.startsWith('annotation:')) {
      const annotationId = state.selectedId.slice('annotation:'.length)
      return { ...checkpoint(state), annotations: state.annotations.filter((annotation) => annotation.id !== annotationId), selectedId: undefined }
    }
    if (state.selectedId?.startsWith('group:')) {
      const groupId = state.selectedId.slice('group:'.length)
      return { ...checkpoint(state), groups: state.groups.filter((group) => group.id !== groupId), selectedId: undefined }
    }
    const ids = new Set(state.nodes.filter((node) => node.selected).map((node) => node.id))
    if (state.selectedId) ids.add(state.selectedId)
    return ids.size ? ({ ...checkpoint(state), settings: state.settings.primaryOutputNodeId && ids.has(state.settings.primaryOutputNodeId) ? { ...state.settings, primaryOutputNodeId: undefined } : state.settings, nodes: state.nodes.filter((node) => !ids.has(node.id)), edges: state.edges.filter((edge) => !ids.has(edge.source) && !ids.has(edge.target)), groups: state.groups.map((group) => ({ ...group, nodeIds: group.nodeIds.filter((id) => !ids.has(id)) })).filter((group) => group.nodeIds.length), selectedId: undefined }) : state
  }),
  setViewport: (viewport) => set({ viewport, dirty: true }),
  replaceNodes: (nodes) => set((state) => ({ ...checkpoint(state), nodes })),
  alignSelected: (direction) => set((state) => {
    const selected = state.nodes.filter((node) => node.selected || node.id === state.selectedId)
    if (selected.length < 2) return state
    const value = Math.min(...selected.map((node) => direction === 'left' ? node.position.x : node.position.y))
    const selectedIds = new Set(selected.map((node) => node.id))
    return { ...checkpoint(state), nodes: state.nodes.map((node) => selectedIds.has(node.id) ? { ...node, position: direction === 'left' ? { ...node.position, x: value } : { ...node.position, y: value } } : node) }
  }),
  beginEdit: () => set((state) => checkpoint(state)),
  paste: (nodes, edges) => set((state) => ({
    ...checkpoint(state),
    nodes: [...state.nodes.map((node) => ({ ...node, selected: false })), ...nodes],
    edges: [...state.edges.map((edge) => ({ ...edge, selected: false })), ...edges],
    selectedId: nodes.length === 1 ? nodes[0].id : undefined,
  })),
  markSaved: () => set({ dirty: false }),
  undo: () => set((state) => { const previous = state.past.at(-1); return previous ? { ...previous, selectedId: undefined, dirty: true, past: state.past.slice(0, -1), future: [copy(state), ...state.future] } : state }),
  redo: () => set((state) => { const next = state.future[0]; return next ? { ...next, selectedId: undefined, dirty: true, past: [...state.past, copy(state)], future: state.future.slice(1) } : state }),
}))
