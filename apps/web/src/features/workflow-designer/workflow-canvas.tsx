import {
  addEdge, Background, BackgroundVariant, Controls, Handle, MiniMap, Position, ReactFlow,
  useEdgesState, useNodesState, type Connection, type Edge, type Node, type NodeProps,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Bot, Database, Library, MemoryStick, Play, Save, Sparkles, Trash2, Wrench } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useParams } from 'react-router-dom'

import { ApiClientError, apiRequest, jsonBody } from '../../shared/api/client'
import type { Knowledge, McpTool, MemoryResource, Model, PageResponse, Skill, Workflow, WorkflowDraft } from '../../shared/api/types'
import { cn } from '../../shared/lib/cn'
import { Button } from '../../shared/ui/button'
import { Dialog, DialogContent } from '../../shared/ui/dialog'
import { Input } from '../../shared/ui/input'
import { Select } from '../../shared/ui/select'
import { useToast } from '../../shared/ui/toast'

type NodeKind = 'manual_trigger' | 'model' | 'mcp_tool' | 'skill' | 'rag' | 'memory'
type CanvasNodeData = { kind: NodeKind; label: string; resourceId?: string; resourceName?: string }
type Definition = { schemaVersion: string; nodes: DefinitionNode[]; connections: DefinitionConnection[]; settings: Record<string, unknown> }
type DefinitionNode = { id: string; type: NodeKind; typeVersion: number; name: string; position: { x: number; y: number }; disabled: boolean; parameters: Record<string, unknown>; resourceReferences: Array<{ resourceType: string; resourceId: string; resourceVersionId?: string | null; operation: string }> }
type DefinitionConnection = { id: string; sourceNodeId: string; sourceHandle: string; targetNodeId: string; targetHandle: string }

const icons = { manual_trigger: Play, model: Bot, mcp_tool: Wrench, skill: Sparkles, rag: Library, memory: MemoryStick }
const tones = { manual_trigger: 'bg-success/10 text-success', model: 'bg-primary/10 text-primary', mcp_tool: 'bg-warning/10 text-warning', skill: 'bg-primary/10 text-primary', rag: 'bg-success/10 text-success', memory: 'bg-warning/10 text-warning' }

function ResourceNode({ data, selected }: NodeProps<Node<CanvasNodeData>>) {
  const Icon = icons[data.kind]
  return <div className={cn('min-w-52 overflow-hidden rounded-xl border bg-surface shadow-lg', selected ? 'border-primary ring-2 ring-primary/15' : 'border-border')}>
    {data.kind !== 'manual_trigger' && <Handle className="!size-2.5 !border-2 !border-background !bg-muted-foreground" id="main" position={Position.Left} type="target" />}
    <div className="flex items-center gap-3 px-3.5 py-3"><span className={cn('grid size-8 place-items-center rounded-lg', tones[data.kind])}><Icon className="size-4" /></span><span className="min-w-0"><strong className="block truncate text-xs">{data.label}</strong><span className="mt-0.5 block truncate text-[10px] text-muted-foreground">{data.resourceName ?? data.kind}</span></span></div>
    <Handle className="!size-2.5 !border-2 !border-background !bg-primary" id="main" position={Position.Right} type="source" />
  </div>
}

export function WorkflowCanvas() {
  const { workflowId = '' } = useParams()
  const { t } = useTranslation()
  const { showToast } = useToast()
  const queryClient = useQueryClient()
  const initialized = useRef(false)
  const [nodes, setNodes, onNodesChange] = useNodesState<Node<CanvasNodeData>>([])
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([])
  const [selectedId, setSelectedId] = useState<string>()
  const [addKind, setAddKind] = useState<NodeKind>('model')
  const [dirty, setDirty] = useState(false)
  const [conflictOpen, setConflictOpen] = useState(false)
  const workflow = useQuery({ queryKey: ['workflow', workflowId], queryFn: () => apiRequest<Workflow>(`/workflows/${workflowId}`) })
  const draft = useQuery({ queryKey: ['workflow-draft', workflowId], queryFn: () => apiRequest<WorkflowDraft>(`/workflows/${workflowId}/draft`) })
  const models = useQuery({ queryKey: ['models', 'canvas'], queryFn: () => apiRequest<PageResponse<Model>>('/models/aliases?pageSize=100&status=active') })
  const mcpTools = useQuery({ queryKey: ['mcp-tools', 'canvas'], queryFn: () => apiRequest<McpTool[]>('/mcp/tools') })
  const skills = useQuery({ queryKey: ['skills', 'canvas'], queryFn: () => apiRequest<PageResponse<Skill>>('/skills?pageSize=100&status=active') })
  const knowledge = useQuery({ queryKey: ['knowledge', 'canvas'], queryFn: () => apiRequest<PageResponse<Knowledge>>('/knowledge/resources?pageSize=100&status=active') })
  const memory = useQuery({ queryKey: ['memory', 'canvas'], queryFn: () => apiRequest<PageResponse<MemoryResource>>('/memory/namespaces?pageSize=100&status=active') })
  const nodeTypes = useMemo(() => ({ resource: ResourceNode }), [])

  const hydrate = useCallback((value: WorkflowDraft) => {
    initialized.current = true
    const definition = value.definition as Definition
    setNodes(definition.nodes.map((item) => ({ id: item.id, type: 'resource', position: item.position, data: { kind: item.type, label: item.name, resourceId: item.resourceReferences[0]?.resourceId } })))
    setEdges(definition.connections.map((item) => ({ id: item.id, source: item.sourceNodeId, sourceHandle: item.sourceHandle, target: item.targetNodeId, targetHandle: item.targetHandle })))
  }, [setEdges, setNodes])
  useEffect(() => {
    if (!draft.data || initialized.current) return
    hydrate(draft.data)
  }, [draft.data, hydrate])

  const resourceOptions = useMemo(() => {
    const map: Record<NodeKind, Array<{ value: string; label: string }>> = {
      manual_trigger: [], model: (models.data?.items ?? []).map((item) => ({ value: item.id, label: `${item.alias} · ${item.modelName}` })),
      mcp_tool: (mcpTools.data ?? []).filter((item) => item.enabled && item.availability === 'available').map((item) => ({ value: item.id, label: item.title ? `${item.title} · ${item.name}` : item.name })), skill: (skills.data?.items ?? []).map((item) => ({ value: item.id, label: item.name })),
      rag: (knowledge.data?.items ?? []).map((item) => ({ value: item.id, label: item.name })), memory: (memory.data?.items ?? []).map((item) => ({ value: item.id, label: item.name })),
    }
    return map
  }, [knowledge.data, mcpTools.data, memory.data, models.data, skills.data])
  useEffect(() => { setNodes((current) => current.map((node) => ({ ...node, data: { ...node.data, resourceName: resourceOptions[node.data.kind].find((option) => option.value === node.data.resourceId)?.label } }))) }, [resourceOptions, setNodes])

  const onConnect = useCallback((connection: Connection) => { setEdges((current) => addEdge({ ...connection, id: crypto.randomUUID() }, current)); setDirty(true) }, [setEdges])
  const addNode = () => { const id = crypto.randomUUID(); const count = nodes.length; setNodes((current) => [...current, { id, type: 'resource', position: { x: 160 + (count % 3) * 280, y: 140 + Math.floor(count / 3) * 180 }, data: { kind: addKind, label: t(kindLabel(addKind)) } }]); setSelectedId(id); setDirty(true) }
  const updateSelected = (data: Partial<CanvasNodeData>) => { setNodes((current) => current.map((node) => node.id === selectedId ? { ...node, data: { ...node.data, ...data } } : node)); setDirty(true) }
  const removeSelected = () => { if (!selectedId) return; setNodes((current) => current.filter((node) => node.id !== selectedId)); setEdges((current) => current.filter((edge) => edge.source !== selectedId && edge.target !== selectedId)); setSelectedId(undefined); setDirty(true) }
  const serialize = (): Definition => ({
    schemaVersion: '2.0', settings: { executionOrder: 'n8n_v1', activationBudget: 10000 },
    nodes: nodes.map((node) => ({ id: node.id, type: node.data.kind, typeVersion: 1, name: node.data.label, position: node.position, disabled: false, parameters: {}, resourceReferences: node.data.kind === 'manual_trigger' ? [] : [{ resourceType: node.data.kind === 'model' ? 'model' : node.data.kind, resourceId: node.data.resourceId ?? '', resourceVersionId: null, operation: node.data.kind === 'rag' || node.data.kind === 'memory' ? 'read' : 'use' }] })),
    connections: edges.map((edge) => ({ id: edge.id, sourceNodeId: edge.source, sourceHandle: edge.sourceHandle ?? 'main', targetNodeId: edge.target, targetHandle: edge.targetHandle ?? 'main' })),
  })
  const save = useMutation({ mutationFn: () => apiRequest<WorkflowDraft>(`/workflows/${workflowId}/draft`, { method: 'PUT', headers: { 'Idempotency-Key': crypto.randomUUID() }, body: jsonBody({ expectedRevision: draft.data?.revision ?? 0, definition: serialize() }) }), onSuccess: async () => { setDirty(false); await Promise.all([queryClient.invalidateQueries({ queryKey: ['workflow-draft', workflowId] }), queryClient.invalidateQueries({ queryKey: ['workflow', workflowId] }), queryClient.invalidateQueries({ queryKey: ['workflow-validation', workflowId] })]); showToast(t('m2.saved')) }, onError: (error: Error) => { if (error instanceof ApiClientError && error.detail.code === 'DRAFT_REVISION_CONFLICT') setConflictOpen(true); else showToast(error.message) } })
  const reloadServerDraft = async () => {
    const result = await draft.refetch()
    if (result.data) hydrate(result.data)
    setDirty(false)
    setConflictOpen(false)
  }
  const selected = nodes.find((node) => node.id === selectedId)
  const kindOptions: NodeKind[] = nodes.some((node) => node.data.kind === 'manual_trigger') ? ['model', 'mcp_tool', 'skill', 'rag', 'memory'] : ['manual_trigger', 'model', 'mcp_tool', 'skill', 'rag', 'memory']
  if (draft.isLoading) return <div className="grid h-full place-items-center text-sm text-muted-foreground">{t('m2.loading')}</div>
  return <div className="flex h-full min-h-[calc(100vh-64px)] flex-col">
    <Dialog onOpenChange={setConflictOpen} open={conflictOpen}><DialogContent description={t('m2.versionConflict')} title={t('m2.conflictTitle')}><div className="border-b border-border px-5 py-4"><h2 className="text-sm font-semibold">{t('m2.conflictTitle')}</h2><p className="mt-2 text-xs leading-5 text-muted-foreground">{t('m2.versionConflict')}</p></div><div className="flex justify-end gap-2 px-5 py-4"><Button onClick={() => setConflictOpen(false)} variant="secondary">{t('m2.keepEditing')}</Button><Button onClick={() => void reloadServerDraft()}>{t('m2.reloadServer')}</Button></div></DialogContent></Dialog>
    <div className="flex h-16 shrink-0 items-center gap-3 border-b border-border bg-surface px-5"><Button asChild size="sm" variant="ghost"><Link to={`/workflows/${workflowId}`}>← {t('m2.backToDetail')}</Link></Button><div className="h-6 w-px bg-border" /><div><h1 className="text-sm font-semibold">{workflow.data?.name ?? t('pages.workflows.title')}</h1><p className="mt-0.5 text-[10px] text-muted-foreground">{t('m2.revision', { revision: draft.data?.revision ?? 0 })} · {dirty ? t('m2.unsaved') : t('m2.saved')}{!dirty && draft.data?.updatedAt ? ` · ${t('m2.savedAt', { time: new Date(draft.data.updatedAt).toLocaleTimeString() })}` : ''}</p></div><div className="flex-1" /><Select aria-label={t('m2.addNode')} onValueChange={(value) => setAddKind(value as NodeKind)} options={kindOptions.map((kind) => ({ value: kind, label: t(kindLabel(kind)) }))} value={addKind} /><Button onClick={addNode} size="sm" variant="secondary">+ {t('m2.addNode')}</Button><Button disabled={!dirty || save.isPending} onClick={() => save.mutate()} size="sm"><Save className="size-3.5" />{t('m2.saveDraft')}</Button><Button disabled size="sm" title={t('m2.executionDisabled')}><Play className="size-3.5" />{t('editor.run')}</Button></div>
    <div className="flex min-h-0 flex-1"><div className="relative min-w-0 flex-1 bg-canvas"><ReactFlow edges={edges} fitView nodeTypes={nodeTypes} nodes={nodes} onConnect={onConnect} onEdgesChange={(changes) => { onEdgesChange(changes); if (initialized.current) setDirty(true) }} onNodeClick={(_, node) => setSelectedId(node.id)} onNodesChange={(changes) => { onNodesChange(changes); if (initialized.current && changes.some((change) => change.type === 'position' && !change.dragging)) setDirty(true) }} proOptions={{ hideAttribution: true }}><Background color="var(--ui-canvas-dot)" gap={22} size={1} variant={BackgroundVariant.Dots} /><Controls className="!border-border !bg-surface !shadow-md" /><MiniMap className="!border !border-border !bg-surface" maskColor="color-mix(in srgb, var(--ui-background) 70%, transparent)" /></ReactFlow><p className="absolute bottom-4 left-1/2 -translate-x-1/2 rounded-full bg-surface/90 px-3 py-1.5 text-[10px] text-muted-foreground shadow">{t('m2.connectHint')}</p></div>
      <aside className="w-80 shrink-0 border-l border-border bg-surface p-5"><h2 className="text-sm font-semibold">{t('m2.nodeSettings')}</h2>{selected ? <div className="mt-5 space-y-4"><label className="block text-xs"><span className="mb-2 block">{t('m2.nodeName')}</span><Input onChange={(event) => updateSelected({ label: event.target.value })} value={selected.data.label} /></label>{selected.data.kind !== 'manual_trigger' && <label className="block text-xs"><span className="mb-2 block">{t('m2.selectResource')}</span><Select className="w-full" onValueChange={(resourceId) => updateSelected({ resourceId, resourceName: resourceOptions[selected.data.kind].find((item) => item.value === resourceId)?.label })} options={resourceOptions[selected.data.kind]} value={selected.data.resourceId ?? ''} /></label>}<Button className="w-full" onClick={removeSelected} variant="secondary"><Trash2 className="size-4" />{t('m2.deleteNode')}</Button></div> : <p className="mt-5 text-xs text-muted-foreground">{t('m2.draftEmpty')}</p>}<div className="mt-8 rounded-lg border border-primary/20 bg-primary/5 p-3 text-[11px] text-muted-foreground"><Database className="mb-2 size-4 text-primary" />{t('m2.executionDisabled')}</div></aside>
    </div>
  </div>
}

function kindLabel(kind: NodeKind) { return ({ manual_trigger: 'm2.manualTrigger', model: 'm2.modelNode', mcp_tool: 'm2.mcpToolNode', skill: 'm2.skillNode', rag: 'm2.ragNode', memory: 'm2.memoryNode' } as const)[kind] }
