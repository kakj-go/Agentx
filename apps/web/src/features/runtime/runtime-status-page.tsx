import { useQuery } from '@tanstack/react-query'
import { Box, Boxes, CircleGauge, ShieldCheck } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { apiRequest } from '../../shared/api/client'
import type { RuntimeStatus } from '../../shared/api/types'
import { MetricCard } from '../../shared/components/metric-card'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { Badge } from '../../shared/ui/badge'
import { Card } from '../../shared/ui/card'

export function RuntimeStatusPage() {
  const { t } = useTranslation()
  const status = useQuery({ queryKey: ['runtime-status'], queryFn: () => apiRequest<RuntimeStatus>('/runtime/status'), refetchInterval: 15_000 })
  return <PageContainer><PageHeader description={t('m3.runtimeDescription')} title={t('m3.runtimeStatus')} /><section className="mt-6 grid grid-cols-2 overflow-hidden rounded-xl border border-border bg-surface lg:grid-cols-4"><MetricCard label={t('dashboard.metrics.running')} value={String(status.data?.running ?? 0)} /><MetricCard label={t('common.waiting')} value={String(status.data?.waiting ?? 0)} /><MetricCard label={t('m3.failedToday')} value={String(status.data?.failedToday ?? 0)} /><MetricCard label={t('m5.activeSandboxes')} value={String(status.data?.activeSandboxes ?? 0)} /></section><Card className="mt-5 overflow-hidden"><div className="border-b border-border px-5 py-4"><h2 className="flex items-center gap-2 text-sm font-semibold"><Boxes className="size-4 text-primary" />{t('m3.runtimeComponents')}</h2></div><div className="divide-y divide-border">{status.data?.components.map((component) => <div className="grid gap-2 px-5 py-4 text-xs sm:grid-cols-[minmax(180px,1fr)_120px_120px_220px] sm:items-center" key={component.component}><span className="flex items-center gap-2 font-medium"><CircleGauge className="size-4 text-muted-foreground" />{component.component}</span><Badge className="w-fit" tone={component.status === 'ready' ? 'success' : component.status === 'unknown' ? 'neutral' : 'warning'}>{component.status}</Badge><span>{component.instances ?? '—'} {t('m3.instances')}</span><span className="text-muted-foreground">{component.lastHeartbeat ? new Date(component.lastHeartbeat).toLocaleString() : t('m3.noHeartbeat')}</span></div>)}</div></Card>{status.data?.sandboxCompatibility !== undefined && <Card className="mt-5 p-5"><div className="flex items-center gap-2"><ShieldCheck className="size-4 text-primary" /><h2 className="text-sm font-semibold">{t('m5.sandboxCompatibility')}</h2><Badge className="ml-auto" tone="primary"><Box className="mr-1 size-3" />OpenSandbox</Badge></div><pre className="mt-4 overflow-auto rounded-md bg-muted p-3 text-[11px] leading-5">{JSON.stringify(status.data.sandboxCompatibility, null, 2)}</pre></Card>}</PageContainer>
}
