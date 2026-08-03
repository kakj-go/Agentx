import { useQuery } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { ShieldCheck } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { apiRequest } from '../../shared/api/client'
import type { Approval, PageResponse } from '../../shared/api/types'
import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { approvalStatus } from './approval-status'

export function ApprovalsPage() {
  const { t } = useTranslation()
  const approvals = useQuery({ queryKey: ['approvals'], queryFn: () => apiRequest<PageResponse<Approval>>('/approvals?pageSize=100') })
  const columns = useMemo<Array<ColumnDef<Approval>>>(() => [
    { accessorKey: 'title', header: t('common.name'), cell: ({ row }) => <EntityCell detail={row.original.workflowName} icon={ShieldCheck} name={row.original.title} /> },
    { accessorKey: 'workflowName', header: t('pages.approvals.workflow') },
    { accessorKey: 'claimedByName', header: t('m3.assignee'), cell: ({ row }) => row.original.claimedByName ?? '—' },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={approvalStatus(row.original.status)} /> },
    { accessorKey: 'deadlineAt', header: t('pages.approvals.deadline'), cell: ({ row }) => row.original.deadlineAt ? new Date(row.original.deadlineAt).toLocaleString() : '—' },
    { accessorKey: 'createdAt', header: t('pages.approvals.createdAt'), cell: ({ row }) => new Date(row.original.createdAt).toLocaleString() },
    { id: 'actions', header: '', cell: ({ row }) => <Button asChild size="sm" variant="ghost"><Link to={`/approvals/${row.original.id}`}>{t('m3.review')}</Link></Button> },
  ], [t])
  return <ListPage columns={columns} data={approvals.data?.items ?? []} description={t('pages.approvals.description')} getSearchText={(row) => `${row.title} ${row.workflowName} ${row.claimedByName ?? ''}`} getStatus={(row) => approvalStatus(row.status)} searchPlaceholder={t('pages.approvals.search')} statusOptions={[{ value: 'pending', label: t('common.pending') }, { value: 'waiting', label: t('common.waiting') }, { value: 'completed', label: t('common.completed') }, { value: 'inactive', label: t('common.inactive') }]} title={t('pages.approvals.title')} />
}
