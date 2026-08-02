import type { ColumnDef } from '@tanstack/react-table'
import { MemoryStick } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge, type StatusValue } from '../../shared/components/status-badge'

type MemoryRow = { name: string; provider: string; namespace: string; permission: string; workflows: number; status: StatusValue }

export function MemoryPage() {
  const { t } = useTranslation()
  const data = useMemo<MemoryRow[]>(() => [
    { name: t('mocks.memory.customer'), provider: 'Mem0 Cloud', namespace: 'customer_sessions', permission: t('mocks.permissionMode.readWrite'), workflows: 4, status: 'active' },
    { name: t('mocks.memory.sales'), provider: 'Mem0 Cloud', namespace: 'sales_leads', permission: t('mocks.permissionMode.readWrite'), workflows: 2, status: 'active' },
    { name: t('mocks.memory.support'), provider: 'Mem0 Self-hosted', namespace: 'support_profiles', permission: t('mocks.permissionMode.readOnly'), workflows: 3, status: 'inactive' },
  ], [t])
  const columns = useMemo<Array<ColumnDef<MemoryRow>>>(() => [
    { accessorKey: 'name', header: t('table.memory'), cell: ({ row }) => <EntityCell detail={row.original.namespace} icon={MemoryStick} name={row.original.name} /> },
    { accessorKey: 'provider', header: t('pages.memory.provider') },
    { accessorKey: 'namespace', header: t('pages.memory.namespace') },
    { accessorKey: 'permission', header: t('pages.memory.permission') },
    { accessorKey: 'workflows', header: t('pages.memory.workflows') },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status} /> },
  ], [t])
  return <ListPage actionLabel={t('pages.memory.create')} columns={columns} data={data} description={t('pages.memory.description')} getSearchText={(row) => `${row.name} ${row.provider} ${row.namespace}`} getStatus={(row) => row.status} searchPlaceholder={t('pages.memory.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'inactive', label: t('common.inactive') }]} title={t('pages.memory.title')} />
}
