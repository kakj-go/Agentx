import type { ColumnDef } from '@tanstack/react-table'
import { Library } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge, type StatusValue } from '../../shared/components/status-badge'

type KnowledgeRow = { name: string; provider: string; scope: string; workflows: number; sync: StatusValue }

export function KnowledgePage() {
  const { t } = useTranslation()
  const data = useMemo<KnowledgeRow[]>(() => [
    { name: t('mocks.knowledge.product'), provider: 'LightRAG · product-main', scope: t('mocks.department.all'), workflows: 8, sync: 'synced' },
    { name: t('mocks.knowledge.policy'), provider: 'LightRAG · service-policy', scope: t('mocks.department.service'), workflows: 3, sync: 'syncing' },
    { name: t('mocks.knowledge.contract'), provider: 'LightRAG · legal-contract', scope: t('mocks.department.legal'), workflows: 2, sync: 'synced' },
  ], [t])
  const columns = useMemo<Array<ColumnDef<KnowledgeRow>>>(() => [
    { accessorKey: 'name', header: t('table.knowledge'), cell: ({ row }) => <EntityCell detail={row.original.provider} icon={Library} name={row.original.name} /> },
    { accessorKey: 'provider', header: t('pages.knowledge.provider') },
    { accessorKey: 'scope', header: t('pages.knowledge.scope') },
    { accessorKey: 'workflows', header: t('pages.knowledge.workflows') },
    { accessorKey: 'sync', header: t('pages.knowledge.sync'), cell: ({ row }) => <StatusBadge status={row.original.sync} /> },
  ], [t])
  return <ListPage actionLabel={t('pages.knowledge.create')} columns={columns} data={data} description={t('pages.knowledge.description')} getSearchText={(row) => `${row.name} ${row.provider} ${row.scope}`} getStatus={(row) => row.sync} searchPlaceholder={t('pages.knowledge.search')} statusOptions={[{ value: 'synced', label: t('common.synced') }, { value: 'syncing', label: t('common.syncing') }]} title={t('pages.knowledge.title')} />
}
