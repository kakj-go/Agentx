import type { ColumnDef } from '@tanstack/react-table'
import { KeyRound } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge, type StatusValue } from '../../shared/components/status-badge'

type RoleRow = { name: string; members: number; scope: string; permissions: string; status: StatusValue }

export function RolesPage() {
  const { t } = useTranslation()
  const data = useMemo<RoleRow[]>(() => [
    { name: t('mocks.role.admin'), members: 2, scope: t('mocks.scope.all'), permissions: t('mocks.permission.full'), status: 'active' },
    { name: t('mocks.role.developer'), members: 18, scope: t('mocks.scope.department'), permissions: t('mocks.permission.workspace'), status: 'active' },
    { name: t('mocks.role.reviewer'), members: 6, scope: t('mocks.scope.assigned'), permissions: t('mocks.permission.approval'), status: 'active' },
    { name: t('mocks.role.viewer'), members: 12, scope: t('mocks.scope.assigned'), permissions: t('mocks.permission.audit'), status: 'inactive' },
  ], [t])
  const columns = useMemo<Array<ColumnDef<RoleRow>>>(() => [
    { accessorKey: 'name', header: t('table.role'), cell: ({ row }) => <EntityCell detail={row.original.permissions} icon={KeyRound} name={row.original.name} /> },
    { accessorKey: 'members', header: t('common.members') },
    { accessorKey: 'scope', header: t('pages.roles.dataScope') },
    { accessorKey: 'permissions', header: t('pages.roles.permissions') },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status} /> },
  ], [t])
  return <ListPage actionLabel={t('pages.roles.create')} columns={columns} data={data} description={t('pages.roles.description')} getSearchText={(row) => `${row.name} ${row.scope} ${row.permissions}`} getStatus={(row) => row.status} searchPlaceholder={t('pages.roles.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'inactive', label: t('common.inactive') }]} title={t('pages.roles.title')} />
}
