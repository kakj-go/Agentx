import type { ColumnDef } from '@tanstack/react-table'
import { Sparkles } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge, type StatusValue } from '../../shared/components/status-badge'

type SkillRow = {
  name: string
  source: string
  version: string
  runtime: string
  workflows: number
  status: StatusValue
}

export function SkillsPage() {
  const { t } = useTranslation()
  const data = useMemo<SkillRow[]>(() => [
    { name: t('mocks.skill.support'), source: t('mocks.skillSource.builtIn'), version: 'v2.3.0', runtime: t('mocks.skillRuntime.agent'), workflows: 8, status: 'active' },
    { name: t('mocks.skill.contract'), source: t('mocks.skillSource.git'), version: 'v1.6.2', runtime: t('mocks.skillRuntime.prompt'), workflows: 3, status: 'active' },
    { name: t('mocks.skill.analysis'), source: t('mocks.skillSource.package'), version: 'v0.9.0', runtime: t('mocks.skillRuntime.python'), workflows: 2, status: 'draft' },
    { name: t('mocks.skill.research'), source: t('mocks.skillSource.registry'), version: 'v1.2.1', runtime: t('mocks.skillRuntime.agent'), workflows: 5, status: 'active' },
  ], [t])
  const columns = useMemo<Array<ColumnDef<SkillRow>>>(() => [
    { accessorKey: 'name', header: t('table.skill'), cell: ({ row }) => <EntityCell detail={row.original.source} icon={Sparkles} name={row.original.name} /> },
    { accessorKey: 'source', header: t('pages.skills.source') },
    { accessorKey: 'version', header: t('pages.skills.version') },
    { accessorKey: 'runtime', header: t('pages.skills.runtime') },
    { accessorKey: 'workflows', header: t('pages.skills.workflows') },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status} /> },
  ], [t])

  return <ListPage actionLabel={t('pages.skills.create')} columns={columns} data={data} description={t('pages.skills.description')} getSearchText={(row) => `${row.name} ${row.source} ${row.version} ${row.runtime}`} getStatus={(row) => row.status} searchPlaceholder={t('pages.skills.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'draft', label: t('common.draft') }]} title={t('pages.skills.title')} />
}
