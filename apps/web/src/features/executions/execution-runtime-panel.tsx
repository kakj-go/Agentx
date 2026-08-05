import { Box, BrainCircuit, Download, Gauge, Wrench } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { RuntimeDetails } from '../../shared/api/types'
import { Badge } from '../../shared/ui/badge'
import { Button } from '../../shared/ui/button'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../../shared/ui/tabs'
import { formatRuntimeTimestamp } from './runtime-format'

type Props = {
  details?: RuntimeDetails
  error?: unknown
  loading: boolean
  onDownloadArtifact: (artifactId: string) => void
}

export function ExecutionRuntimePanel({ details, error, loading, onDownloadArtifact }: Props) {
  const { t } = useTranslation()
  if (loading) return <section className="border-t border-border px-5 py-4 text-xs text-muted-foreground">{t('m5.loadingRuntime')}</section>
  if (error) return <section className="border-t border-border px-5 py-4 text-xs text-danger">{error instanceof Error ? error.message : String(error)}</section>
  if (!details || (!details.agentRuns.length && !details.calls.length && !details.sandboxes.length)) return <section className="border-t border-border px-5 py-4 text-xs text-muted-foreground">{t('m5.noRuntimeDetails')}</section>

  return <section className="border-t border-border bg-surface px-4 py-5 lg:px-6">
    <div className="flex items-center gap-2"><Gauge className="size-4 text-primary" /><h2 className="text-sm font-semibold">{t('m5.runtimeDetails')}</h2></div>
    <dl className="mt-4 grid grid-cols-2 overflow-hidden rounded-lg border border-border sm:grid-cols-4">
      <Metric label={t('m5.agentRuns')} value={details.agentRuns.length} />
      <Metric label={t('m5.inputTokens')} value={details.inputTokens.toLocaleString()} />
      <Metric label={t('m5.outputTokens')} value={details.outputTokens.toLocaleString()} />
      <Metric label={t('m5.cost')} value={formatCost(details.costMicros)} />
    </dl>
    <Tabs className="mt-4" defaultValue="agents">
      <TabsList className="border-b border-border">
        <TabsTrigger value="agents"><BrainCircuit className="mr-2 size-3.5" />{t('m5.agents')} <Count value={details.agentRuns.length} /></TabsTrigger>
        <TabsTrigger value="calls"><Wrench className="mr-2 size-3.5" />{t('m5.runtimeCalls')} <Count value={details.calls.length} /></TabsTrigger>
        <TabsTrigger value="sandboxes"><Box className="mr-2 size-3.5" />{t('m5.sandboxes')} <Count value={details.sandboxes.length} /></TabsTrigger>
      </TabsList>
      <TabsContent className="overflow-x-auto pt-3" value="agents">
        <table className="w-full min-w-[880px] text-left text-xs"><thead className="text-[10px] uppercase text-muted-foreground"><tr><th className="px-3 py-2">{t('common.status')}</th><th className="px-3 py-2">{t('m5.iterations')}</th><th className="px-3 py-2">{t('m5.modelCalls')}</th><th className="px-3 py-2">{t('m5.toolCalls')}</th><th className="px-3 py-2">{t('m5.tokens')}</th><th className="px-3 py-2">{t('m5.cost')}</th><th className="px-3 py-2">{t('m5.stopReason')}</th><th className="px-3 py-2">{t('m5.state')}</th></tr></thead><tbody className="divide-y divide-border">{details.agentRuns.map((run) => <tr key={run.id}><td className="px-3 py-3"><RuntimeBadge status={run.status} /></td><td className="px-3 py-3">{run.iterationCount}</td><td className="px-3 py-3">{run.modelCallCount}</td><td className="px-3 py-3">{run.toolCallCount}</td><td className="px-3 py-3">{(run.inputTokens + run.outputTokens).toLocaleString()}</td><td className="px-3 py-3">{formatCost(run.costMicros)}</td><td className="px-3 py-3">{run.stopReason ?? '—'}</td><td className="px-3 py-3">{run.stateArtifactId ? <ArtifactButton id={run.stateArtifactId} onDownload={onDownloadArtifact} /> : <code className="block max-w-32 truncate text-[10px]" title={run.stateHash ?? undefined}>{run.stateHash ?? '—'}</code>}</td></tr>)}</tbody></table>
        {details.iterations.length > 0 && <div className="mt-5"><h3 className="text-xs font-semibold">{t('m5.iterationLedger')}</h3><div className="mt-2 grid gap-2 md:grid-cols-2 xl:grid-cols-3">{details.iterations.map((iteration) => <div className="grid grid-cols-[auto_1fr_auto] items-center gap-3 border-l-2 border-border px-3 py-2 text-xs" key={iteration.id}><span className="font-mono">#{iteration.iterationIndex}</span><div className="min-w-0"><p className="truncate">{iteration.stopReason ?? iteration.status}</p><p className="truncate font-mono text-[10px] text-muted-foreground" title={iteration.stateAfterHash ?? iteration.stateBeforeHash}>{iteration.stateBeforeHash.slice(0, 12)} → {(iteration.stateAfterHash ?? '—').slice(0, 12)}</p></div>{iteration.stateArtifactId && <ArtifactButton id={iteration.stateArtifactId} onDownload={onDownloadArtifact} />}</div>)}</div></div>}
      </TabsContent>
      <TabsContent className="overflow-x-auto pt-3" value="calls">
        <table className="w-full min-w-[980px] text-left text-xs"><thead className="text-[10px] uppercase text-muted-foreground"><tr><th className="px-3 py-2">#</th><th className="px-3 py-2">{t('m5.kind')}</th><th className="px-3 py-2">{t('common.status')}</th><th className="px-3 py-2">{t('m5.resource')}</th><th className="px-3 py-2">{t('m5.fingerprint')}</th><th className="px-3 py-2">{t('m5.sideEffect')}</th><th className="px-3 py-2">{t('m5.tokens')}</th><th className="px-3 py-2">{t('m5.cost')}</th><th className="px-3 py-2">{t('m5.result')}</th></tr></thead><tbody className="divide-y divide-border">{details.calls.map((call) => <tr key={call.id}><td className="px-3 py-3">{call.iterationIndex}.{call.callIndex}</td><td className="px-3 py-3">{call.callKind}</td><td className="px-3 py-3"><RuntimeBadge status={call.status} /></td><td className="px-3 py-3">{call.resourceType ?? '—'}</td><td className="px-3 py-3"><code className="block max-w-40 truncate text-[10px]" title={call.requestFingerprint}>{call.requestFingerprint}</code></td><td className="px-3 py-3">{call.sideEffect}</td><td className="px-3 py-3">{call.inputTokens + call.outputTokens}{call.usageEstimated ? '*' : ''}</td><td className="px-3 py-3">{formatCost(call.costMicros)}</td><td className="px-3 py-3">{call.responseArtifactId ? <ArtifactButton id={call.responseArtifactId} onDownload={onDownloadArtifact} /> : <span className="text-danger">{call.errorCode ?? '—'}</span>}</td></tr>)}</tbody></table>
      </TabsContent>
      <TabsContent className="overflow-x-auto pt-3" value="sandboxes">
        <table className="w-full min-w-[820px] text-left text-xs"><thead className="text-[10px] uppercase text-muted-foreground"><tr><th className="px-3 py-2">{t('common.status')}</th><th className="px-3 py-2">Sandbox</th><th className="px-3 py-2">{t('m5.profileVersion')}</th><th className="px-3 py-2">{t('m5.expires')}</th><th className="px-3 py-2">{t('m5.terminationAttempts')}</th><th className="px-3 py-2">{t('m5.lastError')}</th></tr></thead><tbody className="divide-y divide-border">{details.sandboxes.map((sandbox) => <tr key={sandbox.id}><td className="px-3 py-3"><RuntimeBadge status={sandbox.status} /></td><td className="px-3 py-3 font-mono text-[10px]">{sandbox.sandboxId ?? sandbox.id}</td><td className="px-3 py-3 font-mono text-[10px]">{sandbox.profileVersionId}</td><td className="px-3 py-3">{formatRuntimeTimestamp(sandbox.expiresAt)}</td><td className="px-3 py-3">{sandbox.terminationAttempts}</td><td className="max-w-64 truncate px-3 py-3 text-danger" title={sandbox.lastError ?? undefined}>{sandbox.lastError ?? '—'}</td></tr>)}</tbody></table>
      </TabsContent>
    </Tabs>
  </section>
}

function Metric({ label, value }: { label: string; value: string | number }) {
  return <div className="border-r border-border p-3 last:border-r-0"><dt className="text-[10px] text-muted-foreground">{label}</dt><dd className="mt-1 text-sm font-semibold">{value}</dd></div>
}

function Count({ value }: { value: number }) { return <span className="ml-1 text-[10px]">{value}</span> }

function RuntimeBadge({ status }: { status: string }) {
  const tone: 'success' | 'danger' | 'primary' | 'neutral' = ['completed', 'succeeded', 'ready', 'terminated'].includes(status) ? 'success' : ['failed', 'orphaned'].includes(status) ? 'danger' : ['running', 'creating'].includes(status) ? 'primary' : 'neutral'
  return <Badge tone={tone}>{status}</Badge>
}

function ArtifactButton({ id, onDownload }: { id: string; onDownload: (id: string) => void }) {
  return <Button aria-label={`Download artifact ${id}`} onClick={() => onDownload(id)} size="icon" title={id} variant="ghost"><Download className="size-3.5" /></Button>
}

function formatCost(micros: number) { return `$${(micros / 1_000_000).toFixed(6)}` }
