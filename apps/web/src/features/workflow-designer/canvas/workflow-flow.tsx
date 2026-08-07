import { Background, BackgroundVariant, Controls, MiniMap, ReactFlow, type Connection, type NodeChange, type ReactFlowInstance } from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import { Plus, Search } from 'lucide-react'
import { forwardRef, useCallback, useImperativeHandle, useMemo, useRef, type DragEvent } from 'react'
import { useTranslation } from 'react-i18next'

import type { CanvasNode, EditorDocument, NodeManifest, ResourceType, StudioEdge, StudioNode } from '../model/types'
import { StudioEdgeComponent } from '../edges/studio-edge'
import { AnnotationNode, GroupNode } from './editor-overlays'
import { AttachmentNode, ManifestNode, type NodeBindingSummary } from '../nodes/manifest-node'
import { canvasNodeMetrics, canvasNodeRole } from '../nodes/node-appearance'
import { useEditorStore } from '../store/editor-store'
import { validateConnection } from '../utils/connections'
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
  onSearchNodes?: (source?: { nodeId: string; handleId: string }) => void
  onAddTrigger?: () => void
}

export type WorkflowFlowHandle = {
  getViewportCenter: () => { x: number; y: number } | undefined
  getViewportBounds: () => { x: number; y: number; width: number; height: number } | undefined
}

export const WorkflowFlow = forwardRef<WorkflowFlowHandle, FlowProps>(function WorkflowFlow({ manifests, runtimeStatuses, onDropAction, onDropBinding, onNodeOpen, onPaneClear, onSearchNodes, onAddTrigger }, ref) {
  const { t } = useTranslation()
  const instance = useRef<ReactFlowInstance<CanvasNode, StudioEdge>>()
  const editor = useEditorStore()
  const { nodes, edges, annotations, groups, viewport, onEdgesChange, connect, select, setViewport, beginEdit } = editor
  const groupViews = useMemo(() => buildGroupViews(nodes, groups, manifests), [groups, manifests, nodes])
  const collapsedByMember = useMemo(() => {
    const result = new Map<string, string>()
    for (const view of groupViews) if (view.group.collapsed) for (const nodeId of view.group.nodeIds) result.set(nodeId, view.id)
    return result
  }, [groupViews])
  const flowNodes = useMemo<CanvasNode[]>(() => [
    ...nodes.filter((node) => !collapsedByMember.has(node.id)),
    ...annotations.map((annotation) => annotationNode(annotation, editor)),
    ...groupViews.map((view) => groupNode(view, editor)),
  ], [annotations, collapsedByMember, editor, groupViews, nodes])
  const flowEdges = useMemo(() => proxyEdges(edges, collapsedByMember), [collapsedByMember, edges])
  const groupViewMap = useMemo(() => new Map(groupViews.map((view) => [view.id, view])), [groupViews])
  const bindingSummaries = useMemo(() => buildBindingSummaries(nodes, edges), [edges, nodes])
  const nodeTypes = useMemo(() => ({
    manifest: (props: Parameters<typeof ManifestNode>[0]) => { const data = props.data.editorKind === 'action' ? props.data : undefined; return <ManifestNode {...props} bindingSummaries={bindingSummaries.get(props.id)} manifest={data ? manifests.get(`${data.nodeType}@${data.typeVersion}`) : undefined} onQuickAdd={onSearchNodes ? (nodeId, handleId) => onSearchNodes({ nodeId, handleId }) : undefined} primary={editor.settings.primaryOutputNodeId === props.id} runtimeStatus={runtimeStatuses.get(props.id)} zoom={viewport.zoom} /> },
    attachment: AttachmentNode,
    annotation: AnnotationNode,
    group: GroupNode,
  }), [bindingSummaries, editor.settings.primaryOutputNodeId, manifests, onSearchNodes, runtimeStatuses, viewport.zoom])
  const edgeTypes = useMemo(() => ({ studio: StudioEdgeComponent }), [])
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
  const valid = useCallback((connection: Connection | StudioEdge) => validateConnection(connection, nodes, edges, manifests), [edges, manifests, nodes])
  const onConnect = useCallback((connection: Connection) => {
    if (!valid(connection)) return
    const source = nodes.find((node) => node.id === connection.source)
    const binding = source?.data.editorKind === 'binding'
    const sourceManifest = source?.data.editorKind === 'action' ? manifests.get(`${source.data.nodeType}@${source.data.typeVersion}`) : undefined
    const sourcePortKind = sourceManifest?.outputPorts.find((port) => port.name === connection.sourceHandle)?.kind
    const order = edges.filter((edge) => edge.source === connection.source && edge.data?.edgeKind === 'execution').length
    connect(connection, { edgeKind: binding ? 'binding' : 'execution', order: binding ? undefined : order, targetSlot: binding ? connection.targetHandle?.replace(/^binding:/, '') : undefined, sourcePortKind })
  }, [connect, edges, manifests, nodes, valid])
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
  return <div className="relative min-w-0 flex-1 bg-canvas" data-testid="workflow-canvas" onDragOver={(event) => { event.preventDefault(); event.dataTransfer.dropEffect = 'copy' }} onDrop={onDrop}>
    <div className="pointer-events-none absolute right-3 top-3 z-10 flex items-center gap-3 rounded-md border border-border bg-surface/90 px-2.5 py-1.5 text-[9px] text-muted-foreground shadow-sm backdrop-blur"><LegendDot className="bg-primary" label={t('studio.ports.flow')} /><LegendDot className="bg-danger" label={t('studio.ports.error')} /><LegendDot className="bg-warning" label={t('studio.ports.resource')} /></div>
    {actionCount === 0 && <div className="pointer-events-none absolute inset-0 z-10 grid place-items-center"><div className="pointer-events-auto flex items-center gap-2 rounded-md border border-border bg-surface/95 p-2 shadow-md"><button className="flex h-9 items-center gap-2 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground" onClick={onAddTrigger} type="button"><Plus className="size-4" />{t('studio.empty.addTrigger')}</button><button className="flex h-9 items-center gap-2 rounded-md px-3 text-xs text-muted-foreground hover:bg-muted hover:text-foreground" onClick={() => onSearchNodes?.()} type="button"><Search className="size-4" />{t('studio.empty.search')}</button></div></div>}
    <ReactFlow<CanvasNode, StudioEdge> defaultViewport={viewport} edges={flowEdges} edgeTypes={edgeTypes} fitView nodes={flowNodes} nodeTypes={nodeTypes} onConnect={onConnect} onEdgesChange={onEdgesChange} onInit={(value) => { instance.current = value }} onMoveStart={beginEdit} onMoveEnd={(_, next) => setViewport(next)} onNodeClick={(_, node) => { select(node.id); if (node.data.editorKind === 'action' || node.data.editorKind === 'binding') onNodeOpen?.(node.id) }} onNodeDragStart={beginEdit} onNodesChange={onNodesChange} onPaneClick={() => { select(undefined); onPaneClear?.() }} isValidConnection={valid} multiSelectionKeyCode="Shift" onlyRenderVisibleElements={nodes.length >= LARGE_GRAPH_RENDER_THRESHOLD} proOptions={{ hideAttribution: true }} selectionOnDrag>
      <Background color="var(--ui-canvas-dot)" gap={22} size={1} variant={BackgroundVariant.Dots} />
      <Controls className="!border-border !bg-surface !shadow-md" />
      {nodes.length < LARGE_GRAPH_MINIMAP_THRESHOLD && <MiniMap className="!border !border-border !bg-surface" maskColor="color-mix(in srgb, var(--ui-background) 70%, transparent)" pannable zoomable />}
    </ReactFlow>
  </div>
})

type GroupView = { id: string; group: EditorDocument['groups'][number]; position: { x: number; y: number }; width: number; height: number }

function buildGroupViews(nodes: StudioNode[], groups: EditorDocument['groups'], manifests: Map<string, NodeManifest>): GroupView[] {
  const nodeMap = new Map(nodes.map((node) => [node.id, node]))
  return groups.flatMap((group) => {
    const members = group.nodeIds.flatMap((id) => { const node = nodeMap.get(id); return node ? [node] : [] })
    if (!members.length) return []
    const bounds = members.map((node) => ({ node, metrics: nodeMetrics(node, manifests) }))
    const left = Math.min(...bounds.map(({ node }) => node.position.x))
    const top = Math.min(...bounds.map(({ node }) => node.position.y))
    const right = Math.max(...bounds.map(({ node, metrics }) => node.position.x + metrics.width))
    const bottom = Math.max(...bounds.map(({ node, metrics }) => node.position.y + metrics.height))
    const collapsedMetrics = canvasNodeMetrics('default', { kind: 'group' }, true)
    return [{ id: `group:${group.id}`, group, position: { x: left - 32, y: top - 48 }, width: group.collapsed ? collapsedMetrics.width : right - left + 64, height: group.collapsed ? collapsedMetrics.height : bottom - top + 80 }]
  })
}

function nodeMetrics(node: StudioNode, manifests: Map<string, NodeManifest>) {
  if (node.data.editorKind === 'binding') return canvasNodeMetrics('default', { kind: 'binding' })
  if (node.data.editorKind !== 'action') return { width: 96, height: 96 }
  const manifest = manifests.get(`${node.data.nodeType}@${node.data.typeVersion}`)
  const metrics = canvasNodeMetrics(canvasNodeRole(manifest), { inputs: manifest?.inputPorts.length, outputs: manifest?.outputPorts.length, bindings: manifest?.bindingSlots.length, richHeight: node.height ?? node.measured?.height })
  return { width: node.width ?? node.measured?.width ?? metrics.width, height: (node.height ?? node.measured?.height ?? metrics.height) + (metrics.labelBelow ? 28 : 0) }
}

function annotationNode(annotation: EditorDocument['annotations'][number], editor: ReturnType<typeof useEditorStore.getState>): CanvasNode {
  return {
    id: `annotation:${annotation.id}`,
    type: 'annotation',
    position: { x: annotation.x, y: annotation.y },
    style: { width: annotation.width ?? 240, height: annotation.height ?? 160, zIndex: -20 },
    dragHandle: '.drag-handle',
    data: {
      editorKind: 'annotation', annotationId: annotation.id, text: annotation.text, color: annotation.color,
      onChange: (patch) => editor.updateAnnotation(annotation.id, patch),
      onRemove: () => editor.removeAnnotation(annotation.id),
      onResizeStart: editor.beginEdit,
      onResize: (frame) => editor.updateAnnotationFrame(annotation.id, frame),
    },
  }
}

function groupNode(view: GroupView, editor: ReturnType<typeof useEditorStore.getState>): CanvasNode {
  return {
    id: view.id,
    type: 'group',
    position: view.position,
    style: { width: view.width, height: view.height, zIndex: view.group.collapsed ? 2 : -10, pointerEvents: view.group.collapsed ? 'auto' : 'none' },
    dragHandle: '.drag-handle',
    selectable: false,
    connectable: false,
    data: { editorKind: 'group', groupId: view.group.id, label: view.group.label, collapsed: Boolean(view.group.collapsed), color: view.group.color, memberCount: view.group.nodeIds.length, onToggle: () => editor.toggleGroup(view.group.id), onRemove: () => editor.removeGroup(view.group.id) },
  }
}

function buildBindingSummaries(nodes: StudioNode[], edges: StudioEdge[]) {
  const bindings = new Map(nodes.flatMap((node) => node.data.editorKind === 'binding' ? [[node.id, node.data] as const] : []))
  const summaries = new Map<string, NodeBindingSummary[]>()
  for (const edge of edges) {
    if (edge.data?.edgeKind !== 'binding') continue
    const binding = bindings.get(edge.source)
    if (!binding) continue
    const role = edge.data.targetSlot ?? binding.bindingRole ?? 'resource'
    const summary = { role, resourceType: binding.resourceType, label: binding.resourceName ?? binding.label }
    summaries.set(edge.target, [...(summaries.get(edge.target) ?? []), summary])
  }
  return summaries
}

function LegendDot({ className, label }: { className: string; label: string }) { return <span className="flex items-center gap-1.5"><span className={`size-2.5 rounded-full border-2 border-background shadow-sm ${className}`} />{label}</span> }
