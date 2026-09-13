import { useQuery } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { KeyRound, ShieldCheck } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { apiRequest } from '../../shared/api/client'
import type { GrantableResource, PageResponse } from '../../shared/api/types'
import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { ResourceGrantDialog } from '../../shared/components/resource-grant-panel'
import { StatusBadge } from '../../shared/components/status-badge'
import { useLocaleFormat } from '../../shared/lib/locale-format'
import { localizedValue } from '../../shared/lib/localized-value'
import { Badge } from '../../shared/ui/badge'
import { Button } from '../../shared/ui/button'
import { Tabs, TabsList, TabsTrigger } from '../../shared/ui/tabs'

const resourceTypes = ['credential', 'model', 'mcp_server', 'mcp_tool', 'skill', 'rag', 'memory', 'sandbox_profile'] as const
type ResourceTypeTab = 'all' | (typeof resourceTypes)[number]

export function ResourceGrantsPage() {
  const { t } = useTranslation()
  const { formatDateTime } = useLocaleFormat()
  const [selected, setSelected] = useState<GrantableResource>()
  const [resourceType, setResourceType] = useState<ResourceTypeTab>('all')
  const resources = useQuery({
    queryKey: ['grantable-resources', resourceType],
    queryFn: () => apiRequest<PageResponse<GrantableResource>>(`/resources/grantable?pageSize=100${resourceType === 'all' ? '' : `&resourceType=${resourceType}`}`),
  })
  const columns = useMemo<Array<ColumnDef<GrantableResource>>>(() => [
    { accessorKey: 'name', header: t('common.name'), cell: ({ row }) => <EntityCell detail={row.original.detail} icon={KeyRound} name={row.original.name} /> },
    { accessorKey: 'resourceType', header: t('resourceGrants.resourceType'), cell: ({ row }) => <Badge tone="neutral">{localizedValue(t, 'resourceGrants.resourceTypes', row.original.resourceType)}</Badge> },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge label={localizedValue(t, 'common', row.original.status)} status={row.original.status === 'active' ? 'active' : 'inactive'} /> },
    { accessorKey: 'grantCount', header: t('resourceGrants.grantCount'), cell: ({ row }) => t('resourceGrants.grantCountValue', { count: row.original.grantCount }) },
    { accessorKey: 'updatedAt', header: t('common.updatedAt'), cell: ({ row }) => formatDateTime(row.original.updatedAt) },
    { id: 'actions', header: '', cell: ({ row }) => <Button onClick={() => setSelected(row.original)} size="sm" variant="ghost"><ShieldCheck className="size-3.5" />{t('resourceGrants.manage')}</Button> },
  ], [formatDateTime, t])
  return <>
    <Tabs onValueChange={(value) => setResourceType(value as ResourceTypeTab)} value={resourceType}>
      <ListPage
        columns={columns}
        data={resources.data?.items ?? []}
        description={t('resourceGrants.description')}
        getSearchText={(row) => `${row.name} ${row.detail}`}
        searchPlaceholder={t('resourceGrants.search')}
        tableHeader={<TabsList aria-label={t('resourceGrants.resourceType')}><TabsTrigger value="all">{t('common.all')}</TabsTrigger>{resourceTypes.map((value) => <TabsTrigger key={value} value={value}>{localizedValue(t, 'resourceGrants.resourceTypes', value)}</TabsTrigger>)}</TabsList>}
        title={t('resourceGrants.title')}
      />
    </Tabs>
    <ResourceGrantDialog onClose={() => setSelected(undefined)} open={Boolean(selected)} resource={selected} />
  </>
}
