import { useMutation, useQuery } from '@tanstack/react-query'
import { GitFork, GripHorizontal, Pin, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { apiRequest } from '../../../shared/api/client'
import type { Checkpoint, NodeExecution, RuntimeDetails } from '../../../shared/api/types'
import { Button } from '../../../shared/ui/button'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../../../shared/ui/tabs'
import { Textarea } from '../../../shared/ui/textarea'
import { useToast } from '../../../shared/ui/toast'
import type { ExecutionEvent } from '../api/studio-api'
import { deleteOverlay, saveOverlay } from '../api/studio-api'

export function RuntimePanel({ workflowId, executionId, selectedNodeId, events, onOverlayChange, onExecutionChange }: { workflowId: string; executionId?: string; selectedNodeId?: string; events: ExecutionEvent[]; onOverlayChange: (nodeId: string, overlayId?: string) => void; onExecutionChange: (executionId: string) => void }) {
  const { t } = useTranslation()
  const { showToast } = useToast()
  const [overlayText, setOverlayText] = useState('{}')
  const [height, setHeight] = useState(240)
  const nodes = useQuery({ queryKey: ['studio-execution-nodes', executionId], queryFn: () => apiRequest<{ items: NodeExecution[] }>(`/executions/${executionId}/nodes`), enabled: Boolean(executionId), refetchInterval: executionId ? 1000 : false })
  const details = useQuery({ queryKey: ['studio-runtime-details', executionId], queryFn: () => apiRequest<RuntimeDetails>(`/executions/${executionId}/runtime-details`), enabled: Boolean(executionId), refetchInterval: executionId ? 1500 : false })
  const checkpoints = useQuery({ queryKey: ['studio-checkpoints', executionId], queryFn: () => apiRequest<{ items: Checkpoint[] }>(`/executions/${executionId}/checkpoints`), enabled: Boolean(executionId), refetchInterval: executionId ? 1500 : false })
  const selected = nodes.data?.items.filter((node) => node.nodeId === selectedNodeId).at(-1)
  const overlay = useMutation({ mutationFn: (kind: string) => saveOverlay(workflowId, selectedNodeId!, kind, JSON.parse(overlayText)), onSuccess: (value) => { onOverlayChange(value.nodeId, value.id); showToast(t('studio.toasts.overlaySaved')) }, onError: (error: Error) => showToast(error.message) })
  const removeOverlay = useMutation({ mutationFn: () => deleteOverlay(workflowId, selectedNodeId!), onSuccess: () => { onOverlayChange(selectedNodeId!); showToast(t('studio.toasts.overlayRemoved')) }, onError: (error: Error) => showToast(error.message) })
  const fork = useMutation({ mutationFn: (checkpointId: string) => apiRequest<{ executionId: string }>(`/executions/${executionId}/fork`, { method: 'POST', body: JSON.stringify({ checkpointId, mode: 'whole', inputOverrides: {}, sideEffectDecisions: {}, idempotencyKey: crypto.randomUUID() }), headers: { 'Content-Type': 'application/json' } }), onSuccess: (value) => { onExecutionChange(value.executionId); showToast(t('studio.toasts.forkStarted')) }, onError: (error: Error) => showToast(error.message) })
  useEffect(() => { if (selected?.output !== undefined) setOverlayText(JSON.stringify(selected.output, null, 2)) }, [selected?.output])
  const beginResize = (event: React.PointerEvent) => {
    const startY = event.clientY; const startHeight = height
    const move = (next: PointerEvent) => setHeight(Math.max(160, Math.min(window.innerHeight * 0.65, startHeight + startY - next.clientY)))
    const end = () => { window.removeEventListener('pointermove', move); window.removeEventListener('pointerup', end) }
    window.addEventListener('pointermove', move); window.addEventListener('pointerup', end)
  }
  return <section className="relative shrink-0 border-t border-border bg-surface px-4" style={{ height }}><button aria-label={t('studio.runtime.resize')} className="absolute -top-2 left-1/2 z-10 grid h-4 w-10 -translate-x-1/2 cursor-row-resize place-items-center rounded-sm border border-border bg-surface text-muted-foreground" onPointerDown={beginResize} type="button"><GripHorizontal className="size-3" /></button><Tabs className="h-full" defaultValue="output"><TabsList className="h-10 gap-5"><TabsTrigger className="text-xs" value="input">{t('studio.runtime.input')}</TabsTrigger><TabsTrigger className="text-xs" value="output">{t('studio.runtime.output')}</TabsTrigger><TabsTrigger className="text-xs" value="events">{t('studio.runtime.events')}</TabsTrigger><TabsTrigger className="text-xs" value="trace">{t('studio.runtime.trace')}</TabsTrigger><TabsTrigger className="text-xs" value="checkpoints">{t('studio.runtime.checkpoints')}</TabsTrigger><span className="flex-1" />{selectedNodeId && <div className="flex items-center gap-1"><Button disabled={overlay.isPending} onClick={() => overlay.mutate('pin_data')} size="sm" variant="ghost"><Pin className="size-3.5" />{t('studio.runtime.pin')}</Button><Button disabled={overlay.isPending} onClick={() => overlay.mutate('mock_output')} size="sm" variant="ghost">{t('studio.runtime.mock')}</Button><Button aria-label={t('studio.runtime.removeOverlay')} disabled={removeOverlay.isPending} onClick={() => removeOverlay.mutate()} size="icon" variant="ghost"><Trash2 className="size-3.5" /></Button></div>}</TabsList>
      <TabsContent className="h-[calc(100%-40px)] overflow-auto py-3" value="input"><JsonValue value={selected?.input} /></TabsContent><TabsContent className="h-[calc(100%-40px)] overflow-auto py-3" value="output"><Textarea className="h-full resize-none font-mono text-[11px]" onChange={(event) => setOverlayText(event.target.value)} value={overlayText} /></TabsContent>
      <TabsContent className="h-[calc(100%-40px)] overflow-auto py-2" value="events">{events.map((event) => <div className="grid grid-cols-[60px_160px_100px_1fr] gap-3 border-b border-border py-1.5 text-[10px]" key={event.sequence}><span>#{event.sequence}</span><span>{event.eventType}</span><span>{event.status}</span><span className="truncate font-mono text-muted-foreground">{JSON.stringify(event.summary)}</span></div>)}</TabsContent>
      <TabsContent className="h-[calc(100%-40px)] overflow-auto py-3" value="trace"><JsonValue value={details.data ?? {}} /></TabsContent><TabsContent className="h-[calc(100%-40px)] overflow-auto py-2" value="checkpoints">{checkpoints.data?.items.map((checkpoint) => <div className="flex items-center border-b border-border py-2 text-[10px]" key={checkpoint.id}><div><strong>#{checkpoint.sequenceNumber} · {checkpoint.checkpointType}</strong><p className="mt-1 font-mono text-muted-foreground">{checkpoint.stateHash.slice(0, 24)}</p></div><span className="flex-1" /><Button disabled={fork.isPending} onClick={() => fork.mutate(checkpoint.id)} size="sm" variant="ghost"><GitFork className="size-3.5" />{t('studio.runtime.fork')}</Button></div>)}{checkpoints.data?.items.length === 0 && <p className="py-4 text-xs text-muted-foreground">{t('studio.runtime.noCheckpoints')}</p>}</TabsContent></Tabs></section>
}

function JsonValue({ value }: { value: unknown }) { return <pre className="whitespace-pre-wrap break-all font-mono text-[11px] leading-5 text-muted-foreground">{JSON.stringify(value ?? {}, null, 2)}</pre> }
