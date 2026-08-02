import { ArrowUpRight, Bot, GitBranch, Plus, Wrench } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useOutletContext } from 'react-router-dom'

import { ComingSoonAction } from '../../shared/components/coming-soon-action'
import { EntityCell } from '../../shared/components/entity-cell'
import { MetricCard } from '../../shared/components/metric-card'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { StatusBadge, type StatusValue } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { Table, TableCell, TableContainer, TableHead } from '../../shared/ui/table'

export function DashboardPage() {
  const { t } = useTranslation()
  const { tenantName } = useOutletContext<{ tenantName: string }>()
  const metrics = useMemo(() => [
    { label: t('dashboard.metrics.running'), value: '18', suffix: t('dashboard.metrics.executions'), note: t('dashboard.metrics.realtime') },
    { label: t('dashboard.metrics.today'), value: '1,284', note: '↑ 12.4%' },
    { label: t('dashboard.metrics.successRate'), value: '98.7%', note: '↑ 1.8%' },
    { label: t('dashboard.metrics.cost'), value: '¥ 428.60', note: t('dashboard.metrics.budget'), noteTone: 'muted' as const },
  ], [t])
  const workflows = useMemo<Array<{ name: string; detail: string; status: StatusValue; run: string; rate: string; icon: typeof Bot }>>(() => [
    { name: t('mocks.workflow.customer'), detail: `${t('mocks.workflow.typeAgent')} · 8 nodes · v17`, status: 'published', run: '2 min', rate: '99.2%', icon: Bot },
    { name: t('mocks.workflow.contract'), detail: `${t('mocks.workflow.typeFlow')} · 12 nodes · v8`, status: 'published', run: '18 min', rate: '97.8%', icon: GitBranch },
    { name: t('mocks.workflow.lead'), detail: `${t('mocks.workflow.typeAgent')} · 6 nodes · Draft`, status: 'draft', run: '2026-08-01 16:42', rate: '—', icon: Wrench },
  ], [t])

  return (
    <PageContainer>
      <PageHeader action={<ComingSoonAction><Plus className="size-4" />{t('dashboard.newWorkflow')}</ComingSoonAction>} description={t('dashboard.intro', { tenant: tenantName })} eyebrow={t('dashboard.eyebrow')} title={t('dashboard.greeting')} />
      <section className="mt-7 grid overflow-hidden rounded-xl border border-border bg-surface shadow-sm grid-cols-4">
        {metrics.map((metric) => <MetricCard key={metric.label} {...metric} />)}
      </section>
      <Card className="mt-5 overflow-hidden">
        <div className="flex h-15 items-center justify-between border-b border-border px-5"><h2 className="text-sm font-semibold">{t('dashboard.recent')}</h2><Button asChild size="sm" variant="ghost"><Link to="/workflows">{t('common.viewAll')} <ArrowUpRight className="size-3.5" /></Link></Button></div>
        <TableContainer>
          <Table>
            <thead className="bg-muted/45 text-[10px] uppercase tracking-[0.08em] text-muted-foreground"><tr><TableHead className="w-[44%]">{t('table.workflow')}</TableHead><TableHead>{t('common.status')}</TableHead><TableHead>{t('common.updatedAt')}</TableHead><TableHead>{t('dashboard.metrics.successRate')}</TableHead></tr></thead>
            <tbody>{workflows.map((workflow) => <tr className="border-t border-border first:border-t-0 hover:bg-muted/30" key={workflow.name}><TableCell><EntityCell detail={workflow.detail} icon={workflow.icon} name={workflow.name} /></TableCell><TableCell><StatusBadge status={workflow.status} /></TableCell><TableCell>{workflow.run}</TableCell><TableCell>{workflow.rate}</TableCell></tr>)}</tbody>
          </Table>
        </TableContainer>
      </Card>
    </PageContainer>
  )
}
