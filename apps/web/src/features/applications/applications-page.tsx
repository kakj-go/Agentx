import type { ColumnDef } from '@tanstack/react-table'
import { AppWindow } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge, type StatusValue } from '../../shared/components/status-badge'

type ApplicationRow = { name: string; environment: string; deployment: string; publishedAt: string; status: StatusValue }

export function ApplicationsPage() {
  const { t } = useTranslation()
  const data = useMemo<ApplicationRow[]>(() => [
    { name: t('mocks.app.customer'), environment: t('mocks.environment.production'), deployment: 'Customer Routing · v17', publishedAt: '2026-08-02 13:44', status: 'active' },
    { name: t('mocks.app.contract'), environment: t('mocks.environment.production'), deployment: 'Contract Review · v8', publishedAt: '2026-08-01 10:15', status: 'active' },
    { name: t('mocks.app.sales'), environment: t('mocks.environment.staging'), deployment: 'Lead Scoring · Draft', publishedAt: '2026-07-31 18:22', status: 'inactive' },
    { name: t('mocks.app.faq'), environment: t('mocks.environment.production'), deployment: 'FAQ Agent · v5', publishedAt: '2026-07-29 09:38', status: 'active' },
  ], [t])
  const columns = useMemo<Array<ColumnDef<ApplicationRow>>>(() => [
    { accessorKey: 'name', header: t('table.application'), cell: ({ row }) => <EntityCell detail={row.original.deployment} icon={AppWindow} name={row.original.name} /> },
    { accessorKey: 'environment', header: t('pages.applications.environment') },
    { accessorKey: 'deployment', header: t('pages.applications.deployment') },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status} /> },
    { accessorKey: 'publishedAt', header: t('pages.applications.publishedAt') },
  ], [t])
  return <ListPage actionLabel={t('pages.applications.create')} columns={columns} data={data} description={t('pages.applications.description')} getSearchText={(row) => `${row.name} ${row.deployment} ${row.environment}`} getStatus={(row) => row.status} searchPlaceholder={t('pages.applications.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'inactive', label: t('common.inactive') }]} title={t('pages.applications.title')} />
}
