import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Box, Boxes, CircleGauge, Play, Save, ShieldCheck, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { apiRequest, jsonBody } from '../../shared/api/client'
import type { QuotaPolicy, RetentionItem, RetentionRun, RuntimeStatus, WorkerCapability } from '../../shared/api/types'
import { ConfirmDialog } from '../../shared/components/confirm-dialog'
import { MetricCard } from '../../shared/components/metric-card'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { Badge } from '../../shared/ui/badge'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { Input } from '../../shared/ui/input'
import { useToast } from '../../shared/ui/toast'

export function RuntimeStatusPage() {
  const { t } = useTranslation()
  const { showToast } = useToast()
  const queryClient = useQueryClient()
  const [selectedRun, setSelectedRun] = useState<string>()
  const [confirmCleanup, setConfirmCleanup] = useState(false)
  const status = useQuery({ queryKey: ['runtime-status'], queryFn: () => apiRequest<RuntimeStatus>('/runtime/status'), refetchInterval: 15_000 })
  const quotas = useQuery({ queryKey: ['runtime-quotas'], queryFn: () => apiRequest<QuotaPolicy[]>('/runtime/quotas'), refetchInterval: 15_000 })
  const capabilities = useQuery({ queryKey: ['runtime-capabilities'], queryFn: () => apiRequest<WorkerCapability[]>('/runtime/capabilities'), refetchInterval: 15_000 })
  const retention = useQuery({ queryKey: ['retention-runs'], queryFn: () => apiRequest<RetentionRun[]>('/retention-runs'), refetchInterval: (query) => query.state.data?.some((run) => run.status === 'queued' || run.status === 'running') ? 2_000 : false })
  const items = useQuery({ queryKey: ['retention-items', selectedRun], queryFn: () => apiRequest<RetentionItem[]>(`/retention-runs/${selectedRun}/items`), enabled: Boolean(selectedRun), refetchInterval: selectedRun ? 2_000 : false })
  const updateQuota = useMutation({ mutationFn: (policy: QuotaPolicy) => apiRequest<QuotaPolicy[]>('/runtime/quotas', { method: 'PUT', body: jsonBody({ policies: [{ dimension: policy.dimension, hardLimit: policy.hardLimit, periodSeconds: policy.periodSeconds }] }) }), onSuccess: () => queryClient.invalidateQueries({ queryKey: ['runtime-quotas'] }), onError: (error: Error) => showToast(error.message) })
  const createRetention = useMutation({ mutationFn: (dryRun: boolean) => apiRequest<RetentionRun>('/retention-runs', { method: 'POST', body: jsonBody({ dryRun, artifactRetentionDays: 30, traceRetentionDays: 180, messageRetentionDays: 180, evaluationRetentionDays: 365 }) }), onSuccess: async (run) => { setSelectedRun(run.id); setConfirmCleanup(false); await queryClient.invalidateQueries({ queryKey: ['retention-runs'] }) }, onError: (error: Error) => showToast(error.message) })

  return <PageContainer>
    <PageHeader description={t('m3.runtimeDescription')} title={t('m3.runtimeStatus')} />
    <section className="mt-6 grid grid-cols-2 overflow-hidden rounded-lg border border-border bg-surface lg:grid-cols-4"><MetricCard label={t('dashboard.metrics.running')} value={String(status.data?.running ?? 0)} /><MetricCard label={t('common.waiting')} value={String(status.data?.waiting ?? 0)} /><MetricCard label={t('m3.failedToday')} value={String(status.data?.failedToday ?? 0)} /><MetricCard label={t('m5.activeSandboxes')} value={String(status.data?.activeSandboxes ?? 0)} /></section>
    <Card className="mt-5 overflow-hidden"><SectionTitle icon={<Boxes className="size-4 text-primary" />} title={t('m3.runtimeComponents')} /><div className="divide-y divide-border">{status.data?.components.map((component) => <div className="grid gap-2 px-5 py-4 text-xs sm:grid-cols-[minmax(180px,1fr)_120px_120px_220px] sm:items-center" key={component.component}><span className="flex items-center gap-2 font-medium"><CircleGauge className="size-4 text-muted-foreground" />{component.component}</span><Badge className="w-fit" tone={component.status === 'ready' ? 'success' : component.status === 'unknown' ? 'neutral' : 'warning'}>{component.status}</Badge><span>{component.instances ?? '—'} {t('m3.instances')}</span><span className="text-muted-foreground">{component.lastHeartbeat ? new Date(component.lastHeartbeat).toLocaleString() : t('m3.noHeartbeat')}</span></div>)}</div></Card>
    <div className="mt-5 grid gap-5 xl:grid-cols-2">
      <Card className="overflow-hidden"><SectionTitle icon={<CircleGauge className="size-4 text-primary" />} title={t('m7.runtimeQuotas')} /><div className="divide-y divide-border">{quotas.data?.map((policy) => <QuotaRow key={policy.dimension} onSave={(value) => updateQuota.mutate({ ...policy, hardLimit: value })} pending={updateQuota.isPending} policy={policy} />)}</div></Card>
      <Card className="overflow-hidden"><SectionTitle icon={<ShieldCheck className="size-4 text-primary" />} title={t('m7.workerCapabilities')} /><div className="divide-y divide-border">{capabilities.data?.map((capability) => <div className="grid gap-2 px-5 py-3 text-xs sm:grid-cols-[minmax(0,1fr)_110px_120px]" key={`${capability.instanceId}-${capability.capability}`}><span className="truncate"><strong>{capability.instanceId}</strong><span className="mt-1 block text-[10px] text-muted-foreground">{capability.capability} · IR {JSON.stringify(capability.irSchemaVersions)}</span></span><Badge className="w-fit" tone={capability.status === 'ready' ? 'success' : 'warning'}>{capability.status}</Badge><span className="text-muted-foreground">{capability.nodeProtocolVersion}</span></div>)}</div></Card>
    </div>
    <Card className="mt-5 overflow-hidden"><div className="flex items-center border-b border-border px-5 py-4"><h2 className="flex items-center gap-2 text-sm font-semibold"><Trash2 className="size-4 text-primary" />{t('m7.retention')}</h2><div className="ml-auto flex gap-2"><Button disabled={createRetention.isPending} onClick={() => createRetention.mutate(true)} size="sm" variant="secondary"><Play className="size-3.5" />{t('m7.dryRun')}</Button><Button disabled={createRetention.isPending} onClick={() => setConfirmCleanup(true)} size="sm" variant="danger"><Trash2 className="size-3.5" />{t('m7.cleanup')}</Button></div></div><div className="grid min-h-48 lg:grid-cols-[minmax(300px,0.8fr)_minmax(0,1.2fr)]"><div className="border-r border-border">{retention.data?.map((run) => <button className={`grid w-full grid-cols-[1fr_auto] gap-2 border-b border-border px-5 py-3 text-left text-xs hover:bg-muted/50 ${selectedRun === run.id ? 'bg-primary/5' : ''}`} key={run.id} onClick={() => setSelectedRun(run.id)} type="button"><span><strong>{run.dryRun ? t('m7.dryRun') : t('m7.cleanup')}</strong><span className="mt-1 block text-[10px] text-muted-foreground">{new Date(run.createdAt).toLocaleString()} · {run.candidateCount}/{run.deletedCount}</span></span><Badge tone={run.status === 'completed' ? 'success' : run.status === 'failed' ? 'danger' : 'warning'}>{run.status}</Badge></button>)}</div><div className="max-h-80 overflow-auto divide-y divide-border">{items.data?.map((item) => <div className="grid gap-2 px-5 py-3 text-[11px] sm:grid-cols-[150px_minmax(0,1fr)_100px]" key={item.id}><span>{item.dataType}</span><span className="truncate text-muted-foreground" title={item.targetId}>{item.targetId}{item.reason ? ` · ${item.reason}` : ''}</span><Badge className="w-fit" tone={item.status === 'deleted' ? 'success' : item.status === 'blocked' || item.status === 'failed' ? 'danger' : 'neutral'}>{item.status}</Badge></div>)}</div></div></Card>
    {status.data?.sandboxCompatibility !== undefined && <Card className="mt-5 p-5"><div className="flex items-center gap-2"><ShieldCheck className="size-4 text-primary" /><h2 className="text-sm font-semibold">{t('m5.sandboxCompatibility')}</h2><Badge className="ml-auto" tone="primary"><Box className="mr-1 size-3" />OpenSandbox</Badge></div><pre className="mt-4 overflow-auto rounded-md bg-muted p-3 text-[11px] leading-5">{JSON.stringify(status.data.sandboxCompatibility, null, 2)}</pre></Card>}
    <ConfirmDialog cancelLabel={t('common.cancel')} confirmLabel={t('m7.cleanup')} description={t('m7.cleanupConfirmation')} onClose={() => setConfirmCleanup(false)} onConfirm={() => createRetention.mutateAsync(false).then(() => undefined)} open={confirmCleanup} pending={createRetention.isPending} title={t('m7.cleanup')} />
  </PageContainer>
}

function SectionTitle({ icon, title }: { icon: React.ReactNode; title: string }) { return <div className="border-b border-border px-5 py-4"><h2 className="flex items-center gap-2 text-sm font-semibold">{icon}{title}</h2></div> }

function QuotaRow({ policy, pending, onSave }: { policy: QuotaPolicy; pending: boolean; onSave: (value: string) => void }) {
  const [value, setValue] = useState(policy.hardLimit)
  useEffect(() => setValue(policy.hardLimit), [policy.hardLimit])
  return <div className="grid items-center gap-3 px-5 py-3 text-xs sm:grid-cols-[minmax(150px,1fr)_100px_120px_36px]"><span><strong>{policy.dimension}</strong><span className="mt-1 block text-[10px] text-muted-foreground">{policy.periodUsage} used · {policy.activeReserved} reserved</span></span><Input aria-label={policy.dimension} className="h-8" onChange={(event) => setValue(event.target.value)} value={value} /><span className="text-muted-foreground">{policy.periodSeconds ? `${policy.periodSeconds}s` : 'concurrent'}</span><Button aria-label="Save quota" disabled={pending || value === policy.hardLimit} onClick={() => onSave(value)} size="icon" variant="ghost"><Save className="size-3.5" /></Button></div>
}
