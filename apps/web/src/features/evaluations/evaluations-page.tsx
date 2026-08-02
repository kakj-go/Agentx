import type { ColumnDef } from '@tanstack/react-table'
import { FlaskConical } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge, type StatusValue } from '../../shared/components/status-badge'

type EvaluationRow = { name: string; workflowVersion: string; dataset: string; passRate: string; cost: string; status: StatusValue }

export function EvaluationsPage() {
  const { t } = useTranslation()
  const data = useMemo<EvaluationRow[]>(() => [
    { name: t('mocks.evaluation.customer'), workflowVersion: `${t('mocks.workflow.customer')} · v17`, dataset: `${t('mocks.dataset.customer')} · v12`, passRate: '98.6%', cost: '¥42.80', status: 'completed' },
    { name: t('mocks.evaluation.contract'), workflowVersion: `${t('mocks.workflow.contract')} · v8`, dataset: `${t('mocks.dataset.contract')} · v6`, passRate: '96.8%', cost: '¥28.40', status: 'completed' },
    { name: t('mocks.evaluation.faq'), workflowVersion: `${t('mocks.workflow.faq')} · v5`, dataset: `${t('mocks.dataset.faq')} · v9`, passRate: '—', cost: '¥18.20', status: 'running' },
  ], [t])
  const columns = useMemo<Array<ColumnDef<EvaluationRow>>>(() => [
    { accessorKey: 'name', header: t('table.report'), cell: ({ row }) => <EntityCell detail={row.original.workflowVersion} icon={FlaskConical} name={row.original.name} /> },
    { accessorKey: 'workflowVersion', header: t('pages.evaluations.workflowVersion') },
    { accessorKey: 'dataset', header: t('pages.evaluations.dataset') },
    { accessorKey: 'passRate', header: t('pages.evaluations.passRate') },
    { accessorKey: 'cost', header: t('pages.evaluations.cost') },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status} /> },
  ], [t])
  return <ListPage actionLabel={t('pages.evaluations.create')} columns={columns} data={data} description={t('pages.evaluations.description')} getSearchText={(row) => `${row.name} ${row.workflowVersion} ${row.dataset}`} getStatus={(row) => row.status} searchPlaceholder={t('pages.evaluations.search')} statusOptions={[{ value: 'completed', label: t('common.completed') }, { value: 'running', label: t('common.running') }]} title={t('pages.evaluations.title')} />
}
