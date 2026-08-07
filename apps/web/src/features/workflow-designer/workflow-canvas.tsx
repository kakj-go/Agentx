import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { AlertTriangle } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router-dom'

import { ApiClientError, apiRequest, jsonBody } from '../../shared/api/client'
import type { Workflow, WorkflowDeployment, WorkflowDraft, WorkflowEnvironment, WorkflowVersion } from '../../shared/api/types'
import { Button } from '../../shared/ui/button'
import { Dialog, DialogContent } from '../../shared/ui/dialog'
import { Select } from '../../shared/ui/select'
import { useToast } from '../../shared/ui/toast'
import { loadNodeCatalog, saveDraft, startDebugExecution, validateDraft, cancelExecution } from './api/studio-api'
import { useExecutionEvents } from './api/use-execution-events'
import { useResourceOptions } from './api/use-resource-options'
import { WorkflowFlow, type WorkflowFlowHandle } from './canvas/workflow-flow'
import { deserializeDraft, serializeStudio } from './model/serializer'
import type { NodeManifest, ResourceType, StudioDocument } from './model/types'
import { canvasNodeMetrics, canvasNodeRole, findOpenCanvasPosition, type CanvasPlacementRect } from './nodes/node-appearance'
import { NodeInspector } from './panels/node-inspector'
import { NodePalette } from './panels/node-palette'
import { RuntimePanel } from './panels/runtime-panel'
import { StudioToolbar } from './panels/studio-toolbar'
import { VersionDialog } from './panels/version-dialog'
import { DebugRunDialog, type DebugInputSource } from './panels/debug-run-dialog'
import { useEditorStore } from './store/editor-store'
import { autoLayout } from './utils/layout'
import { useStudioShortcuts } from './utils/use-studio-shortcuts'
import { configurationIssues } from './utils/configuration'
import { includedActionNodeIds } from './utils/debug-plan'

type DebugMode = 'full' | 'single_node' | 'to_node' | 'from_node'
type LocalRecovery = { revision: number; savedAt: string; document: StudioDocument }
type StudioValidationIssue = { code: string; message: string; nodeId?: string | null; fieldPath?: string | null }

export function WorkflowCanvas() {
  const { workflowId = '' } = useParams()
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const hydrated = useRef(false)
  const [revision, setRevision] = useState(0)
  const [mode, setMode] = useState<DebugMode>('full')
  const [executionId, setExecutionId] = useState<string>()
  const [conflictOpen, setConflictOpen] = useState(false)
  const [recovery, setRecovery] = useState<LocalRecovery>()
  const [issues, setIssues] = useState<StudioValidationIssue[]>([])
  const [locatedIssue, setLocatedIssue] = useState<StudioValidationIssue>()
  const [publishOpen, setPublishOpen] = useState(false)
  const [versionsOpen, setVersionsOpen] = useState(false)
  const [publishEnvironment, setPublishEnvironment] = useState('')
  const [publishVersion, setPublishVersion] = useState('')
  const [overlayIds, setOverlayIds] = useState<Record<string, string>>({})
  const [debugDialogOpen, setDebugDialogOpen] = useState(false)
  const [detailsOpen, setDetailsOpen] = useState(false)
  const [creatorOpen, setCreatorOpen] = useState(false)
  const [creatorSource, setCreatorSource] = useState<{ nodeId: string; handleId: string }>()
  const flowRef = useRef<WorkflowFlowHandle>(null)
  const previousPrimaryRef = useRef<string>()

  const workflow = useQuery({ queryKey: ['workflow', workflowId], queryFn: () => apiRequest<Workflow>(`/workflows/${workflowId}`) })
  const draft = useQuery({ queryKey: ['workflow-draft', workflowId], queryFn: () => apiRequest<WorkflowDraft>(`/workflows/${workflowId}/draft`) })
  const catalog = useQuery({ queryKey: ['node-definitions'], queryFn: loadNodeCatalog, staleTime: 60_000 })
  const versions = useQuery({ queryKey: ['workflow-versions', workflowId], queryFn: () => apiRequest<WorkflowVersion[]>(`/workflows/${workflowId}/versions`) })
  const environments = useQuery({ queryKey: ['environments'], queryFn: () => apiRequest<WorkflowEnvironment[]>('/environments') })
  const deployments = useQuery({ queryKey: ['workflow-deployments', workflowId], queryFn: () => apiRequest<WorkflowDeployment[]>(`/workflows/${workflowId}/deployments`) })
  const resources = useResourceOptions()
  const manifests = useMemo(() => (catalog.data ?? []).map((item) => item.manifest), [catalog.data])
  const manifestMap = useMemo(() => new Map(manifests.map((manifest) => [`${manifest.nodeType}@${manifest.version}`, manifest])), [manifests])

  const editor = useEditorStore()
  const selected = editor.nodes.find((node) => node.id === editor.selectedId)
  const selectedManifest = selected?.data.editorKind === 'action' ? manifestMap.get(`${selected.data.nodeType}@${selected.data.typeVersion}`) : undefined
  const creatorSourceManifest = creatorSource ? (() => { const node = editor.nodes.find((item) => item.id === creatorSource.nodeId); return node?.data.editorKind === 'action' ? manifestMap.get(`${node.data.nodeType}@${node.data.typeVersion}`) : undefined })() : undefined
  const runtime = useExecutionEvents(executionId)
  const revalidatePaste = useCallback(async (nodes: StudioDocument['nodes'], edges: StudioDocument['edges']) => {
    const state = useEditorStore.getState()
    const candidate = studioDocument({ ...state, nodes: [...state.nodes, ...nodes], edges: [...state.edges, ...edges] })
    const pastedIds = new Set(nodes.map((node) => node.id))
    const local = configurationIssues(candidate, manifestMap).filter((issue) => issue.nodeId && pastedIds.has(issue.nodeId))
    if (!resources.loading) {
      for (const node of nodes) {
        if (node.data.editorKind !== 'binding' || !node.data.resourceId) continue
        const data = node.data
        const visible = resources.options[data.resourceType]?.some((option) => option.value === data.resourceId)
        if (!visible) local.push({ code: 'PASTED_RESOURCE_UNAVAILABLE', nodeId: node.id, fieldPath: 'resourceId', message: `${data.resourceType} ${data.resourceId} is not visible in this Workflow.` })
      }
    }
    if (local.length) return local.map((issue) => issue.message)
    const serialized = serializeStudio(candidate)
    const result = await validateDraft(workflowId, serialized.definition, serialized.editorDocument)
    return result.issues.filter((issue) => issue.severity === 'error' && (!issue.nodeId || pastedIds.has(issue.nodeId))).map((issue) => issue.message)
  }, [manifestMap, resources.loading, resources.options, workflowId])
  useStudioShortcuts({
    workflowId,
    revalidatePaste,
    onPasteRejected: (messages) => setIssues(messages.map((message) => ({ code: 'CROSS_WORKFLOW_PASTE_REJECTED', message }))),
    onOpenNode: () => setDetailsOpen(true),
    onEscape: () => { setDetailsOpen(false); setCreatorOpen(false); setCreatorSource(undefined) },
  })

  useEffect(() => {
    if (!draft.data || hydrated.current) return
    hydrated.current = true
    setRevision(draft.data.revision)
    editor.hydrate(deserializeDraft(draft.data))
    const local = readRecovery(workflowId)
    if (local && local.revision === draft.data.revision && new Date(local.savedAt) > new Date(draft.data.updatedAt)) setRecovery(local)
  }, [draft.data, editor, workflowId])

  useEffect(() => {
    if (!editor.dirty) return
    const timer = window.setTimeout(() => writeRecovery(workflowId, revision, studioDocument(useEditorStore.getState())), 250)
    return () => window.clearTimeout(timer)
  }, [editor.nodes, editor.edges, editor.viewport, editor.annotations, editor.groups, editor.settings, editor.dirty, revision, workflowId])

  useEffect(() => {
    const beforeUnload = (event: BeforeUnloadEvent) => { if (editor.dirty) event.preventDefault() }
    window.addEventListener('beforeunload', beforeUnload)
    return () => window.removeEventListener('beforeunload', beforeUnload)
  }, [editor.dirty])

  useEffect(() => {
    const previous = previousPrimaryRef.current
    const current = editor.settings.primaryOutputNodeId ?? undefined
    if (previous && !current) {
      const node = editor.nodes.find((item) => item.id === previous)
      const message = !node
        ? 'studio.toasts.primaryClearedDeleted'
        : node.data.editorKind === 'action' && node.data.disabled
          ? 'studio.toasts.primaryClearedDisabled'
          : editor.edges.some((edge) => edge.source === previous && edge.data?.edgeKind === 'execution' && edge.data.sourcePortKind === 'main')
            ? 'studio.toasts.primaryClearedConnection'
            : undefined
      if (message) showToast(t(message))
    }
    previousPrimaryRef.current = current
  }, [editor.edges, editor.nodes, editor.settings.primaryOutputNodeId, showToast, t])

  const saveMutation = useMutation({
    mutationFn: ({ expectedRevision, document }: { expectedRevision: number; document: StudioDocument }) => {
      const serialized = serializeStudio(document)
      return saveDraft(workflowId, expectedRevision, serialized.definition, serialized.editorDocument)
    },
    onSuccess: async (value) => {
      setRevision(value.revision)
      setLocatedIssue(undefined)
      editor.markSaved()
      clearRecovery(workflowId)
      queryClient.setQueryData(['workflow-draft', workflowId], value)
      await Promise.all([queryClient.invalidateQueries({ queryKey: ['workflow', workflowId] }), queryClient.invalidateQueries({ queryKey: ['workflow-validation', workflowId] })])
      showToast(t('studio.toasts.saved'))
    },
    onError: (error: Error) => {
      if (error instanceof ApiClientError && error.detail.code === 'DRAFT_REVISION_CONFLICT') setConflictOpen(true)
      else showToast(error.message)
    },
  })
  const saveDocument = useCallback((showIssues: boolean) => {
    if (saveMutation.isPending) return
    const document = studioDocument(useEditorStore.getState())
    const clientIssues = configurationIssues(document, manifestMap)
    if (clientIssues.length) {
      if (showIssues) setIssues(clientIssues)
      return
    }
    saveMutation.mutate({ expectedRevision: revision, document })
  }, [manifestMap, revision, saveMutation])
  const saveNow = useCallback(() => saveDocument(true), [saveDocument])

  useEffect(() => {
    if (!editor.dirty || saveMutation.isPending || conflictOpen || recovery) return
    const timer = window.setTimeout(() => saveDocument(false), 1400)
    return () => window.clearTimeout(timer)
  }, [editor.nodes, editor.edges, editor.viewport, editor.annotations, editor.groups, editor.settings, editor.dirty, saveMutation.isPending, conflictOpen, recovery, saveDocument])

  const validateCurrent = async () => {
    const document = studioDocument(useEditorStore.getState())
    const clientIssues = configurationIssues(document, manifestMap)
    if (clientIssues.length) { setIssues(clientIssues); return false }
    const serialized = serializeStudio(document)
    const result = await validateDraft(workflowId, serialized.definition, serialized.editorDocument)
    const errors = result.issues.filter((issue) => issue.severity === 'error')
    setIssues(errors)
    return errors.length === 0
  }

  const executeRun = async (inputSource?: DebugInputSource, input: unknown = {}, debugMode: DebugMode = mode) => {
    if (editor.dirty || saveMutation.isPending) return showToast(t('studio.toasts.saveBeforeRun'))
    const target = selected?.data.editorKind === 'action' ? selected.id : undefined
    try {
      if (!await validateCurrent()) return
      const included = includedActionNodeIds(studioDocument(editor), debugMode, target)
      const irreversible = editor.nodes.filter((node) => included.has(node.id) && node.data.editorKind === 'action' && manifestMap.get(`${node.data.nodeType}@${node.data.typeVersion}`)?.sideEffectLevel === 'irreversible')
      if (irreversible.length && !window.confirm(t('studio.confirmIrreversible', { count: irreversible.length }))) return
      const accepted = await startDebugExecution(workflowId, {
        expectedRevision: revision,
        mode: debugMode,
        targetNodeId: target,
        input,
        inputSource,
        overlayIds: Object.entries(overlayIds).filter(([nodeId]) => included.has(nodeId)).map(([, overlayId]) => overlayId),
        sideEffectDecisions: Object.fromEntries(irreversible.map((node) => [node.id, 'execute'])),
        idempotencyKey: crypto.randomUUID(),
      })
      setExecutionId(accepted.executionId)
      runtime.setRunning(true)
      setDebugDialogOpen(false)
    } catch (error) { showToast((error as Error).message) }
  }

  const run = () => {
    if (editor.dirty || saveMutation.isPending) return showToast(t('studio.toasts.saveBeforeRun'))
    const target = selected?.data.editorKind === 'action' ? selected.id : undefined
    if (mode !== 'full' && !target) return showToast(t('studio.toasts.partialSelect'))
    if (mode === 'single_node' || mode === 'from_node') return setDebugDialogOpen(true)
    void executeRun()
  }

  const stop = async () => {
    if (!executionId) return
    try { await cancelExecution(executionId); runtime.setRunning(false) } catch (error) { showToast((error as Error).message) }
  }

  const createVersion = async () => {
    if (editor.dirty || saveMutation.isPending) return showToast(t('studio.toasts.saveBeforeRun'))
    if (!await validateCurrent()) return
    try {
      await apiRequest<WorkflowVersion>(`/workflows/${workflowId}/versions`, { method: 'POST', headers: { 'Idempotency-Key': crypto.randomUUID() }, body: jsonBody({ draftRevision: revision }) })
      await versions.refetch()
      setVersionsOpen(false)
      showToast(t('studio.toasts.versionCreated'))
    } catch (error) { showToast((error as Error).message) }
  }

  const publish = async () => {
    if (!publishEnvironment || !publishVersion) return
    try {
      await apiRequest(`/workflows/${workflowId}/deployments`, { method: 'POST', headers: { 'Idempotency-Key': crypto.randomUUID() }, body: jsonBody({ environmentId: publishEnvironment, workflowVersionId: publishVersion }) })
      setPublishOpen(false)
      await deployments.refetch()
      showToast(t('studio.toasts.published'))
    } catch (error) { showToast((error as Error).message) }
  }
  const rollback = async (deployment: WorkflowDeployment) => {
    try {
      await apiRequest(`/workflows/${workflowId}/deployments/${deployment.environmentId}/rollback`, { method: 'POST', headers: { 'Idempotency-Key': crypto.randomUUID() }, body: jsonBody({ targetWorkflowVersionId: deployment.workflowVersionId }) })
      await deployments.refetch(); showToast(t('studio.toasts.rolledBack'))
    } catch (error) { showToast((error as Error).message) }
  }

  const actionData = (manifest: NodeManifest) => ({ editorKind: 'action' as const, nodeType: manifest.nodeType, typeVersion: manifest.version, label: manifest.displayName, parameters: defaults(manifest), resourceReferences: [], settings: {}, disabled: false })
  const occupiedCanvasRects = (): CanvasPlacementRect[] => [
    ...editor.nodes.map((node) => {
      const nodeManifest = node.data.editorKind === 'action' ? manifestMap.get(`${node.data.nodeType}@${node.data.typeVersion}`) : undefined
      const metrics = node.data.editorKind === 'binding'
        ? canvasNodeMetrics('default', { kind: 'binding' })
        : canvasNodeMetrics(canvasNodeRole(nodeManifest), {
          inputs: nodeManifest?.inputPorts.length,
          outputs: nodeManifest?.outputPorts.length,
          bindings: nodeManifest?.bindingSlots.length,
          richHeight: node.height ?? node.measured?.height,
        })
      return { x: node.position.x, y: node.position.y, width: node.width ?? node.measured?.width ?? metrics.width, height: node.height ?? node.measured?.height ?? metrics.height }
    }),
    ...editor.annotations.map((annotation) => ({ x: annotation.x, y: annotation.y, width: annotation.width ?? 240, height: annotation.height ?? 160 })),
  ]
  const defaultCanvasPosition = (size: { width: number; height: number }, keepVisible = false) => {
    const center = flowRef.current?.getViewportCenter()
    return center ? findOpenCanvasPosition(center, size, occupiedCanvasRects(), keepVisible ? flowRef.current?.getViewportBounds() : undefined) : undefined
  }
  const addAction = (manifest: NodeManifest, position?: { x: number; y: number }) => {
    const metrics = canvasNodeMetrics(canvasNodeRole(manifest), { inputs: manifest.inputPorts.length, outputs: manifest.outputPorts.length, bindings: manifest.bindingSlots.length })
    return editor.addAction(actionData(manifest), position ?? defaultCanvasPosition(metrics))
  }
  const addActionFromCreator = (manifest: NodeManifest, position?: { x: number; y: number }) => {
    if (!creatorSource || !creatorSourceManifest) return addAction(manifest, position)
    const sourceNode = editor.nodes.find((node) => node.id === creatorSource.nodeId)
    const sourcePort = creatorSourceManifest.outputPorts.find((port) => port.name === creatorSource.handleId)
    const targetPort = sourcePort ? manifest.inputPorts.find((port) => port.kind === sourcePort.kind) : undefined
    if (!sourceNode || !targetPort) return addAction(manifest, position)
    const sourceMetrics = canvasNodeMetrics(canvasNodeRole(creatorSourceManifest), {
      inputs: creatorSourceManifest.inputPorts.length,
      outputs: creatorSourceManifest.outputPorts.length,
      bindings: creatorSourceManifest.bindingSlots.length,
      richHeight: sourceNode.height ?? sourceNode.measured?.height,
    })
    return editor.addConnectedAction(actionData(manifest), { nodeId: sourceNode.id, handleId: creatorSource.handleId, targetHandle: targetPort.name }, position ?? { x: sourceNode.position.x + sourceMetrics.width + 160, y: sourceNode.position.y })
  }
  const addBinding = (resourceType: ResourceType, role: string, position?: { x: number; y: number }) => editor.addBinding({ editorKind: 'binding', bindingId: crypto.randomUUID(), bindingRole: role, resourceType, operation: resourceType === 'rag' || resourceType === 'memory' ? 'read' : 'use', label: resourceType.replaceAll('_', ' ') }, position ?? defaultCanvasPosition(canvasNodeMetrics('default', { kind: 'binding' })))
  const addFirstTrigger = () => {
    const trigger = manifests.find((manifest) => manifest.uiSchema.canvas?.role === 'trigger') ?? manifests.find((manifest) => manifest.executionStyle === 'trigger')
    if (trigger) addAction(trigger)
    else setCreatorOpen(true)
  }

  const loadServer = async () => {
    const result = await draft.refetch()
    if (result.data) { editor.hydrate(deserializeDraft(result.data)); setRevision(result.data.revision); clearRecovery(workflowId) }
    setConflictOpen(false)
  }
  const overwriteServer = async () => {
    const local = studioDocument(useEditorStore.getState())
    const result = await draft.refetch()
    if (result.data) saveMutation.mutate({ expectedRevision: result.data.revision, document: local })
    setConflictOpen(false)
  }
  const locateIssue = (issue: StudioValidationIssue) => {
    setLocatedIssue(issue)
    setIssues([])
    if (!issue.nodeId) return
    editor.select(issue.nodeId)
    setDetailsOpen(true)
    window.setTimeout(() => {
      const field = [...document.querySelectorAll<HTMLElement>('[data-field-path]')].find((element) => element.dataset.fieldPath === issue.fieldPath)
      if (!field) return
      field.scrollIntoView({ block: 'center' })
      field.classList.add('studio-field-located')
      const control = field.querySelector<HTMLElement>('input, textarea, button, [role="combobox"], [contenteditable="true"]')
      if (control) control.focus()
      else { field.tabIndex = -1; field.focus() }
      window.setTimeout(() => field.classList.remove('studio-field-located'), 1800)
    }, 50)
  }

  if (draft.isLoading || catalog.isLoading) return <div className="grid h-full place-items-center text-sm text-muted-foreground">{t('studio.loading')}</div>
  if (!draft.data || catalog.error) return <div className="grid h-full place-items-center px-8 text-sm text-danger">{String(draft.error ?? catalog.error ?? t('studio.unavailable'))}</div>
  return <div className="flex h-full min-h-[calc(100vh-64px)] flex-col overflow-hidden">
    <StudioToolbar canRedo={editor.future.length > 0} canUndo={editor.past.length > 0} dirty={editor.dirty} mode={mode} name={workflow.data?.name ?? t('studio.workflow')} onAlign={editor.alignSelected} onLayout={() => void autoLayout(editor.nodes, editor.edges, manifestMap).then(editor.replaceNodes)} onMode={(value) => setMode(value as DebugMode)} onPublish={() => setPublishOpen(true)} onRedo={editor.redo} onRun={() => void run()} onSave={saveNow} onStop={() => void stop()} onUndo={editor.undo} onVersion={() => setVersionsOpen(true)} revision={revision} running={runtime.running} saving={saveMutation.isPending} selectedCount={editor.nodes.filter((node) => node.selected || node.id === editor.selectedId).length} workflowId={workflowId} />
    <main className="flex min-h-0 flex-1"><NodePalette manifests={manifests} onAddAction={(manifest) => { addActionFromCreator(manifest); setCreatorSource(undefined); setDetailsOpen(true) }} onAddAnnotation={() => { editor.addAnnotation(defaultCanvasPosition({ width: 240, height: 160 }, true), t('studio.note.default')); setCreatorSource(undefined) }} onAddBinding={(type, role) => { addBinding(type, role); setCreatorSource(undefined); setDetailsOpen(true) }} onAddGroup={() => { editor.addGroup(t('studio.group.default')); setCreatorSource(undefined) }} onOpenChange={(open) => { setCreatorOpen(open); if (!open) setCreatorSource(undefined) }} open={creatorOpen} sourceConnection={creatorSourceManifest && creatorSource ? { manifest: creatorSourceManifest, handleId: creatorSource.handleId } : undefined} /><div className="flex min-w-0 flex-1 flex-col"><WorkflowFlow ref={flowRef} manifests={manifestMap} onAddTrigger={addFirstTrigger} onDropAction={(manifest, position) => { addActionFromCreator(manifest, position); setCreatorSource(undefined); setCreatorOpen(false); setDetailsOpen(true) }} onDropBinding={(type, role, position) => { addBinding(type, role, position); setDetailsOpen(true) }} onNodeOpen={(nodeId) => { editor.select(nodeId); setDetailsOpen(true) }} onPaneClear={() => setDetailsOpen(false)} onSearchNodes={(source) => { setCreatorSource(source); setCreatorOpen(true) }} runtimeStatuses={runtime.nodeStatuses} /><RuntimePanel events={runtime.events} executionId={executionId} onExecutionChange={setExecutionId} workflowId={workflowId} /></div><NodeInspector data={selected?.data.editorKind === 'action' || selected?.data.editorKind === 'binding' ? selected.data : undefined} executionId={executionId} fieldErrors={Object.fromEntries([...issues, ...(locatedIssue ? [locatedIssue] : [])].filter((issue) => issue.nodeId === selected?.id && issue.fieldPath).map((issue) => [issue.fieldPath!, issue.message]))} manifest={selectedManifest} nodeId={selected?.id} onChange={(patch) => { setLocatedIssue(undefined); if (selected) editor.updateNode(selected.id, patch) }} onClose={() => setDetailsOpen(false)} onDelete={() => { editor.removeSelected(); setDetailsOpen(false) }} onOverlayChange={(nodeId, id) => setOverlayIds((current) => { const next = { ...current }; if (id) next[nodeId] = id; else delete next[nodeId]; return next })} onPrimaryToggle={() => selected && editor.setPrimaryOutput(editor.settings.primaryOutputNodeId === selected.id ? undefined : selected.id)} onRun={() => void executeRun(undefined, {}, 'single_node')} open={detailsOpen} primary={Boolean(selected && editor.settings.primaryOutputNodeId === selected.id)} resources={resources.options} workflowId={workflowId} /></main>
    <ConflictDialog onClose={() => setConflictOpen(false)} onLoad={() => void loadServer()} onOverwrite={() => void overwriteServer()} open={conflictOpen} />
    <RecoveryDialog onDiscard={() => { clearRecovery(workflowId); setRecovery(undefined) }} onRestore={() => { if (recovery) { editor.hydrate(recovery.document); setRevision(recovery.revision) } setRecovery(undefined) }} open={Boolean(recovery)} />
    <IssuesDialog issues={issues} onClose={() => setIssues([])} onLocate={locateIssue} />
    {selected?.data.editorKind === 'action' && (mode === 'single_node' || mode === 'from_node') && <DebugRunDialog executionId={executionId} mode={mode} onClose={() => setDebugDialogOpen(false)} onRun={(source, input) => void executeRun(source, input)} open={debugDialogOpen} running={runtime.running} targetName={selected.data.label} />}
    <PublishDialog deployments={deployments.data ?? []} environment={publishEnvironment} environments={environments.data ?? []} onClose={() => setPublishOpen(false)} onEnvironment={setPublishEnvironment} onPublish={() => void publish()} onRollback={(deployment) => void rollback(deployment)} onVersion={setPublishVersion} open={publishOpen} version={publishVersion} versions={versions.data ?? []} />
    {(() => { const current = serializeStudio(studioDocument(editor)); return <VersionDialog creating={false} definition={current.definition} editorDocument={current.editorDocument} onClose={() => setVersionsOpen(false)} onCreate={() => void createVersion()} open={versionsOpen} versions={versions.data ?? []} /> })()}
  </div>
}

function ConflictDialog({ open, onClose, onLoad, onOverwrite }: { open: boolean; onClose: () => void; onLoad: () => void; onOverwrite: () => void }) { const { t } = useTranslation(); return <Dialog onOpenChange={(value) => !value && onClose()} open={open}><DialogContent description={t('studio.conflict.description')} title={t('studio.conflict.title')}><div className="p-5"><h2 className="text-sm font-semibold">{t('studio.conflict.title')}</h2><p className="mt-2 text-xs leading-5 text-muted-foreground">{t('studio.conflict.description')}</p><div className="mt-5 flex flex-wrap justify-end gap-2"><Button onClick={onClose} variant="ghost">{t('studio.conflict.keep')}</Button><Button onClick={onLoad} variant="secondary">{t('studio.conflict.load')}</Button><Button onClick={onOverwrite}>{t('studio.conflict.overwrite')}</Button></div></div></DialogContent></Dialog> }
function RecoveryDialog({ open, onDiscard, onRestore }: { open: boolean; onDiscard: () => void; onRestore: () => void }) { const { t } = useTranslation(); return <Dialog open={open}><DialogContent description={t('studio.recovery.description')} title={t('studio.recovery.title')}><div className="p-5"><h2 className="text-sm font-semibold">{t('studio.recovery.title')}</h2><p className="mt-2 text-xs text-muted-foreground">{t('studio.recovery.description')}</p><div className="mt-5 flex justify-end gap-2"><Button onClick={onDiscard} variant="ghost">{t('studio.recovery.discard')}</Button><Button onClick={onRestore}>{t('studio.recovery.restore')}</Button></div></div></DialogContent></Dialog> }
function IssuesDialog({ issues, onClose, onLocate }: { issues: StudioValidationIssue[]; onClose: () => void; onLocate: (issue: StudioValidationIssue) => void }) { const { t } = useTranslation(); return <Dialog onOpenChange={(open) => !open && onClose()} open={issues.length > 0}><DialogContent description={t('studio.validation.description')} title={t('studio.validation.title')}><div className="p-5"><div className="flex items-center gap-2"><AlertTriangle className="size-4 text-warning" /><h2 className="text-sm font-semibold">{t('studio.validation.title')}</h2></div><div className="mt-4 max-h-80 divide-y divide-border overflow-auto">{issues.map((issue, index) => <button className="block w-full py-3 text-left text-xs hover:bg-muted/50" disabled={!issue.nodeId} key={`${issue.code}-${index}`} onClick={() => onLocate(issue)} type="button"><strong>{issue.code}</strong><p className="mt-1 text-muted-foreground">{issue.nodeId ?? issue.fieldPath ?? t('studio.validation.workflow')} · {issue.message}</p>{issue.nodeId && <span className="mt-1 block text-[10px] text-primary">{t('studio.validation.locate')}</span>}</button>)}</div><div className="mt-5 flex justify-end"><Button onClick={onClose}>{t('studio.validation.close')}</Button></div></div></DialogContent></Dialog> }
function PublishDialog({ open, environment, version, environments, versions, deployments, onClose, onEnvironment, onVersion, onPublish, onRollback }: { open: boolean; environment: string; version: string; environments: WorkflowEnvironment[]; versions: WorkflowVersion[]; deployments: WorkflowDeployment[]; onClose: () => void; onEnvironment: (id: string) => void; onVersion: (id: string) => void; onPublish: () => void; onRollback: (deployment: WorkflowDeployment) => void }) { const { t } = useTranslation(); return <Dialog onOpenChange={(value) => !value && onClose()} open={open}><DialogContent title={t('studio.publishDialog.title')}><div className="p-5"><h2 className="text-sm font-semibold">{t('studio.publishDialog.title')}</h2><div className="mt-4 space-y-3"><Select aria-label={t('studio.publishDialog.environment')} className="w-full" onValueChange={onEnvironment} options={environments.filter((item) => item.status === 'active').map((item) => ({ value: item.id, label: item.name }))} placeholder={t('studio.publishDialog.environment')} value={environment} /><Select aria-label={t('studio.publishDialog.version')} className="w-full" onValueChange={onVersion} options={versions.map((item) => ({ value: item.id, label: `v${item.versionNumber} · revision ${item.sourceRevision}` }))} placeholder={t('studio.publishDialog.version')} value={version} /></div>{deployments.length > 0 && <div className="mt-5 border-t border-border pt-3"><h3 className="text-xs font-semibold">{t('studio.publishDialog.history')}</h3>{deployments.map((deployment) => <div className="mt-2 flex items-center text-[11px]" key={deployment.id}><span>{deployment.environmentName} · v{deployment.versionNumber} · {deployment.status}</span><span className="flex-1" />{deployment.status !== 'active' && <Button onClick={() => onRollback(deployment)} size="sm" variant="ghost">{t('studio.publishDialog.rollback')}</Button>}</div>)}</div>}<div className="mt-5 flex justify-end gap-2"><Button onClick={onClose} variant="ghost">{t('studio.publishDialog.cancel')}</Button><Button disabled={!environment || !version} onClick={onPublish}>{t('studio.publishDialog.publish')}</Button></div></div></DialogContent></Dialog> }

function defaults(manifest: NodeManifest) { return Object.fromEntries(Object.entries(manifest.parameterSchema.properties ?? {}).flatMap(([name, schema]) => schema.default === undefined ? [] : [[name, structuredClone(schema.default)]])) }
function studioDocument(state: Pick<StudioDocument, 'nodes' | 'edges' | 'viewport' | 'annotations' | 'groups' | 'settings'>): StudioDocument { return { nodes: state.nodes, edges: state.edges, viewport: state.viewport, annotations: state.annotations, groups: state.groups, settings: state.settings } }
function recoveryKey(workflowId: string) { return `agentx:studio:${workflowId}` }
function writeRecovery(workflowId: string, revision: number, document: StudioDocument) { try { localStorage.setItem(recoveryKey(workflowId), JSON.stringify({ revision, savedAt: new Date().toISOString(), document })) } catch { /* Local recovery is best effort. */ } }
function readRecovery(workflowId: string): LocalRecovery | undefined { try { const value = JSON.parse(localStorage.getItem(recoveryKey(workflowId)) ?? 'null') as LocalRecovery | null; return value?.document?.nodes && value?.document?.edges ? value : undefined } catch { return undefined } }
function clearRecovery(workflowId: string) { try { localStorage.removeItem(recoveryKey(workflowId)) } catch { /* Local recovery is best effort. */ } }
