import type { ColumnDef } from '@tanstack/react-table'
import { Database } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge, type StatusValue } from '../../shared/components/status-badge'

type DatasetRow = { name: string; version: string; cases: number; owner: string; updatedAt: string; status: StatusValue }

export function DatasetsPage() {
  const { t } = useTranslation()
  const data = useMemo<DatasetRow[]>(() => [
    { name: t('mocks.dataset.customer'), version: 'v12', cases: 248, owner: t('mocks.user.chen'), updatedAt: '2026-08-02 12:04', status: 'active' },
    { name: t('mocks.dataset.contract'), version: 'v6', cases: 86, owner: t('mocks.user.zhou'), updatedAt: '2026-08-01 17:30', status: 'active' },
    { name: t('mocks.dataset.faq'), version: 'v9', cases: 412, owner: t('mocks.user.li'), updatedAt: '2026-07-30 11:16', status: 'inactive' },
  ], [t])
  const columns = useMemo<Array<ColumnDef<DatasetRow>>>(() => [
    { accessorKey: 'name', header: t('common.name'), cell: ({ row }) => <EntityCell detail={`${row.original.version} · ${row.original.cases} cases`} icon={Database} name={row.original.name} /> },
    { accessorKey: 'version', header: t('common.version') },
    { accessorKey: 'cases', header: t('pages.datasets.cases') },
    { accessorKey: 'owner', header: t('common.owner') },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status} /> },
    { accessorKey: 'updatedAt', header: t('common.updatedAt') },
  ], [t])
  return <ListPage actionLabel={t('pages.datasets.create')} columns={columns} data={data} description={t('pages.datasets.description')} getSearchText={(row) => `${row.name} ${row.owner}`} getStatus={(row) => row.status} searchPlaceholder={t('pages.datasets.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'inactive', label: t('common.inactive') }]} title={t('pages.datasets.title')} />
}
