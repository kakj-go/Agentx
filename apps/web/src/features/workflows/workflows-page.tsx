import type { ColumnDef } from '@tanstack/react-table'
import { Bot, GitBranch } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge, type StatusValue } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'

type WorkflowRow = { id: string; name: string; type: string; version: string; owner: string; updatedAt: string; status: StatusValue; kind: 'agent' | 'flow' }

export function WorkflowsPage() {
  const { t } = useTranslation()
  const data = useMemo<WorkflowRow[]>(() => [
    { id: 'customer-routing', name: t('mocks.workflow.customer'), type: t('mocks.workflow.typeAgent'), version: 'v17', owner: t('mocks.user.lin'), updatedAt: '2026-08-02 14:32', status: 'published', kind: 'agent' },
    { id: 'contract-review', name: t('mocks.workflow.contract'), type: t('mocks.workflow.typeFlow'), version: 'v8', owner: t('mocks.user.zhou'), updatedAt: '2026-08-02 13:18', status: 'published', kind: 'flow' },
    { id: 'lead-scoring', name: t('mocks.workflow.lead'), type: t('mocks.workflow.typeAgent'), version: 'Draft', owner: t('mocks.user.wang'), updatedAt: '2026-08-01 16:42', status: 'draft', kind: 'agent' },
    { id: 'product-faq', name: t('mocks.workflow.faq'), type: t('mocks.workflow.typeAgent'), version: 'v5', owner: t('mocks.user.chen'), updatedAt: '2026-07-31 09:20', status: 'published', kind: 'agent' },
    { id: 'invoice-archive', name: t('mocks.workflow.invoice'), type: t('mocks.workflow.typeFlow'), version: 'Draft', owner: t('mocks.user.li'), updatedAt: '2026-07-30 17:08', status: 'draft', kind: 'flow' },
  ], [t])
  const columns = useMemo<Array<ColumnDef<WorkflowRow>>>(() => [
    { accessorKey: 'name', header: t('table.workflow'), cell: ({ row }) => <EntityCell detail={`${row.original.type} · ${row.original.version}`} icon={row.original.kind === 'agent' ? Bot : GitBranch} name={row.original.name} /> },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status} /> },
    { accessorKey: 'version', header: t('pages.workflows.latestVersion') },
    { accessorKey: 'owner', header: t('common.owner') },
    { accessorKey: 'updatedAt', header: t('common.updatedAt') },
    { id: 'actions', header: '', cell: ({ row }) => <Button asChild size="sm" variant="ghost"><Link to={`/workflows/${row.original.id}/editor`}>{t('common.open')}</Link></Button> },
  ], [t])
  return <ListPage actionLabel={t('pages.workflows.create')} columns={columns} data={data} description={t('pages.workflows.description')} getSearchText={(row) => `${row.name} ${row.type} ${row.owner}`} getStatus={(row) => row.status} searchPlaceholder={t('pages.workflows.search')} statusOptions={[{ value: 'published', label: t('common.published') }, { value: 'draft', label: t('common.draft') }]} title={t('pages.workflows.title')} />
}
