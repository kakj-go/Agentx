import type { ColumnDef } from '@tanstack/react-table'
import { Wrench } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge, type StatusValue } from '../../shared/components/status-badge'

type ToolRow = { name: string; type: string; connection: string; workflows: number; status: StatusValue }

export function ToolsPage() {
  const { t } = useTranslation()
  const data = useMemo<ToolRow[]>(() => [
    { name: t('mocks.tool.order'), type: 'OpenAPI', connection: 'Order Service · Production', workflows: 6, status: 'active' },
    { name: t('mocks.tool.refund'), type: 'HTTP Tool', connection: 'Payment Gateway', workflows: 2, status: 'active' },
    { name: t('mocks.tool.contract'), type: 'CubeSandbox', connection: 'Python Runtime', workflows: 3, status: 'active' },
    { name: t('mocks.tool.crm'), type: 'MCP', connection: 'CRM Server', workflows: 4, status: 'inactive' },
  ], [t])
  const columns = useMemo<Array<ColumnDef<ToolRow>>>(() => [
    { accessorKey: 'name', header: t('table.tool'), cell: ({ row }) => <EntityCell detail={row.original.connection} icon={Wrench} name={row.original.name} /> },
    { accessorKey: 'type', header: t('pages.tools.type') },
    { accessorKey: 'connection', header: t('pages.tools.connection') },
    { accessorKey: 'workflows', header: t('pages.tools.workflows') },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status} /> },
  ], [t])
  return <ListPage actionLabel={t('pages.tools.create')} columns={columns} data={data} description={t('pages.tools.description')} getSearchText={(row) => `${row.name} ${row.type} ${row.connection}`} getStatus={(row) => row.status} searchPlaceholder={t('pages.tools.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'inactive', label: t('common.inactive') }]} title={t('pages.tools.title')} />
}
