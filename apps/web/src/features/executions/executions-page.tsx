import type { ColumnDef } from '@tanstack/react-table'
import { Activity } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge, type StatusValue } from '../../shared/components/status-badge'

type ExecutionRow = { id: string; workflow: string; trigger: string; status: StatusValue; startedAt: string; duration: string; cost: string }

export function ExecutionsPage() {
  const { t } = useTranslation()
  const data = useMemo<ExecutionRow[]>(() => [
    { id: 'EX-8342', workflow: t('mocks.workflow.customer'), trigger: t('mocks.trigger.chat'), status: 'success', startedAt: '2026-08-02 14:32:16', duration: '8.4s', cost: '¥0.84' },
    { id: 'EX-8341', workflow: t('mocks.workflow.contract'), trigger: t('mocks.trigger.webhook'), status: 'waiting', startedAt: '2026-08-02 14:30:02', duration: '2m 14s', cost: '¥1.26' },
    { id: 'EX-8340', workflow: t('mocks.workflow.lead'), trigger: t('mocks.trigger.schedule'), status: 'failed', startedAt: '2026-08-02 14:22:48', duration: '3.1s', cost: '¥0.18' },
    { id: 'EX-8339', workflow: t('mocks.workflow.faq'), trigger: t('mocks.trigger.chat'), status: 'running', startedAt: '2026-08-02 14:20:11', duration: '—', cost: '¥0.42' },
    { id: 'EX-8338', workflow: t('mocks.workflow.invoice'), trigger: t('mocks.trigger.manual'), status: 'success', startedAt: '2026-08-02 14:10:36', duration: '12.7s', cost: '¥0.06' },
    { id: 'EX-8337', workflow: t('mocks.workflow.customer'), trigger: t('mocks.trigger.chat'), status: 'success', startedAt: '2026-08-02 13:58:10', duration: '6.9s', cost: '¥0.72' },
    { id: 'EX-8336', workflow: t('mocks.workflow.contract'), trigger: t('mocks.trigger.webhook'), status: 'success', startedAt: '2026-08-02 13:42:55', duration: '18.2s', cost: '¥1.12' },
    { id: 'EX-8335', workflow: t('mocks.workflow.faq'), trigger: t('mocks.trigger.chat'), status: 'success', startedAt: '2026-08-02 13:38:21', duration: '4.8s', cost: '¥0.34' },
    { id: 'EX-8334', workflow: t('mocks.workflow.lead'), trigger: t('mocks.trigger.schedule'), status: 'failed', startedAt: '2026-08-02 13:30:00', duration: '2.7s', cost: '¥0.12' },
  ], [t])
  const columns = useMemo<Array<ColumnDef<ExecutionRow>>>(() => [
    { accessorKey: 'id', header: 'Execution ID', cell: ({ row }) => <EntityCell detail={row.original.id} icon={Activity} name={row.original.workflow} /> },
    { accessorKey: 'trigger', header: t('pages.executions.trigger') },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status} /> },
    { accessorKey: 'startedAt', header: t('pages.executions.startedAt') },
    { accessorKey: 'duration', header: t('pages.executions.duration') },
    { accessorKey: 'cost', header: t('pages.executions.cost') },
  ], [t])
  return <ListPage columns={columns} data={data} description={t('pages.executions.description')} getSearchText={(row) => `${row.id} ${row.workflow} ${row.trigger}`} getStatus={(row) => row.status} searchPlaceholder={t('pages.executions.search')} statusOptions={['success', 'running', 'waiting', 'failed'].map((value) => ({ value, label: t(`common.${value}`) }))} title={t('pages.executions.title')} />
}
