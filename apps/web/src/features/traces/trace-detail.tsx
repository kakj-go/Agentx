import { useQuery } from '@tanstack/react-query'
import { Download, LoaderCircle } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { apiRequest } from '../../shared/api/client'
import type { TraceSpan, TraceSpanDetail as TraceSpanDetailValue } from '../../shared/api/types'
import { useLocaleFormat } from '../../shared/lib/locale-format'
import { Badge } from '../../shared/ui/badge'
import { Button } from '../../shared/ui/button'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../../shared/ui/tabs'
import { duration, statusTone } from './trace-model'

type Props = {
  executionId: string
  span?: TraceSpan
  onDownloadArtifact?: (artifactId: string) => void
}

export function TraceDetail({ executionId, span, onDownloadArtifact }: Props) {
  const { t } = useTranslation()
  const [tab, setTab] = useState('overview')
  const detail = useQuery({
    queryKey: ['trace-span-detail', executionId, span?.spanId],
    queryFn: () => apiRequest<TraceSpanDetailValue>(`/executions/${executionId}/trace/spans/${span?.spanId}`),
    enabled: Boolean(span),
    retry: false,
  })
  if (!span) return <aside className="grid min-h-56 place-items-center border-l border-border p-6 text-xs text-muted-foreground max-[1120px]:border-l-0 max-[1120px]:border-t">{t('trace.selectSpan')}</aside>
  const value = detail.data
  return <aside className="min-w-0 border-l border-border bg-surface max-[1120px]:border-l-0 max-[1120px]:border-t" data-testid="trace-detail">
    <div className="border-b border-border px-4 py-3">
      <div className="flex items-center justify-between gap-3"><span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{kindLabel(t, span.spanKind)}</span><Badge tone={statusTone(span.status)}>{statusLabel(t, span.status)}</Badge></div>
      <h3 className="mt-2 truncate text-sm font-semibold">{span.spanName}</h3>
      <p className="mt-1 truncate font-mono text-[9px] text-muted-foreground">{span.spanId}</p>
    </div>
    {detail.isLoading ? <div className="flex min-h-48 items-center justify-center gap-2 text-xs text-muted-foreground"><LoaderCircle className="size-4 animate-spin" />{t('trace.loadingDetail')}</div> :
      <Tabs className="min-h-0" onValueChange={setTab} value={tab}>
        <TabsList className="h-10 gap-4 border-b border-border px-4"><TabsTrigger className="text-[11px]" onClick={() => setTab('overview')} value="overview">{t('trace.overview')}</TabsTrigger><TabsTrigger className="text-[11px]" onClick={() => setTab('input')} value="input">{t('trace.input')}</TabsTrigger><TabsTrigger className="text-[11px]" onClick={() => setTab('output')} value="output">{t('trace.output')}</TabsTrigger><TabsTrigger className="text-[11px]" onClick={() => setTab('events')} value="events">{t('trace.events')}</TabsTrigger><TabsTrigger className="text-[11px]" onClick={() => setTab('raw')} value="raw">{t('trace.raw')}</TabsTrigger></TabsList>
        <TabsContent className="max-h-[430px] overflow-auto p-4" value="overview"><Overview span={span} attributes={value?.attributes} /></TabsContent>
        <TabsContent className="max-h-[430px] overflow-auto p-4" value="input"><ContentView content={value?.input} empty={t('trace.noInput')} onDownload={onDownloadArtifact} /></TabsContent>
        <TabsContent className="max-h-[430px] overflow-auto p-4" value="output"><ContentView content={value?.output} empty={t('trace.noOutput')} onDownload={onDownloadArtifact} /></TabsContent>
        <TabsContent className="max-h-[430px] overflow-auto p-4" value="events"><EventList events={value?.events ?? []} /></TabsContent>
        <TabsContent className="max-h-[430px] overflow-auto p-4" value="raw"><Json value={value ?? span} /></TabsContent>
      </Tabs>}
  </aside>
}

function Overview({ span, attributes }: { span: TraceSpan; attributes?: Record<string, unknown> }) {
  const { t } = useTranslation()
  const { formatDateTime } = useLocaleFormat()
  return <div className="space-y-4"><div className="grid grid-cols-2 overflow-hidden rounded-lg border border-border text-[10px]">{[
    [t('trace.startedAt'), formatDateTime(span.startedAt)], [t('trace.duration'), duration(span.durationMs)],
    [t('trace.tokens'), `${span.inputTokens ?? 0} / ${span.outputTokens ?? 0}`], [t('trace.cost'), span.costMicros ? `$${(span.costMicros / 1_000_000).toFixed(6)}` : '—'],
  ].map(([label, value]) => <div className="border-b border-r border-border p-3 even:border-r-0 last:border-b-0" key={label}><span className="text-muted-foreground">{label}</span><strong className="mt-1 block text-foreground">{value}</strong></div>)}</div>
    {span.errorCode && <div className="rounded-md border border-danger/25 bg-danger/5 p-3 text-xs text-danger"><strong>{span.errorCode}</strong>{span.errorMessage && <p className="mt-1 whitespace-pre-wrap text-foreground">{span.errorMessage}</p>}</div>}
    <div><h4 className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{t('trace.attributes')}</h4><Json value={attributes ?? {}} /></div>
  </div>
}

function ContentView({ content, empty, onDownload }: { content?: TraceSpanDetailValue['input']; empty: string; onDownload?: (id: string) => void }) {
  if (!content) return <p className="text-xs text-muted-foreground">{empty}</p>
  return <div className="space-y-3"><Json value={content.preview ?? { contentRef: content.contentRef }} />{content.contentRef && onDownload && <Button onClick={() => onDownload(content.contentRef!)} size="sm" variant="secondary"><Download className="size-3.5" />{content.contentRef}</Button>}</div>
}

function EventList({ events }: { events: Record<string, unknown>[] }) {
  const { formatTime } = useLocaleFormat()
  if (!events.length) return <p className="text-xs text-muted-foreground">—</p>
  return <ol className="space-y-2">{events.map((event, index) => <li className="rounded-md border border-border p-3 text-[10px]" key={String(event.eventId ?? index)}><div className="flex justify-between gap-3"><strong>{String(event.eventType ?? event.eventKind ?? 'event')}</strong><time className="text-muted-foreground">{typeof event.occurredAt === 'string' ? formatTime(event.occurredAt) : '—'}</time></div><p className="mt-1 text-muted-foreground">{String(event.status ?? '')}</p></li>)}</ol>
}

function Json({ value }: { value: unknown }) { return <pre className="overflow-auto whitespace-pre-wrap break-all rounded-lg border border-border bg-background p-3 font-mono text-[10px] leading-5 text-muted-foreground">{JSON.stringify(value, null, 2)}</pre> }
function statusLabel(t: ReturnType<typeof useTranslation>['t'], status: string) { return t(`common.${status}`, { defaultValue: status }) }
function kindLabel(t: ReturnType<typeof useTranslation>['t'], kind: string) { return t(`trace.kinds.${kind}`, { defaultValue: kind }) }
