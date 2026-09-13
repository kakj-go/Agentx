import { AlertTriangle, ChevronDown, ChevronRight, ChevronsDownUp, LoaderCircle, Search } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { TraceSpan } from '../../shared/api/types'
import { useNodeNames } from '../workflow-designer/api/node-display'
import { Badge } from '../../shared/ui/badge'
import { Button } from '../../shared/ui/button'
import { Input } from '../../shared/ui/input'
import { Select } from '../../shared/ui/select'
import { cn } from '../../shared/lib/cn'
import { TraceDetail } from './trace-detail'
import { buildTraceRows, duration, isError, kindColor, spanKinds, statusTone, toggleSet, traceBounds } from './trace-model'
import { useExecutionTrace } from './use-trace'

type Props = {
  executionId: string
  className?: string
  active?: boolean
  onNodeSelect?: (nodeExecutionId: string) => void
  onDownloadArtifact?: (artifactId: string) => void
}

export function TraceWaterfall({ executionId, className, active = true, onNodeSelect, onDownloadArtifact }: Props) {
  const { t } = useTranslation()
  const { resolveSpanName } = useNodeNames()
  const kindLabels = t('trace.kinds', { returnObjects: true }) as Record<string, string>
  const trace = useExecutionTrace(executionId, active)
  const [selectedId, setSelectedId] = useState<string>()
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set())
  const [search, setSearch] = useState('')
  const [kind, setKind] = useState('all')
  const [errorsOnly, setErrorsOnly] = useState(false)
  const [zoom, setZoom] = useState(100)
  const [now, setNow] = useState(Date.now())
  useEffect(() => { const timer = window.setInterval(() => setNow(Date.now()), 1_000); return () => window.clearInterval(timer) }, [])
  useEffect(() => {
    if (selectedId && trace.spans.some((span) => span.spanId === selectedId)) return
    const preferred = trace.spans.find((span) => isError(span.status)) ?? trace.spans.find((span) => span.status === 'running') ?? trace.spans.find((span) => span.spanKind === 'execution') ?? trace.spans[0]
    setSelectedId(preferred?.spanId)
  }, [selectedId, trace.spans])
  const rows = useMemo(() => buildTraceRows(trace.spans, collapsed, search, kind, errorsOnly, (span) => resolveSpanName(span.spanName)), [collapsed, errorsOnly, kind, resolveSpanName, search, trace.spans])
  const selected = trace.spans.find((span) => span.spanId === selectedId)
  const bounds = useMemo(() => traceBounds(trace.spans, now), [now, trace.spans])
  const select = (span: TraceSpan) => { setSelectedId(span.spanId); if (span.nodeExecutionId) onNodeSelect?.(span.nodeExecutionId) }
  if (trace.isLoading) return <div className={cn('grid min-h-64 place-items-center text-xs text-muted-foreground', className)}><span className="flex items-center gap-2"><LoaderCircle className="size-4 animate-spin" />{t('trace.loading')}</span></div>
  if (trace.isError && !trace.spans.length) return <div className={cn('grid min-h-64 place-items-center p-6', className)}><div className="text-center"><AlertTriangle className="mx-auto size-5 text-danger" /><p className="mt-2 text-xs text-danger">{t('trace.unavailable')}</p></div></div>
  return <section className={cn('flex min-h-0 flex-col overflow-hidden bg-surface', className)} data-testid="trace-waterfall">
    {(trace.trace?.warningCode || trace.trace?.degraded) && <div className="flex items-center gap-2 border-b border-warning/25 bg-warning/5 px-4 py-2 text-[10px] text-warning"><AlertTriangle className="size-3.5" />{trace.trace.warningCode === 'TRACE_DELAYED' ? t('trace.delayed') : t('trace.degraded')}</div>}
    <div className="flex flex-wrap items-center gap-2 border-b border-border px-3 py-2">
      <label className="relative min-w-48 flex-1"><Search className="absolute left-2.5 top-2 size-3.5 text-muted-foreground" /><Input aria-label={t('trace.search')} className="h-8 pl-8 text-xs" onChange={(event) => setSearch(event.target.value)} placeholder={t('trace.search')} value={search} /></label>
      <Select aria-label={t('trace.filterKind')} className="h-8 min-w-36 text-xs" onValueChange={setKind} options={[{ value: 'all', label: t('trace.allKinds') }, ...spanKinds.map((value) => ({ value, label: kindLabels[value] ?? value }))]} value={kind} />
      <Button aria-pressed={errorsOnly} onClick={() => setErrorsOnly((value) => !value)} size="sm" variant={errorsOnly ? 'secondary' : 'ghost'}>{t('trace.errorsOnly')}</Button>
      <Button aria-label={t('trace.expandAll')} onClick={() => setCollapsed(new Set())} size="icon" variant="ghost"><ChevronsDownUp className="size-3.5" /></Button>
      <label className="flex items-center gap-2 text-[10px] text-muted-foreground">{t('trace.zoom')}<input aria-label={t('trace.zoom')} className="w-20 accent-primary" max="200" min="50" onChange={(event) => setZoom(Number(event.target.value))} step="10" type="range" value={zoom} /><span className="w-8 tabular-nums">{zoom}%</span></label>
      <span className="text-[10px] text-muted-foreground">{t('trace.loaded', { loaded: trace.spans.length, total: trace.trace?.totalSpans ?? trace.spans.length })}</span>
    </div>
    <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_370px] max-[1120px]:grid-cols-1">
      <div className="min-w-0 overflow-auto" role="treegrid" aria-label={t('trace.tree')}>
        <div className="min-w-[820px]">
          <div className="sticky top-0 z-10 grid h-9 grid-cols-[300px_88px_74px_minmax(350px,1fr)] items-center border-b border-border bg-muted/80 px-2 text-[9px] font-semibold uppercase tracking-wider text-muted-foreground backdrop-blur" role="row"><span role="columnheader">{t('trace.hierarchy')}</span><span role="columnheader">{t('trace.status')}</span><span role="columnheader">{t('trace.duration')}</span><span role="columnheader">{t('trace.timeline')}</span></div>
          {rows.map(({ span, depth }) => <TraceWaterfallRow bounds={bounds} collapsed={collapsed.has(span.spanId)} depth={depth} hasChildren={trace.spans.some((item) => item.parentSpanId === span.spanId)} key={span.spanId} now={now} onSelect={() => select(span)} onToggle={() => setCollapsed((current) => toggleSet(current, span.spanId))} selected={selectedId === span.spanId} span={span} zoom={zoom} />)}          {!rows.length && <p className="p-8 text-center text-xs text-muted-foreground">{t('trace.noMatches')}</p>}
          {trace.hasNextPage && <div className="flex justify-center border-t border-border p-3"><Button disabled={trace.isFetchingNextPage} onClick={() => void trace.fetchNextPage()} size="sm" variant="secondary">{trace.isFetchingNextPage && <LoaderCircle className="size-3.5 animate-spin" />}{t('trace.loadMore')}</Button></div>}
        </div>
      </div>
      <TraceDetail executionId={executionId} onDownloadArtifact={onDownloadArtifact} span={selected} />
    </div>
  </section>
}

function TraceWaterfallRow({ span, depth, selected, collapsed, hasChildren, bounds, now, zoom, onSelect, onToggle }: { span: TraceSpan; depth: number; selected: boolean; collapsed: boolean; hasChildren: boolean; bounds: { start: number; duration: number }; now: number; zoom: number; onSelect: () => void; onToggle: () => void }) {
  const { t } = useTranslation()
  const { resolveSpanName } = useNodeNames()
  const start = new Date(span.startedAt).getTime()
  const spanDuration = span.durationMs ?? Math.max(0, (span.endedAt ? new Date(span.endedAt).getTime() : now) - start)
  const left = ((start - bounds.start) / bounds.duration) * 100
  const width = Math.max(.35, (spanDuration / bounds.duration) * 100)
  return <div aria-expanded={hasChildren ? !collapsed : undefined} aria-level={depth + 1} aria-selected={selected} className={cn('grid h-10 grid-cols-[300px_88px_74px_minmax(350px,1fr)] items-center border-b border-border px-2 text-[10px] outline-none hover:bg-muted/45 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-primary/30', selected && 'bg-primary/5 ring-inset ring-primary/20')} onClick={onSelect} onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); onSelect() } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { event.preventDefault(); moveRowFocus(event.currentTarget, event.key === 'ArrowDown' ? 1 : -1) } else if (event.key === 'ArrowRight' && hasChildren && collapsed) { event.preventDefault(); onToggle() } else if (event.key === 'ArrowLeft' && hasChildren && !collapsed) { event.preventDefault(); onToggle() } }} role="row" tabIndex={selected ? 0 : -1}>
    <div className="flex min-w-0 items-center" role="gridcell"><span style={{ width: depth * 16 }} /><button aria-label={collapsed ? t('trace.expand') : t('trace.collapse')} className="mr-1 grid size-5 shrink-0 place-items-center rounded hover:bg-muted" disabled={!hasChildren} onClick={(event) => { event.stopPropagation(); onToggle() }} type="button">{hasChildren ? collapsed ? <ChevronRight className="size-3" /> : <ChevronDown className="size-3" /> : null}</button><span className={cn('mr-2 size-2 shrink-0 rounded-sm', kindColor[span.spanKind] ?? 'bg-muted-foreground')} /><span className="min-w-0"><strong className="block truncate font-medium">{resolveSpanName(span.spanName)}</strong><small className="block truncate text-[9px] text-muted-foreground">{t(`trace.kinds.${span.spanKind}`)}</small></span></div>
    <div role="gridcell"><Badge className="px-1.5 py-0.5 text-[9px]" tone={statusTone(span.status)}>{t(`common.${span.status}`, { defaultValue: span.status })}</Badge></div>
    <span className="tabular-nums text-muted-foreground" role="gridcell">{duration(spanDuration)}</span>
    <div className="relative h-full overflow-hidden border-l border-border" role="gridcell"><div className="absolute inset-0 origin-left" style={{ width: `${zoom}%`, backgroundImage: 'linear-gradient(to right, transparent calc(25% - 1px), var(--color-border) 25%, transparent calc(25% + 1px), transparent calc(50% - 1px), var(--color-border) 50%, transparent calc(50% + 1px), transparent calc(75% - 1px), var(--color-border) 75%, transparent calc(75% + 1px))' }}><span className={cn('absolute top-3 h-3.5 min-w-[3px] rounded-sm opacity-85', isError(span.status) ? 'bg-danger' : kindColor[span.spanKind] ?? 'bg-primary', !span.endedAt && 'animate-pulse')} style={{ left: `${left}%`, width: `${width}%` }} title={duration(spanDuration)} /></div></div>
  </div>
}

function moveRowFocus(row: HTMLDivElement, delta: number) { const rows = [...(row.parentElement?.querySelectorAll<HTMLDivElement>('[role="row"][aria-level]') ?? [])]; rows[Math.max(0, Math.min(rows.length - 1, rows.indexOf(row) + delta))]?.focus() }
