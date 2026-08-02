import type { ColumnDef } from '@tanstack/react-table'
import { BrainCircuit } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge, type StatusValue } from '../../shared/components/status-badge'

type ModelRow = { name: string; provider: string; alias: string; model: string; departments: string; status: StatusValue }

export function ModelsPage() {
  const { t } = useTranslation()
  const data = useMemo<ModelRow[]>(() => [
    { name: t('mocks.model.gpt'), provider: 'OpenAI', alias: 'production-chat', model: 'GPT-5', departments: `${t('mocks.department.ai')}, ${t('mocks.department.service')}`, status: 'active' },
    { name: t('mocks.model.claude'), provider: 'Anthropic', alias: 'contract-review', model: 'Claude Sonnet', departments: `${t('mocks.department.ai')}, ${t('mocks.department.legal')}`, status: 'active' },
    { name: t('mocks.model.qwen'), provider: 'Alibaba Cloud', alias: 'internal-qwen', model: 'Qwen Max', departments: t('mocks.department.ai'), status: 'inactive' },
  ], [t])
  const columns = useMemo<Array<ColumnDef<ModelRow>>>(() => [
    { accessorKey: 'name', header: t('common.name'), cell: ({ row }) => <EntityCell detail={`${row.original.provider} · ${row.original.alias}`} icon={BrainCircuit} name={row.original.name} /> },
    { accessorKey: 'provider', header: t('pages.models.provider') },
    { accessorKey: 'alias', header: t('pages.models.alias') },
    { accessorKey: 'model', header: t('pages.models.model') },
    { accessorKey: 'departments', header: t('pages.models.departments') },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status} /> },
  ], [t])
  return <ListPage actionLabel={t('pages.models.create')} columns={columns} data={data} description={t('pages.models.description')} getSearchText={(row) => `${row.name} ${row.provider} ${row.alias} ${row.model}`} getStatus={(row) => row.status} searchPlaceholder={t('pages.models.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'inactive', label: t('common.inactive') }]} title={t('pages.models.title')} />
}
