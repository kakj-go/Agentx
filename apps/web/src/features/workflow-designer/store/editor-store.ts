import { addEdge, applyEdgeChanges, applyNodeChanges, type Connection, type EdgeChange, type NodeChange, type Viewport } from '@xyflow/react'
import { create } from 'zustand'

import type { ActionNodeData, BindingNodeData, StudioDocument, StudioEdge, StudioNode } from '../model/types'

type Snapshot = Pick<StudioDocument, 'nodes' | 'edges' | 'viewport'>
type EditorState = StudioDocument & {
  selectedId?: string
  dirty: boolean
  past: Snapshot[]
  future: Snapshot[]
  hydrate: (value: StudioDocument) => void
  onNodesChange: (changes: NodeChange<StudioNode>[]) => void
  onEdgesChange: (changes: EdgeChange<StudioEdge>[]) => void
  connect: (connection: Connection, data: StudioEdge['data']) => void
  addAction: (data: ActionNodeData, position?: { x: number; y: number }) => void
  addBinding: (data: BindingNodeData, position?: { x: number; y: number }) => void
  select: (id?: string) => void
  updateNode: (id: string, data: Partial<ActionNodeData> | Partial<BindingNodeData>) => void
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

const copy = (state: EditorState): Snapshot => ({ nodes: structuredClone(state.nodes), edges: structuredClone(state.edges), viewport: { ...state.viewport } })
const checkpoint = (state: EditorState) => ({ past: [...state.past.slice(-49), copy(state)], future: [], dirty: true })

export const useEditorStore = create<EditorState>((set) => ({
  nodes: [], edges: [], viewport: { x: 0, y: 0, zoom: 1 }, annotations: [], groups: [], dirty: false, past: [], future: [],
  hydrate: (value) => set({ ...value, dirty: false, selectedId: undefined, past: [], future: [] }),
  onNodesChange: (changes) => set((state) => ({ nodes: applyNodeChanges(changes, state.nodes), dirty: state.dirty || changes.some((change) => change.type !== 'select') })),
  onEdgesChange: (changes) => set((state) => ({ ...checkpoint(state), edges: applyEdgeChanges(changes, state.edges) })),
  connect: (connection, data) => set((state) => ({ ...checkpoint(state), edges: addEdge({ ...connection, id: crypto.randomUUID(), type: 'studio', data }, state.edges) })),
  addAction: (data, position) => set((state) => {
    const count = state.nodes.filter((node) => node.data.editorKind === 'action').length
    const nextPosition = position ?? { x: 60 + (count % 3) * 250, y: 60 + Math.floor(count / 3) * 170 }
    const id = crypto.randomUUID()
    return { ...checkpoint(state), selectedId: id, nodes: [...state.nodes.map((node) => ({ ...node, selected: false })), { id, type: 'manifest', position: nextPosition, data, selected: true }] }
  }),
  addBinding: (data, position) => set((state) => {
    const count = state.nodes.filter((node) => node.data.editorKind === 'binding').length
    const nextPosition = position ?? { x: 80 + count * 210, y: 410 }
    const id = `binding:${data.bindingId}`
    return { ...checkpoint(state), selectedId: id, nodes: [...state.nodes.map((node) => ({ ...node, selected: false })), { id, type: 'attachment', position: nextPosition, data, selected: true }] }
  }),
  select: (selectedId) => set({ selectedId }),
  updateNode: (id, data) => set((state) => ({ ...checkpoint(state), nodes: state.nodes.map((node) => node.id === id ? { ...node, data: { ...node.data, ...data } as typeof node.data } : node) })),
  removeSelected: () => set((state) => {
    const ids = new Set(state.nodes.filter((node) => node.selected).map((node) => node.id))
    if (state.selectedId) ids.add(state.selectedId)
    return ids.size ? ({ ...checkpoint(state), nodes: state.nodes.filter((node) => !ids.has(node.id)), edges: state.edges.filter((edge) => !ids.has(edge.source) && !ids.has(edge.target)), selectedId: undefined }) : state
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
  undo: () => set((state) => { const previous = state.past.at(-1); return previous ? { ...previous, annotations: state.annotations, groups: state.groups, selectedId: undefined, dirty: true, past: state.past.slice(0, -1), future: [copy(state), ...state.future] } : state }),
  redo: () => set((state) => { const next = state.future[0]; return next ? { ...next, annotations: state.annotations, groups: state.groups, selectedId: undefined, dirty: true, past: [...state.past, copy(state)], future: state.future.slice(1) } : state }),
}))
