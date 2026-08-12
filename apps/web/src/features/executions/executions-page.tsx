import { useQuery } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { Activity } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { apiRequest } from '../../shared/api/client'
import type { Execution, PageResponse } from '../../shared/api/types'
import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge } from '../../shared/components/status-badge'
import { useLocaleFormat } from '../../shared/lib/locale-format'
import { localizedValue } from '../../shared/lib/localized-value'
import { Button } from '../../shared/ui/button'
import { executionStatus, formatCost } from './execution-format'

export function ExecutionsPage() {
  const { t } = useTranslation()
  const { formatDateTime } = useLocaleFormat()
  const executions = useQuery({ queryKey: ['executions'], queryFn: () => apiRequest<PageResponse<Execution>>('/executions?pageSize=100') })
  const columns = useMemo<Array<ColumnDef<Execution>>>(() => [
    { accessorKey: 'id', header: t('executions.executionIdLabel'), cell: ({ row }) => <EntityCell detail={row.original.id} icon={Activity} name={row.original.workflowName} /> },
    { accessorKey: 'workflowVersionNumber', header: t('common.version'), cell: ({ row }) => `v${row.original.workflowVersionNumber}` },
    { accessorKey: 'triggerType', header: t('executions.trigger'), cell: ({ row }) => localizedValue(t, 'executions.triggerTypes', row.original.triggerType) },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={executionStatus(row.original.status)} /> },
    { accessorKey: 'startedAt', header: t('executions.startedAt'), cell: ({ row }) => formatDateTime(row.original.startedAt) },
    { accessorKey: 'durationMs', header: t('executions.duration'), cell: ({ row }) => row.original.durationMs == null ? '—' : `${row.original.durationMs} ms` },
    { accessorKey: 'costMicros', header: t('executions.cost'), cell: ({ row }) => formatCost(row.original.costMicros) },
    { id: 'actions', header: '', cell: ({ row }) => <Button asChild size="sm" variant="ghost"><Link to={`/executions/${row.original.id}`}>{t('executions.trace')}</Link></Button> },
  ], [formatDateTime, t])
  return <ListPage columns={columns} data={executions.data?.items ?? []} description={t('executions.description')} getSearchText={(row) => `${row.id} ${row.workflowName} ${row.triggerType}`} getStatus={(row) => executionStatus(row.status)} searchPlaceholder={t('executions.search')} statusOptions={['success', 'running', 'waiting', 'failed', 'pending'].map((value) => ({ value, label: t(`common.${value}`) }))} title={t('executions.title')} />
}
