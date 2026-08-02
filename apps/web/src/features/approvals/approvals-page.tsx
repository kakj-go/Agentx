import type { ColumnDef } from '@tanstack/react-table'
import { ShieldCheck } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge, type StatusValue } from '../../shared/components/status-badge'

type ApprovalRow = { title: string; workflow: string; requester: string; status: StatusValue; deadline: string; createdAt: string }

export function ApprovalsPage() {
  const { t } = useTranslation()
  const data = useMemo<ApprovalRow[]>(() => [
    { title: t('mocks.approval.refund'), workflow: t('mocks.workflow.customer'), requester: t('mocks.user.chen'), status: 'pending', deadline: '2026-08-03 14:32', createdAt: '2026-08-02 14:32' },
    { title: t('mocks.approval.contract'), workflow: t('mocks.workflow.contract'), requester: t('mocks.user.zhou'), status: 'pending', deadline: '2026-08-04 10:00', createdAt: '2026-08-02 10:00' },
    { title: t('mocks.approval.discount'), workflow: t('mocks.workflow.lead'), requester: t('mocks.user.wang'), status: 'completed', deadline: '2026-08-02 18:00', createdAt: '2026-08-01 16:20' },
  ], [t])
  const columns = useMemo<Array<ColumnDef<ApprovalRow>>>(() => [
    { accessorKey: 'title', header: t('common.name'), cell: ({ row }) => <EntityCell detail={row.original.workflow} icon={ShieldCheck} name={row.original.title} /> },
    { accessorKey: 'workflow', header: t('pages.approvals.workflow') },
    { accessorKey: 'requester', header: t('pages.approvals.requester') },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status} /> },
    { accessorKey: 'deadline', header: t('pages.approvals.deadline') },
    { accessorKey: 'createdAt', header: t('pages.approvals.createdAt') },
  ], [t])
  return <ListPage columns={columns} data={data} description={t('pages.approvals.description')} getSearchText={(row) => `${row.title} ${row.workflow} ${row.requester}`} getStatus={(row) => row.status} searchPlaceholder={t('pages.approvals.search')} statusOptions={[{ value: 'pending', label: t('common.pending') }, { value: 'completed', label: t('common.completed') }]} title={t('pages.approvals.title')} />
}
