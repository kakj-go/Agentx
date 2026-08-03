import { useQuery } from '@tanstack/react-query'
import { ArrowUpRight, Bot, Plus } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Link, useOutletContext } from 'react-router-dom'

import { apiRequest } from '../../shared/api/client'
import type { DashboardSummary, PageResponse, Workflow } from '../../shared/api/types'
import { EmptyState } from '../../shared/components/empty-state'
import { EntityCell } from '../../shared/components/entity-cell'
import { MetricCard } from '../../shared/components/metric-card'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { Table, TableCell, TableContainer, TableHead } from '../../shared/ui/table'

export function DashboardPage() {
  const { t } = useTranslation()
  const { tenantName } = useOutletContext<{ tenantName: string }>()
  const summary = useQuery({ queryKey: ['dashboard-summary'], queryFn: () => apiRequest<DashboardSummary>('/dashboard/summary'), refetchInterval: 30_000 })
  const workflows = useQuery({ queryKey: ['workflows', 'dashboard'], queryFn: () => apiRequest<PageResponse<Workflow>>('/workflows?pageSize=5') })
  const value = summary.data
  const total = (value?.succeededToday ?? 0) + (value?.failedToday ?? 0)
  const rate = total ? `${(((value?.succeededToday ?? 0) / total) * 100).toFixed(1)}%` : '—'
  const metrics = [{ label: t('dashboard.metrics.running'), value: String(value?.runningExecutions ?? 0), suffix: t('dashboard.metrics.executions'), note: t('dashboard.metrics.realtime') }, { label: t('dashboard.metrics.today'), value: String(value?.executionsToday ?? 0), note: `${value?.failedToday ?? 0} ${t('common.failed')}` }, { label: t('dashboard.metrics.successRate'), value: rate, note: `${value?.succeededToday ?? 0} ${t('common.success')}` }, { label: t('dashboard.metrics.cost'), value: `¥ ${((value?.costMicrosToday ?? 0) / 1_000_000).toFixed(4)}`, note: `${value?.pendingApprovals ?? 0} ${t('common.pending')}`, noteTone: 'muted' as const }]
  return <PageContainer><PageHeader action={<Button asChild><Link to="/workflows"><Plus className="size-4" />{t('dashboard.newWorkflow')}</Link></Button>} description={t('dashboard.intro', { tenant: tenantName })} eyebrow={t('dashboard.eyebrow')} title={t('dashboard.greeting')} /><section className="mt-7 grid grid-cols-4 overflow-hidden rounded-xl border border-border bg-surface shadow-sm">{metrics.map((metric) => <MetricCard key={metric.label} {...metric} />)}</section><Card className="mt-5 overflow-hidden"><div className="flex h-15 items-center justify-between border-b border-border px-5"><h2 className="text-sm font-semibold">{t('dashboard.recent')}</h2><Button asChild size="sm" variant="ghost"><Link to="/workflows">{t('common.viewAll')} <ArrowUpRight className="size-3.5" /></Link></Button></div>{workflows.data?.items.length ? <TableContainer><Table><thead className="bg-muted/45 text-[10px] uppercase text-muted-foreground"><tr><TableHead className="w-[52%]">{t('table.workflow')}</TableHead><TableHead>{t('common.status')}</TableHead><TableHead>{t('common.version')}</TableHead><TableHead>{t('common.updatedAt')}</TableHead></tr></thead><tbody>{workflows.data.items.map((workflow) => <tr className="border-t border-border first:border-0 hover:bg-muted/30" key={workflow.id}><TableCell><EntityCell detail={workflow.description ?? `Revision ${workflow.draftRevision}`} icon={Bot} name={workflow.name} /></TableCell><TableCell><StatusBadge status={workflow.status === 'active' ? 'active' : 'inactive'} /></TableCell><TableCell>{workflow.latestVersion ? `v${workflow.latestVersion}` : '—'}</TableCell><TableCell>{new Date(workflow.updatedAt).toLocaleString()}</TableCell></tr>)}</tbody></Table></TableContainer> : <EmptyState title={t('m3.noWorkflows')} description={t('m3.noWorkflowsDescription')} />}</Card></PageContainer>
}
