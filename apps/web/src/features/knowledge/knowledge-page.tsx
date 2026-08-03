import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { Library, Plus } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { Credential, Department, ExternalConnection, Knowledge, PageResponse } from '../../shared/api/types'
import { EntityCell } from '../../shared/components/entity-cell'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { useToast } from '../../shared/ui/toast'

export function KnowledgePage() {
  const { t } = useTranslation(); const auth = useAuth(); const queryClient = useQueryClient(); const { showToast } = useToast(); const [open, setOpen] = useState(false)
  const resources = useQuery({ queryKey: ['knowledge'], queryFn: () => apiRequest<PageResponse<Knowledge>>('/knowledge/resources?pageSize=100') })
  const departments = useQuery({ queryKey: ['departments'], queryFn: () => apiRequest<Department[]>('/departments') })
  const credentials = useQuery({ queryKey: ['credentials', 'options'], queryFn: () => apiRequest<PageResponse<Credential>>('/credentials?pageSize=100&status=active') })
  const create = useMutation({ mutationFn: async (values: Record<string, string>) => { const connection = await apiRequest<ExternalConnection>('/knowledge/connections', { method: 'POST', body: jsonBody({ name: values.connectionName, endpoint: values.endpoint, healthPath: values.healthPath, credentialId: values.credential || null, ownerDepartmentId: values.department, configuration: {} }) }); return apiRequest<Knowledge>('/knowledge/resources', { method: 'POST', body: jsonBody({ connectionId: connection.id, name: values.name, externalResourceId: values.externalId, ownerDepartmentId: values.department }) }) }, onSuccess: async () => { await queryClient.invalidateQueries({ queryKey: ['knowledge'] }); showToast(t('m2.created')) } })
  const columns = useMemo<Array<ColumnDef<Knowledge>>>(() => [
    { accessorKey: 'name', header: t('table.knowledge'), cell: ({ row }) => <EntityCell detail={row.original.connectionName} icon={Library} name={row.original.name} /> }, { accessorKey: 'externalResourceId', header: t('m2.externalId') }, { accessorKey: 'grantCount', header: t('pages.knowledge.workflows') }, { accessorKey: 'syncStatus', header: t('pages.knowledge.sync'), cell: ({ row }) => <StatusBadge status={row.original.syncStatus === 'synced' ? 'synced' : row.original.syncStatus === 'syncing' ? 'syncing' : row.original.syncStatus === 'failed' ? 'failed' : 'pending'} /> }, { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status === 'active' ? 'active' : 'inactive'} /> }, { id: 'actions', header: '', cell: ({ row }) => <Button asChild size="sm" variant="ghost"><Link to={`/knowledge/${row.original.id}`}>{t('m2.details')}</Link></Button> },
  ], [t])
  const fields: EntityFormField[] = [{ name: 'connectionName', label: t('m2.connectionName'), required: true }, { name: 'endpoint', label: t('m2.endpoint'), required: true }, { name: 'healthPath', label: t('m2.healthPath'), defaultValue: '/health', required: true }, { name: 'credential', label: t('m2.credential'), type: 'select', options: [{ value: '', label: t('m2.noCredential') }, ...(credentials.data?.items ?? []).map((item) => ({ value: item.id, label: item.name }))] }, { name: 'department', label: t('m2.department'), type: 'select', required: true, options: (departments.data ?? []).map((item) => ({ value: item.id, label: item.name })) }, { name: 'name', label: t('m2.resourceName'), required: true }, { name: 'externalId', label: t('m2.externalId'), required: true }]
  return <><ListPage action={auth.hasPermission('knowledge:manage') ? <Button onClick={() => setOpen(true)}><Plus className="size-4" />{t('m2.createKnowledge')}</Button> : undefined} columns={columns} data={resources.data?.items ?? []} description={t('pages.knowledge.description')} getSearchText={(row) => `${row.name} ${row.connectionName} ${row.externalResourceId}`} getStatus={(row) => row.status === 'active' ? 'active' : 'inactive'} searchPlaceholder={t('pages.knowledge.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'inactive', label: t('common.inactive') }]} title={t('pages.knowledge.title')} />{open && <EntityFormDialog cancelLabel={t('common.cancel')} fields={fields} onClose={() => setOpen(false)} onSubmit={(values) => create.mutateAsync(values).then(() => undefined)} open submitLabel={t('common.save')} title={t('m2.createKnowledge')} />}</>
}
