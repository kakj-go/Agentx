import { Background, BackgroundVariant, Controls, MiniMap, ReactFlow, type Connection, type ReactFlowInstance } from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import { useCallback, useMemo, useRef, type DragEvent } from 'react'
import { useTranslation } from 'react-i18next'

import type { NodeManifest, ResourceType, StudioEdge, StudioNode } from '../model/types'
import { StudioEdgeComponent } from '../edges/studio-edge'
import { AttachmentNode, ManifestNode } from '../nodes/manifest-node'
import { useEditorStore } from '../store/editor-store'
import { validateConnection } from '../utils/connections'

export const LARGE_GRAPH_RENDER_THRESHOLD = 150
export const LARGE_GRAPH_MINIMAP_THRESHOLD = 300

export function WorkflowFlow({ manifests, runtimeStatuses, onDropAction, onDropBinding }: { manifests: Map<string, NodeManifest>; runtimeStatuses: Map<string, string>; onDropAction: (manifest: NodeManifest, position: { x: number; y: number }) => void; onDropBinding: (resourceType: ResourceType, role: string, position: { x: number; y: number }) => void }) {
  const { t } = useTranslation()
  const instance = useRef<ReactFlowInstance<StudioNode, StudioEdge>>()
  const { nodes, edges, viewport, onNodesChange, onEdgesChange, connect, select, setViewport, beginEdit } = useEditorStore()
  const nodeTypes = useMemo(() => ({
    manifest: (props: Parameters<typeof ManifestNode>[0]) => { const data = props.data.editorKind === 'action' ? props.data : undefined; return <ManifestNode {...props} manifest={data ? manifests.get(`${data.nodeType}@${data.typeVersion}`) : undefined} runtimeStatus={runtimeStatuses.get(props.id)} /> },
    attachment: AttachmentNode,
  }), [manifests, runtimeStatuses])
  const edgeTypes = useMemo(() => ({ studio: StudioEdgeComponent }), [])
  const valid = useCallback((connection: Connection | StudioEdge) => validateConnection(connection, nodes, edges, manifests), [edges, manifests, nodes])
  const onConnect = useCallback((connection: Connection) => {
    if (!valid(connection)) return
    const source = nodes.find((node) => node.id === connection.source)
    const binding = source?.data.editorKind === 'binding'
    const order = edges.filter((edge) => edge.source === connection.source && edge.data?.edgeKind === 'execution').length
    connect(connection, { edgeKind: binding ? 'binding' : 'execution', order: binding ? undefined : order, targetSlot: binding ? connection.targetHandle?.replace(/^binding:/, '') : undefined })
  }, [connect, edges, nodes, valid])
  const onDrop = (event: DragEvent) => {
    event.preventDefault()
    const raw = event.dataTransfer.getData('application/agentx-studio')
    if (!raw || !instance.current) return
    const value = JSON.parse(raw) as { kind: string; nodeType?: string; version?: number; resourceType?: ResourceType; role?: string }
    const position = instance.current.screenToFlowPosition({ x: event.clientX, y: event.clientY })
    if (value.kind === 'action') { const manifest = manifests.get(`${value.nodeType}@${value.version}`); if (manifest) onDropAction(manifest, position) }
    if (value.kind === 'binding' && value.resourceType && value.role) onDropBinding(value.resourceType, value.role, position)
  }
  return <div className="relative min-w-0 flex-1 bg-canvas" data-testid="workflow-canvas" onDragOver={(event) => { event.preventDefault(); event.dataTransfer.dropEffect = 'copy' }} onDrop={onDrop}>
    <div className="pointer-events-none absolute right-3 top-3 z-10 flex items-center gap-3 rounded-md border border-border bg-surface/90 px-2.5 py-1.5 text-[9px] text-muted-foreground shadow-sm backdrop-blur"><LegendDot className="bg-primary" label={t('studio.ports.flow')} /><LegendDot className="bg-danger" label={t('studio.ports.error')} /><LegendDot className="bg-warning" label={t('studio.ports.resource')} /></div>
    <ReactFlow<StudioNode, StudioEdge> defaultViewport={viewport} edges={edges} edgeTypes={edgeTypes} fitView nodes={nodes} nodeTypes={nodeTypes} onConnect={onConnect} onEdgesChange={onEdgesChange} onInit={(value) => { instance.current = value }} onMoveEnd={(_, next) => setViewport(next)} onNodeClick={(_, node) => select(node.id)} onNodeDragStart={beginEdit} onNodesChange={onNodesChange} onPaneClick={() => select(undefined)} isValidConnection={valid} multiSelectionKeyCode="Shift" onlyRenderVisibleElements={nodes.length >= LARGE_GRAPH_RENDER_THRESHOLD} proOptions={{ hideAttribution: true }} selectionOnDrag>
      <Background color="var(--ui-canvas-dot)" gap={22} size={1} variant={BackgroundVariant.Dots} />
      <Controls className="!border-border !bg-surface !shadow-md" />
      {nodes.length < LARGE_GRAPH_MINIMAP_THRESHOLD && <MiniMap className="!border !border-border !bg-surface" maskColor="color-mix(in srgb, var(--ui-background) 70%, transparent)" pannable zoomable />}
    </ReactFlow>
  </div>
}

function LegendDot({ className, label }: { className: string; label: string }) { return <span className="flex items-center gap-1.5"><span className={`size-2.5 rounded-full border-2 border-background shadow-sm ${className}`} />{label}</span> }
