import { useQuery } from '@tanstack/react-query'
import { AlertTriangle, LoaderCircle } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { apiRequest } from '../../shared/api/client'
import type { TraceSpan, TraceSpanDetail as TraceSpanDetailValue } from '../../shared/api/types'
import { useNodeNames } from '../workflow-designer/api/node-display'
import { useLocaleFormat } from '../../shared/lib/locale-format'
import { Badge } from '../../shared/ui/badge'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../../shared/ui/tabs'
import { TraceContents, groupContents } from './trace-content-detail'
import { JsonBlock } from './trace-semantic-value'
import { duration, statusTone } from './trace-model'

type Props = {
  executionId: string
  span?: TraceSpan
  onDownloadArtifact?: (artifactId: string) => void
}

export function TraceDetail({ executionId, span, onDownloadArtifact }: Props) {
  const { t } = useTranslation()
  const { resolveSpanName } = useNodeNames()
  const [tab, setTab] = useState('overview')
  const detail = useQuery({
    queryKey: ['trace-span-detail', executionId, span?.spanId],
    queryFn: () => apiRequest<TraceSpanDetailValue>(`/executions/${executionId}/trace/spans/${span?.spanId}`),
    enabled: Boolean(span?.hasDetails),
    retry: false,
  })
  const groups = useMemo(() => groupContents(detail.data?.contents ?? []), [detail.data?.contents])
  const tabs = useMemo(() => ['overview', ...groups.map(([kind]) => `content:${kind}`), ...(detail.data?.events.length ? ['events'] : []), 'raw'], [detail.data?.events.length, groups])
  useEffect(() => { if (!tabs.includes(tab)) setTab('overview') }, [tab, tabs])
  useEffect(() => setTab('overview'), [span?.spanId])
  if (!span) return <aside className="grid min-h-56 place-items-center border-l border-border p-6 text-xs text-muted-foreground max-[1120px]:border-l-0 max-[1120px]:border-t">{t('trace.selectSpan')}</aside>
  return <aside className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden border-l border-border bg-surface max-[1120px]:border-l-0 max-[1120px]:border-t" data-testid="trace-detail">
    <div className="shrink-0 border-b border-border px-4 py-3">
      <div className="flex items-center justify-between gap-3"><span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{kindLabel(t, span.spanKind)}</span><Badge tone={statusTone(span.status)}>{statusLabel(t, span.status)}</Badge></div>
      <h3 className="mt-2 truncate text-sm font-semibold">{resolveSpanName(span.spanName)}</h3>
      <p className="mt-1 truncate font-mono text-[9px] text-muted-foreground">{span.spanId}</p>
    </div>
    {detail.isLoading ? <div className="flex min-h-48 items-center justify-center gap-2 text-xs text-muted-foreground"><LoaderCircle className="size-4 animate-spin" />{t('trace.loadingDetail')}</div> : detail.isError ? <div className="grid min-h-48 place-items-center p-4 text-center text-xs text-warning"><span><AlertTriangle className="mx-auto mb-2 size-4" />{t('trace.diagnosticUnavailable')}</span></div> :
      <Tabs className="flex min-h-0 flex-1 flex-col overflow-hidden" onValueChange={setTab} value={tab}>
        <TabsList className="h-10 shrink-0 justify-start gap-4 overflow-x-auto border-b border-border px-4">
          {tabs.map((value) => <TabsTrigger className="shrink-0 text-[11px]" key={value} onClick={() => setTab(value)} value={value}>{tabLabel(t, value)}</TabsTrigger>)}
        </TabsList>
        <TabsContent className="min-h-0 flex-1 overflow-auto p-4" value="overview"><Overview span={span} /></TabsContent>
        {groups.map(([kind, contents]) => <TabsContent className="min-h-0 flex-1 overflow-auto p-4" key={kind} value={`content:${kind}`}><TraceContents contents={contents} executionId={executionId} onDownloadArtifact={onDownloadArtifact} /></TabsContent>)}
        <TabsContent className="min-h-0 flex-1 overflow-auto p-4" value="events"><EventList events={detail.data?.events ?? []} /></TabsContent>
        <TabsContent className="min-h-0 flex-1 overflow-auto p-4" value="raw"><JsonBlock value={detail.data ?? span} /></TabsContent>
      </Tabs>}
  </aside>
}

function Overview({ span }: { span: TraceSpan }) {
  const { t } = useTranslation()
  const { formatDateTime } = useLocaleFormat()
  return <div className="space-y-4"><div className="grid grid-cols-2 overflow-hidden rounded-lg border border-border text-[10px]">{[
    [t('trace.startedAt'), formatDateTime(span.startedAt)], [t('trace.duration'), duration(span.durationMs)],
    [t('trace.tokens'), `${span.inputTokens ?? 0} / ${span.outputTokens ?? 0}`], [t('trace.cost'), span.costMicros ? `$${(span.costMicros / 1_000_000).toFixed(6)}` : '—'],
  ].map(([label, value]) => <div className="border-b border-r border-border p-3 even:border-r-0 last:border-b-0" key={label}><span className="text-muted-foreground">{label}</span><strong className="mt-1 block text-foreground">{value}</strong></div>)}</div>
    {span.errorCode && <div className="rounded-md border border-danger/25 bg-danger/5 p-3 text-xs text-danger"><strong>{span.errorCode}</strong>{span.errorMessage && <p className="mt-1 whitespace-pre-wrap text-foreground">{span.errorMessage}</p>}</div>}
    {!span.hasDetails && <p className="text-xs text-muted-foreground">{t('trace.lifecycleOnly')}</p>}
  </div>
}

function EventList({ events }: { events: TraceSpanDetailValue['events'] }) {
  const { formatTime } = useLocaleFormat()
  if (!events.length) return <p className="text-xs text-muted-foreground">—</p>
  return <ol className="space-y-2">{events.map((event) => <li className="rounded-md border border-border p-3 text-[10px]" key={event.eventId}><div className="flex justify-between gap-3"><strong>{event.eventType || event.eventKind}</strong><time className="text-muted-foreground">{formatTime(event.occurredAt)}</time></div><p className="mt-1 text-muted-foreground">{event.status}</p>{event.attributes != null && <details className="mt-2"><summary className="cursor-pointer text-muted-foreground">attributes</summary><JsonBlock className="mt-2" value={event.attributes} /></details>}</li>)}</ol>
}

function tabLabel(t: ReturnType<typeof useTranslation>['t'], value: string) { return value.startsWith('content:') ? t(`trace.contentKinds.${value.slice(8)}`) : t(`trace.${value}`) }
function statusLabel(t: ReturnType<typeof useTranslation>['t'], status: string) { return t(`common.${status}`, { defaultValue: status }) }
function kindLabel(t: ReturnType<typeof useTranslation>['t'], kind: string) { return t(`trace.kinds.${kind}`, { defaultValue: kind }) }
