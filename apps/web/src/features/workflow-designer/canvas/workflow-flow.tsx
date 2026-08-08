import { Background, BackgroundVariant, Controls, MarkerType, MiniMap, ReactFlow, type Connection, type NodeChange, type ReactFlowInstance } from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import { Plus, Search } from 'lucide-react'
import { forwardRef, useCallback, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState, type DragEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { useShallow } from 'zustand/react/shallow'

import type { CanvasNode, ConnectionInteractionState, EditorDocument, NodeManifest, ResourceType, StudioEdge, StudioNode } from '../model/types'
import { StudioEdgeComponent } from '../edges/studio-edge'
import { AnnotationNode, GroupNode } from './editor-overlays'
import { AttachmentNode, ManifestNode } from '../nodes/manifest-node'
import { canvasNodeMetrics, canvasNodeRole } from '../nodes/node-appearance'
import { canvasZoomTier, syncCanvasRenderState } from '../store/canvas-render-store'
import { useEditorStore } from '../store/editor-store'
import { inspectConnection } from '../utils/connections'
import { createGraphIndex, IncrementalGraphIndex, occupiedHandlesByNodeId, portKey } from '../utils/graph-index'
import { proxyEdges } from '../utils/group-edges'

export const LARGE_GRAPH_RENDER_THRESHOLD = 150
export const LARGE_GRAPH_MINIMAP_THRESHOLD = 300

type FlowProps = {
  manifests: Map<string, NodeManifest>
  runtimeStatuses: Map<string, string>
  onDropAction: (manifest: NodeManifest, position: { x: number; y: number }) => void
  onDropBinding: (resourceType: ResourceType, role: string, position: { x: number; y: number }) => void
  onNodeOpen?: (nodeId: string) => void
  onPaneClear?: () => void
  onSearchNodes?: (source?: { nodeId: string; handleId: string; mode: 'output' | 'binding' }) => void
  onAddTrigger?: () => void
}

export type WorkflowFlowHandle = {
  getViewportCenter: () => { x: number; y: number } | undefined
  getViewportBounds: () => { x: number; y: number; width: number; height: number } | undefined
}

const NODE_TYPES = { manifest: ManifestNode, attachment: AttachmentNode, annotation: AnnotationNode, group: GroupNode }
const EDGE_TYPES = { studio: StudioEdgeComponent }

export const WorkflowFlow = forwardRef<WorkflowFlowHandle, FlowProps>(function WorkflowFlow({ manifests, runtimeStatuses, onDropAction, onDropBinding, onNodeOpen, onPaneClear, onSearchNodes, onAddTrigger }, ref) {
  const { t } = useTranslation()
  const instance = useRef<ReactFlowInstance<CanvasNode, StudioEdge>>()
  const [connectionState, setConnectionState] = useState<ConnectionInteractionState>({ status: 'idle' })
  const editor = useEditorStore(useShallow((state) => ({
    nodes: state.nodes, edges: state.edges, annotations: state.annotations, groups: state.groups, viewport: state.viewport,
    graphRevision: state.graphRevision, primaryOutputNodeId: state.settings.primaryOutputNodeId,
    onEdgesChange: state.onEdgesChange, connect: state.connect, select: state.select, setViewport: state.setViewport,
    reconnectEdge: state.reconnectEdge, edgeReconnectRequest: state.edgeReconnectRequest, clearEdgeReconnectRequest: state.clearEdgeReconnectRequest,
    beginEdit: state.beginEdit, commitEdit: state.commitEdit, onNodesChange: state.onNodesChange,
    updateAnnotationFrame: state.updateAnnotationFrame, removeAnnotation: state.removeAnnotation, updateAnnotation: state.updateAnnotation,
    moveGroup: state.moveGroup, removeGroup: state.removeGroup, toggleGroup: state.toggleGroup,
  })))
  const { nodes, edges, annotations, groups, viewport, onEdgesChange, connect, select, setViewport, commitEdit } = editor
  const annotationActions = useMemo(() => ({
    updateAnnotation: editor.updateAnnotation,
    removeAnnotation: editor.removeAnnotation,
    beginEdit: editor.beginEdit,
    commitEdit: editor.commitEdit,
    updateAnnotationFrame: editor.updateAnnotationFrame,
  }), [editor.beginEdit, editor.commitEdit, editor.removeAnnotation, editor.updateAnnotation, editor.updateAnnotationFrame])
  const groupActions = useMemo(() => ({ toggleGroup: editor.toggleGroup, removeGroup: editor.removeGroup }), [editor.removeGroup, editor.toggleGroup])
  const groupViewIndex = useRef(new GroupViewIndex())
  const groupViews = useMemo(() => groupViewIndex.current.sync(nodes, groups, manifests), [groups, manifests, nodes])
  const collapsedByMember = useMemo(() => {
    const result = new Map<string, string>()
    for (const group of groups) if (group.collapsed) for (const nodeId of group.nodeIds) result.set(nodeId, `group:${group.id}`)
    return result
  }, [groups])
  const renderedNodeCache = useRef(new Map<string, { source: StudioNode; manifest?: NodeManifest; value: StudioNode }>())
  const flowNodes = useMemo<CanvasNode[]>(() => [
    ...materializeNodes(nodes.filter((node) => !collapsedByMember.has(node.id)), manifests, renderedNodeCache.current),
    ...annotations.map((annotation) => annotationNode(annotation, annotationActions)),
    ...groupViews.map((view) => groupNode(view, groupActions)),
  ], [annotationActions, annotations, collapsedByMember, groupActions, groupViews, manifests, nodes])
  const renderedEdgeCache = useRef(new Map<string, { source: StudioEdge; sourceStatus?: string; targetStatus?: string; value: StudioEdge }>())
  const flowEdges = useMemo(() => materializeRuntimeEdges(proxyEdges(edges, collapsedByMember), runtimeStatuses, renderedEdgeCache.current), [collapsedByMember, edges, runtimeStatuses])
  const groupViewMap = useMemo(() => new Map(groupViews.map((view) => [view.id, view])), [groupViews])
  const graphIndexController = useRef(new IncrementalGraphIndex())
  const graphIndex = useMemo(() => graphIndexController.current.sync(useEditorStore.getState().nodes, useEditorStore.getState().edges, manifests), [editor.graphRevision, manifests])
  const occupiedHandlesCache = useRef<{ graphRevision: number; value: Map<string, string> }>()
  if (occupiedHandlesCache.current?.graphRevision !== editor.graphRevision) occupiedHandlesCache.current = { graphRevision: editor.graphRevision, value: occupiedHandlesByNodeId(graphIndex) }
  const occupiedHandles = occupiedHandlesCache.current.value
  const onSourceHover = useCallback((nodeId: string, handleId: string, active: boolean) => setConnectionState((current) => {
    if (active && current.status === 'idle') return { status: 'source-hover', nodeId, handleId }
    if (!active && current.status === 'source-hover' && current.nodeId === nodeId && current.handleId === handleId) return { status: 'idle' }
    return current
  }), [])
  useLayoutEffect(() => syncCanvasRenderState({
    manifests, runtimeStatuses, bindingSummaries: graphIndex.bindingSummaryByNodeId, occupiedHandlesByNodeId: occupiedHandles,
    primaryOutputNodeId: editor.primaryOutputNodeId ?? undefined, zoomTier: canvasZoomTier(viewport.zoom),
    onQuickAdd: onSearchNodes ? (nodeId, handleId, mode) => onSearchNodes({ nodeId, handleId, mode }) : undefined,
    onSourceHover,
  }), [editor.graphRevision, editor.primaryOutputNodeId, graphIndex, manifests, occupiedHandles, onSearchNodes, onSourceHover, runtimeStatuses, viewport.zoom])
  useImperativeHandle(ref, () => ({
    getViewportCenter: () => {
      const flow = instance.current
      const element = document.querySelector<HTMLElement>('[data-testid="workflow-canvas"]')
      if (!flow || !element) return undefined
      const bounds = element.getBoundingClientRect()
      return flow.screenToFlowPosition({ x: bounds.left + bounds.width / 2, y: bounds.top + bounds.height / 2 })
    },
    getViewportBounds: () => {
      const flow = instance.current
      const element = document.querySelector<HTMLElement>('[data-testid="workflow-canvas"]')
      if (!flow || !element) return undefined
      const bounds = element.getBoundingClientRect()
      const topLeft = flow.screenToFlowPosition({ x: bounds.left, y: bounds.top })
      const bottomRight = flow.screenToFlowPosition({ x: bounds.right, y: bounds.bottom })
      return { x: topLeft.x, y: topLeft.y, width: bottomRight.x - topLeft.x, height: bottomRight.y - topLeft.y }
    },
  }), [])
  const valid = useCallback((connection: Connection | StudioEdge) => {
    const validation = inspectConnection(connection, graphIndex)
    setConnectionState(validation.status === 'invalid'
      ? { status: 'incompatible', reason: validation.reason }
      : validation.status === 'occupied' ? { status: 'occupied' } : { status: 'compatible' })
    return validation.status !== 'invalid'
  }, [graphIndex])
  const onConnect = useCallback((connection: Connection) => {
    const validation = inspectConnection(connection, graphIndex)
    if (validation.status === 'invalid') return
    const source = graphIndex.nodeById.get(connection.source ?? '')
    const binding = source?.data.editorKind === 'binding'
    const sourceManifest = source?.data.editorKind === 'action' ? manifests.get(`${source.data.nodeType}@${source.data.typeVersion}`) : undefined
    const sourcePortKind = sourceManifest?.outputPorts.find((port) => port.name === connection.sourceHandle)?.kind
    const order = graphIndex.edgesBySourcePort.get(portKey(connection.source ?? '', connection.sourceHandle))?.filter((edge) => edge.data?.edgeKind === 'execution').length ?? 0
    connect(connection, { edgeKind: binding ? 'binding' : 'execution', order: binding ? undefined : order, targetSlot: binding ? connection.targetHandle?.replace(/^binding:/, '') : undefined, sourcePortKind }, validation.replaceEdge?.id)
    setConnectionState({ status: 'committed' })
  }, [connect, graphIndex, manifests])
  const onNodesChange = useCallback((changes: NodeChange<CanvasNode>[]) => {
    const canonical: NodeChange<CanvasNode>[] = []
    for (const change of changes) {
      if (!('id' in change)) continue
      if (change.id.startsWith('annotation:')) {
        const annotationId = change.id.slice('annotation:'.length)
        if (change.type === 'position' && change.position) editor.updateAnnotationFrame(annotationId, change.position)
        if (change.type === 'remove') editor.removeAnnotation(annotationId)
        continue
      }
      const group = groupViewMap.get(change.id)
      if (group) {
        if (change.type === 'position' && change.position) editor.moveGroup(group.group.id, { x: change.position.x - group.position.x, y: change.position.y - group.position.y })
        if (change.type === 'remove') editor.removeGroup(group.group.id)
        continue
      }
      canonical.push(change)
    }
    if (canonical.length) editor.onNodesChange(canonical as NodeChange<StudioNode>[])
  }, [editor, groupViewMap])
  const onReconnect = useCallback((edge: StudioEdge, connection: Connection) => {
    const reconnectIndex = createGraphIndex(nodes, edges.filter((candidate) => candidate.id !== edge.id), manifests)
    if (inspectConnection(connection, reconnectIndex).status === 'invalid') return
    editor.reconnectEdge(edge.id, connection)
    editor.clearEdgeReconnectRequest()
  }, [edges, editor, manifests, nodes])
  const onNodeDragStart = useCallback((_: unknown, node: CanvasNode) => {
    if (node.id.startsWith('annotation:')) return editor.beginEdit({ annotationIds: [node.id.slice('annotation:'.length)] })
    const group = groupViewMap.get(node.id)
    if (group) return editor.beginEdit({ nodeIds: group.group.nodeIds })
    const nodeIds = nodes.filter((candidate) => candidate.selected || candidate.id === node.id).map((candidate) => candidate.id)
    editor.beginEdit({ nodeIds })
  }, [editor, groupViewMap, nodes])
  const onDrop = (event: DragEvent) => {
    event.preventDefault()
    const raw = event.dataTransfer.getData('application/agentx-studio')
    if (!raw || !instance.current) return
    const value = JSON.parse(raw) as { kind: string; nodeType?: string; version?: number; resourceType?: ResourceType; role?: string }
    const position = instance.current.screenToFlowPosition({ x: event.clientX, y: event.clientY })
    if (value.kind === 'action') { const manifest = manifests.get(`${value.nodeType}@${value.version}`); if (manifest) onDropAction(manifest, position) }
    if (value.kind === 'binding' && value.resourceType && value.role) onDropBinding(value.resourceType, value.role, position)
  }
  const actionCount = nodes.filter((node) => node.data.editorKind === 'action').length
  return <div className="relative min-w-0 flex-1 bg-canvas" data-connection-state={connectionState.status} data-testid="workflow-canvas" onDragOver={(event) => { event.preventDefault(); event.dataTransfer.dropEffect = 'copy' }} onDrop={onDrop}>
    <div className="pointer-events-none absolute right-3 top-3 z-10 flex items-center gap-3 rounded-md border border-border bg-surface/90 px-2.5 py-1.5 text-[9px] text-muted-foreground shadow-sm backdrop-blur"><LegendDot className="bg-primary" label={t('studio.ports.flow')} /><LegendDot className="bg-danger" label={t('studio.ports.error')} /><LegendDot className="bg-warning" label={t('studio.ports.resource')} /></div>
    {actionCount === 0 && <div className="pointer-events-none absolute inset-0 z-10 grid place-items-center"><div className="pointer-events-auto flex items-center gap-2 rounded-md border border-border bg-surface/95 p-2 shadow-md"><button className="flex h-9 items-center gap-2 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground" onClick={onAddTrigger} type="button"><Plus className="size-4" />{t('studio.empty.addTrigger')}</button><button className="flex h-9 items-center gap-2 rounded-md px-3 text-xs text-muted-foreground hover:bg-muted hover:text-foreground" onClick={() => onSearchNodes?.()} type="button"><Search className="size-4" />{t('studio.empty.search')}</button></div></div>}
    <ReactFlow<CanvasNode, StudioEdge> connectionRadius={60} defaultViewport={viewport} edges={flowEdges} edgeTypes={EDGE_TYPES} edgesReconnectable fitViewOptions={{ maxZoom: 1, padding: 0.2 }} nodes={flowNodes} nodeTypes={NODE_TYPES} onConnect={onConnect} onConnectEnd={() => { setConnectionState({ status: 'cancelled' }); window.requestAnimationFrame(() => setConnectionState({ status: 'idle' })) }} onConnectStart={(_, params) => setConnectionState({ status: 'connecting', nodeId: params.nodeId ?? '', handleId: params.handleId ?? undefined })} onEdgesChange={onEdgesChange} onInit={(value) => { instance.current = value }} onMoveEnd={(_, next) => setViewport(next)} onNodeClick={(_, node) => { select(node.id); if (node.data.editorKind === 'action' || node.data.editorKind === 'binding') onNodeOpen?.(node.id) }} onNodeDragStart={onNodeDragStart} onNodeDragStop={commitEdit} onNodesChange={onNodesChange} onPaneClick={() => { select(undefined); onPaneClear?.() }} onReconnect={onReconnect} onReconnectEnd={() => editor.clearEdgeReconnectRequest()} isValidConnection={valid} multiSelectionKeyCode="Shift" onlyRenderVisibleElements={nodes.length >= LARGE_GRAPH_RENDER_THRESHOLD} proOptions={{ hideAttribution: true }} selectionOnDrag>
      <Background color="var(--ui-canvas-dot)" gap={22} size={1} variant={BackgroundVariant.Dots} />
      <Controls className="!border-border !bg-surface !shadow-md" />
      {nodes.length < LARGE_GRAPH_MINIMAP_THRESHOLD && <MiniMap className="!border !border-border !bg-surface" maskColor="color-mix(in srgb, var(--ui-background) 70%, transparent)" pannable zoomable />}
    </ReactFlow>
  </div>
})

type GroupView = { id: string; group: EditorDocument['groups'][number]; position: { x: number; y: number }; width: number; height: number }

class GroupViewIndex {
  private readonly views = new Map<string, GroupView>()
  private nodeSignatures = new Map<string, string>()
  private memberships = new Map<string, string[]>()
  private groupSignatures = new Map<string, string>()
  private manifests?: Map<string, NodeManifest>

  sync(nodes: StudioNode[], groups: EditorDocument['groups'], manifests: Map<string, NodeManifest>) {
    const nodeMap = new Map(nodes.map((node) => [node.id, node]))
    const memberships = new Map<string, string[]>()
    const affected = new Set<string>()
    const groupIds = new Set(groups.map((group) => group.id))
    for (const group of groups) {
      const signature = JSON.stringify(group)
      if (this.groupSignatures.get(group.id) !== signature || this.manifests !== manifests) affected.add(group.id)
      this.groupSignatures.set(group.id, signature)
      for (const nodeId of group.nodeIds) memberships.set(nodeId, [...(memberships.get(nodeId) ?? []), group.id])
    }
    for (const id of this.groupSignatures.keys()) if (!groupIds.has(id)) { this.groupSignatures.delete(id); this.views.delete(id) }
    const nodeIds = new Set(nodes.map((node) => node.id))
    for (const node of nodes) {
      const signature = `${node.position.x}:${node.position.y}:${node.width}:${node.height}:${node.measured?.width}:${node.measured?.height}:${node.data.editorKind === 'action' ? `${node.data.nodeType}@${node.data.typeVersion}` : node.data.editorKind}`
      if (this.nodeSignatures.get(node.id) !== signature) for (const groupId of new Set([...(this.memberships.get(node.id) ?? []), ...(memberships.get(node.id) ?? [])])) affected.add(groupId)
      this.nodeSignatures.set(node.id, signature)
    }
    for (const id of this.nodeSignatures.keys()) if (!nodeIds.has(id)) { for (const groupId of this.memberships.get(id) ?? []) affected.add(groupId); this.nodeSignatures.delete(id) }
    for (const group of groups) if (affected.has(group.id)) {
      const view = computeGroupView(group, nodeMap, manifests)
      if (view) this.views.set(group.id, view)
      else this.views.delete(group.id)
    }
    this.memberships = memberships
    this.manifests = manifests
    return groups.flatMap((group) => { const view = this.views.get(group.id); return view ? [view] : [] })
  }
}

function computeGroupView(group: EditorDocument['groups'][number], nodeMap: Map<string, StudioNode>, manifests: Map<string, NodeManifest>): GroupView | undefined {
  const members = group.nodeIds.flatMap((id) => { const node = nodeMap.get(id); return node ? [node] : [] })
  if (!members.length) return undefined
  const bounds = members.map((node) => ({ node, metrics: nodeMetrics(node, manifests) }))
  const left = Math.min(...bounds.map(({ node }) => node.position.x))
  const top = Math.min(...bounds.map(({ node }) => node.position.y))
  const right = Math.max(...bounds.map(({ node, metrics }) => node.position.x + metrics.width))
  const bottom = Math.max(...bounds.map(({ node, metrics }) => node.position.y + metrics.height))
  const collapsedMetrics = canvasNodeMetrics('default', { kind: 'group' }, true)
  return { id: `group:${group.id}`, group, position: { x: left - 32, y: top - 48 }, width: group.collapsed ? collapsedMetrics.width : right - left + 64, height: group.collapsed ? collapsedMetrics.height : bottom - top + 80 }
}

function nodeMetrics(node: StudioNode, manifests: Map<string, NodeManifest>) {
  if (node.data.editorKind === 'binding') return canvasNodeMetrics('default', { kind: 'binding' })
  if (node.data.editorKind !== 'action') return { width: 96, height: 96 }
  const manifest = manifests.get(`${node.data.nodeType}@${node.data.typeVersion}`)
  const metrics = canvasNodeMetrics(canvasNodeRole(manifest), { inputs: manifest?.inputPorts.length, outputs: manifest?.outputPorts.length, bindings: manifest?.bindingSlots.length, richHeight: node.height ?? node.measured?.height })
  return { width: node.width ?? node.measured?.width ?? metrics.width, height: (node.height ?? node.measured?.height ?? metrics.height) + (metrics.labelBelow ? 28 : 0) }
}

function materializeNodes(nodes: StudioNode[], manifests: Map<string, NodeManifest>, cache: Map<string, { source: StudioNode; manifest?: NodeManifest; value: StudioNode }>) {
  const ids = new Set(nodes.map((node) => node.id))
  for (const id of cache.keys()) if (!ids.has(id)) cache.delete(id)
  return nodes.map((node) => {
    const manifest = node.data.editorKind === 'action' ? manifests.get(`${node.data.nodeType}@${node.data.typeVersion}`) : undefined
    const cached = cache.get(node.id)
    if (cached?.source === node && cached.manifest === manifest) return cached.value
    const value = withInitialNodeMetrics(node, manifest)
    cache.set(node.id, { source: node, manifest, value })
    return value
  })
}

function withInitialNodeMetrics(node: StudioNode, manifest?: NodeManifest): StudioNode {
  if (node.data.editorKind === 'binding') {
    const metrics = canvasNodeMetrics('default', { kind: 'binding' })
    return { ...node, initialWidth: metrics.width, initialHeight: metrics.height, style: { ...node.style, zIndex: 3 } }
  }
  if (node.data.editorKind !== 'action') return node
  const metrics = canvasNodeMetrics(canvasNodeRole(manifest))
  return { ...node, initialWidth: metrics.width, initialHeight: metrics.height, style: { ...node.style, zIndex: 3 } }
}

export function materializeRuntimeEdges(edges: StudioEdge[], runtimeStatuses: Map<string, string>, cache = new Map<string, { source: StudioEdge; sourceStatus?: string; targetStatus?: string; value: StudioEdge }>()) {
  const ids = new Set(edges.map((edge) => edge.id))
  for (const id of cache.keys()) if (!ids.has(id)) cache.delete(id)
  return edges.map((edge) => {
    const sourceStatus = runtimeStatuses.get(edge.source)
    const targetStatus = runtimeStatuses.get(edge.target)
    const cached = cache.get(edge.id)
    if (cached?.source === edge && cached.sourceStatus === sourceStatus && cached.targetStatus === targetStatus) return cached.value
    const runtimeStatus = edge.data?.edgeKind === 'binding' ? undefined : edgeRuntimeStatus(sourceStatus, targetStatus)
    const color = edge.data?.sourcePortKind === 'error' || runtimeStatus === 'failed' ? 'var(--ui-danger)' : runtimeStatus === 'running' ? 'var(--ui-warning)' : runtimeStatus === 'succeeded' ? 'var(--ui-success)' : 'var(--ui-primary)'
    const value: StudioEdge = { ...edge, zIndex: 0, data: edge.data ? { ...edge.data, runtimeStatus } : edge.data, markerEnd: edge.data?.edgeKind === 'binding' ? undefined : { type: MarkerType.ArrowClosed, color } }
    cache.set(edge.id, { source: edge, sourceStatus, targetStatus, value })
    return value
  })
}

function edgeRuntimeStatus(source?: string, target?: string) {
  if (source === 'failed' || target === 'failed') return 'failed'
  if (source === 'running' || target === 'running') return 'running'
  if (source === 'succeeded' && target === 'succeeded') return 'succeeded'
  return undefined
}

function annotationNode(annotation: EditorDocument['annotations'][number], editor: Pick<ReturnType<typeof useEditorStore.getState>, 'updateAnnotation' | 'removeAnnotation' | 'beginEdit' | 'commitEdit' | 'updateAnnotationFrame'>): CanvasNode {
  return {
    id: `annotation:${annotation.id}`,
    type: 'annotation',
    position: { x: annotation.x, y: annotation.y },
    style: { width: annotation.width ?? 240, height: annotation.height ?? 160, zIndex: 1 },
    dragHandle: '.drag-handle',
    data: {
      editorKind: 'annotation', annotationId: annotation.id, text: annotation.text, color: annotation.color,
      onChange: (patch) => editor.updateAnnotation(annotation.id, patch),
      onRemove: () => editor.removeAnnotation(annotation.id),
      onResizeStart: () => editor.beginEdit({ annotationIds: [annotation.id] }),
      onResize: (frame) => editor.updateAnnotationFrame(annotation.id, frame),
      onResizeEnd: editor.commitEdit,
    },
  }
}

function groupNode(view: GroupView, editor: Pick<ReturnType<typeof useEditorStore.getState>, 'toggleGroup' | 'removeGroup'>): CanvasNode {
  return {
    id: view.id,
    type: 'group',
    position: view.position,
    style: { width: view.width, height: view.height, zIndex: 2, pointerEvents: view.group.collapsed ? 'auto' : 'none' },
    dragHandle: '.drag-handle',
    selectable: false,
    connectable: false,
    data: { editorKind: 'group', groupId: view.group.id, label: view.group.label, collapsed: Boolean(view.group.collapsed), color: view.group.color, memberCount: view.group.nodeIds.length, onToggle: () => editor.toggleGroup(view.group.id), onRemove: () => editor.removeGroup(view.group.id) },
  }
}

function LegendDot({ className, label }: { className: string; label: string }) { return <span className="flex items-center gap-1.5"><span className={`size-2.5 rounded-full border-2 border-background shadow-sm ${className}`} />{label}</span> }
