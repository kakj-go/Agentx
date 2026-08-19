import { useMutation, useQuery } from '@tanstack/react-query'
import { ChevronDown, ChevronUp, GitFork, GripHorizontal } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { apiRequest } from '../../../shared/api/client'
import type { Checkpoint, NodeExecution } from '../../../shared/api/types'
import { useLocaleFormat } from '../../../shared/lib/locale-format'
import { localizedValue } from '../../../shared/lib/localized-value'
import { Button } from '../../../shared/ui/button'
import { Select } from '../../../shared/ui/select'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../../../shared/ui/tabs'
import { useToast } from '../../../shared/ui/toast'
import type { Execution, PageResponse } from '../../../shared/api/types'
import { TraceWaterfall } from '../../traces/trace-waterfall'
import type { ExecutionEvent } from '../api/studio-api'

export function RuntimePanel({ workflowId, executionId, events, onExecutionChange, onNodeSelect }: { workflowId: string; executionId?: string; events: ExecutionEvent[]; onExecutionChange: (executionId: string) => void; onNodeSelect?: (nodeId: string) => void }) {
  const { t } = useTranslation()
  const { formatDateTime } = useLocaleFormat()
  const { showToast } = useToast()
  const [height, setHeight] = useState(220)
  const [collapsed, setCollapsed] = useState(!executionId)
  const [tab, setTab] = useState('events')
  const executions = useQuery({ queryKey: ['studio-executions', workflowId], queryFn: () => apiRequest<PageResponse<Execution>>('/executions?pageSize=100'), enabled: Boolean(workflowId), refetchInterval: executionId ? 3000 : false })
  const nodes = useQuery({ queryKey: ['studio-trace-nodes', executionId], queryFn: () => apiRequest<{ items: NodeExecution[] }>(`/executions/${executionId}/nodes`), enabled: Boolean(executionId), refetchInterval: executionId ? 2000 : false })
  const checkpoints = useQuery({ queryKey: ['studio-checkpoints', executionId], queryFn: () => apiRequest<{ items: Checkpoint[] }>(`/executions/${executionId}/checkpoints`), enabled: Boolean(executionId), refetchInterval: executionId ? 1500 : false })
  const fork = useMutation({ mutationFn: (checkpointId: string) => apiRequest<{ executionId: string }>(`/executions/${executionId}/fork`, { method: 'POST', body: JSON.stringify({ checkpointId, mode: 'whole', inputOverrides: {}, sideEffectDecisions: {}, idempotencyKey: crypto.randomUUID() }), headers: { 'Content-Type': 'application/json' } }), onSuccess: (value) => { onExecutionChange(value.executionId); showToast(t('studio.toasts.forkStarted')) }, onError: (error: Error) => showToast(error.message) })
  useEffect(() => { if (executionId) setCollapsed(false) }, [executionId])
  const executionOptions = (executions.data?.items ?? []).filter((execution) => execution.workflowId === workflowId).map((execution) => ({ value: execution.id, label: `${localizedValue(t, 'common', execution.status)} · ${formatDateTime(execution.startedAt)}` }))
  if (executionId && !executionOptions.some((option) => option.value === executionId)) executionOptions.unshift({ value: executionId, label: executionId })
  const executionSelect = <Select aria-label={t('studio.runtime.selectExecution')} className="h-7 min-w-56 max-w-80 text-[10px]" disabled={!executionOptions.length} onValueChange={onExecutionChange} options={executionOptions} placeholder={t('studio.runtime.noExecution')} value={executionId ?? ''} />
  const beginResize = (event: React.PointerEvent) => {
    const startY = event.clientY; const startHeight = height
    const move = (next: PointerEvent) => setHeight(Math.max(160, Math.min(window.innerHeight * 0.65, startHeight + startY - next.clientY)))
    const end = () => { window.removeEventListener('pointermove', move); window.removeEventListener('pointerup', end) }
    window.addEventListener('pointermove', move); window.addEventListener('pointerup', end)
  }
  if (collapsed) return <section className="relative flex h-10 shrink-0 items-center gap-3 border-t border-border bg-surface px-4" data-testid="runtime-rail"><button aria-label={t('studio.runtime.expand')} className="flex items-center gap-2 text-xs text-muted-foreground hover:text-foreground" onClick={() => setCollapsed(false)} type="button"><ChevronUp className="size-4" />{t('studio.runtime.title')}</button>{executionSelect}</section>
  const selectTraceNode = (nodeExecutionId: string) => { const node = nodes.data?.items.find((item) => item.id === nodeExecutionId); if (node) onNodeSelect?.(node.nodeId) }
  const openTrace = () => setHeight((current) => Math.max(current, 420))
  return <section className="relative shrink-0 border-t border-border bg-surface px-4" data-testid="runtime-rail" style={{ height }}><button aria-label={t('studio.runtime.resize')} className="absolute -top-2 left-1/2 z-10 grid h-4 w-10 -translate-x-1/2 cursor-row-resize place-items-center rounded-sm border border-border bg-surface text-muted-foreground" onPointerDown={beginResize} type="button"><GripHorizontal className="size-3" /></button><Tabs className="h-full" onValueChange={(value) => { setTab(value); if (value === 'trace') openTrace() }} value={tab}><TabsList className="h-10 gap-5"><TabsTrigger className="text-xs" value="events">{t('studio.runtime.events')}</TabsTrigger><TabsTrigger className="text-xs" onClick={openTrace} value="trace">{t('studio.runtime.trace')}</TabsTrigger><TabsTrigger className="text-xs" value="checkpoints">{t('studio.runtime.checkpoints')}</TabsTrigger><span className="flex-1" />{executionSelect}<Button aria-label={t('studio.runtime.collapse')} onClick={() => setCollapsed(true)} size="icon" variant="ghost"><ChevronDown className="size-3.5" /></Button></TabsList>
      <TabsContent className="h-[calc(100%-40px)] overflow-auto py-3" value="events">{events.map((event) => <div className="grid grid-cols-[60px_160px_100px_1fr] gap-3 border-b border-border py-1.5 text-[10px]" key={event.sequence}><span>#{event.sequence}</span><span>{event.eventType}</span><span>{localizedValue(t, 'common', event.status)}</span><span className="truncate font-mono text-muted-foreground">{JSON.stringify(event.summary)}</span></div>)}</TabsContent>
      <TabsContent className="h-[calc(100%-40px)] overflow-hidden" value="trace">{executionId ? <TraceWaterfall active={tab === 'trace'} className="h-full" executionId={executionId} onNodeSelect={selectTraceNode} /> : <p className="p-4 text-xs text-muted-foreground">{t('studio.runtime.noExecution')}</p>}</TabsContent><TabsContent className="h-[calc(100%-40px)] overflow-auto py-2" value="checkpoints">{checkpoints.data?.items.map((checkpoint) => <div className="flex items-center border-b border-border py-2 text-[10px]" key={checkpoint.id}><div><strong>#{checkpoint.sequenceNumber} · {checkpoint.checkpointType}</strong><p className="mt-1 font-mono text-muted-foreground">{checkpoint.stateHash.slice(0, 24)}</p></div><span className="flex-1" /><Button disabled={fork.isPending} onClick={() => fork.mutate(checkpoint.id)} size="sm" variant="ghost"><GitFork className="size-3.5" />{t('studio.runtime.fork')}</Button></div>)}{checkpoints.data?.items.length === 0 && <p className="py-4 text-xs text-muted-foreground">{t('studio.runtime.noCheckpoints')}</p>}</TabsContent></Tabs></section>
}
