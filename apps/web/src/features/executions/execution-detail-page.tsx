import { useQuery } from '@tanstack/react-query'
import { Box, Cpu, Database, Download, Wrench } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router-dom'

import { apiRequest, apiRequestBlob } from '../../shared/api/client'
import type { Execution, Trace } from '../../shared/api/types'
import { EmptyState } from '../../shared/components/empty-state'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { useToast } from '../../shared/ui/toast'
import { executionStatus, formatCost } from './execution-format'

export function ExecutionDetailPage() {
  const { id = '' } = useParams()
  const { t } = useTranslation()
  const { showToast } = useToast()
  const execution = useQuery({ queryKey: ['execution', id], queryFn: () => apiRequest<Execution>(`/executions/${id}`) })
  const trace = useQuery({ queryKey: ['execution-trace', id], queryFn: () => apiRequest<Trace>(`/executions/${id}/trace?limit=200`) })
  if (!execution.data) return <PageContainer><EmptyState title={execution.isLoading ? t('m2.loading') : t('m2.loadFailed')} description={String(execution.error ?? '')} /></PageContainer>
  const value = execution.data
  const downloadArtifact = async (artifactId: string) => {
    try {
      const blob = await apiRequestBlob(`/executions/${id}/artifacts/${artifactId}`)
      const url = URL.createObjectURL(blob)
      const anchor = document.createElement('a')
      anchor.href = url
      anchor.download = artifactId
      anchor.click()
      URL.revokeObjectURL(url)
    } catch (error) { showToast(error instanceof Error ? error.message : String(error)) }
  }
  return <PageContainer>
    <PageHeader description={`${value.workflowName} · v${value.workflowVersionNumber}`} title={value.id} />
    <div className="mt-5 flex items-center gap-4"><StatusBadge status={executionStatus(value.status)} /><span className="text-xs text-muted-foreground">{value.triggerType} · {value.durationMs ?? '—'} ms · {formatCost(value.costMicros)}</span></div>
    {value.errorMessage && <div className="mt-5 rounded-lg border border-danger/30 bg-danger/10 p-4 text-xs text-danger">{value.errorCode}: {value.errorMessage}</div>}
    <Card className="mt-6 overflow-hidden">
      <div className="border-b border-border px-5 py-4"><h2 className="text-sm font-semibold">{t('m3.traceEvents')}</h2><p className="mt-1 text-[11px] text-muted-foreground">Trace {value.traceId}</p></div>
      {trace.data?.events.length ? <div className="divide-y divide-border">{trace.data.events.map((event) => <div className="grid grid-cols-[32px_180px_minmax(0,1fr)_140px] items-start gap-3 px-5 py-4" key={event.eventId}>
        <span className="grid size-8 place-items-center rounded-lg bg-muted text-muted-foreground">{event.mcpToolName ? <Wrench className="size-4" /> : event.modelName ? <Cpu className="size-4" /> : event.contentRef ? <Database className="size-4" /> : <Box className="size-4" />}</span>
        <div><p className="text-xs font-medium">{event.eventType}</p><p className="mt-1 text-[10px] text-muted-foreground">{new Date(event.eventTime).toLocaleString()}</p></div>
        <div><p className="text-xs">{event.nodeId ?? event.modelName ?? event.mcpToolName ?? 'Workflow'}</p><p className="mt-1 text-[10px] text-muted-foreground">{event.errorMessage ?? JSON.stringify(event.attributes)}</p>{event.contentRef && <Button className="mt-2" onClick={() => void downloadArtifact(event.contentRef!)} size="sm" variant="secondary"><Download className="size-3.5" />{t('common.download')}</Button>}</div>
        <div className="text-right text-[11px] text-muted-foreground">{event.status}<br />{event.durationMs ?? '—'} ms</div>
      </div>)}</div> : <EmptyState title={t('m3.noTrace')} description={t('m3.noTraceDescription')} />}
    </Card>
  </PageContainer>
}
