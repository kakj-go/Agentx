import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { MemoryStick, Plus } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { Credential, Department, ExternalConnection, MemoryResource, PageResponse } from '../../shared/api/types'
import { EntityCell } from '../../shared/components/entity-cell'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { useToast } from '../../shared/ui/toast'

export function MemoryPage() {
  const { t } = useTranslation(); const auth = useAuth(); const queryClient = useQueryClient(); const { showToast } = useToast(); const [open, setOpen] = useState(false)
  const resources = useQuery({ queryKey: ['memory'], queryFn: () => apiRequest<PageResponse<MemoryResource>>('/memory/namespaces?pageSize=100') })
  const departments = useQuery({ queryKey: ['departments'], queryFn: () => apiRequest<Department[]>('/departments') })
  const credentials = useQuery({ queryKey: ['credentials', 'options'], queryFn: () => apiRequest<PageResponse<Credential>>('/credentials?pageSize=100&status=active') })
  const create = useMutation({ mutationFn: async (values: Record<string, string>) => { const connection = await apiRequest<ExternalConnection>('/memory/connections', { method: 'POST', body: jsonBody({ name: values.connectionName, endpoint: values.endpoint, healthPath: values.healthPath, credentialId: values.credential || null, ownerDepartmentId: values.department, configuration: {} }) }); return apiRequest<MemoryResource>('/memory/namespaces', { method: 'POST', body: jsonBody({ connectionId: connection.id, name: values.name, externalNamespace: values.namespace, accessMode: values.accessMode, ownerDepartmentId: values.department }) }) }, onSuccess: async () => { await queryClient.invalidateQueries({ queryKey: ['memory'] }); showToast(t('m2.created')) } })
  const columns = useMemo<Array<ColumnDef<MemoryResource>>>(() => [
    { accessorKey: 'name', header: t('table.memory'), cell: ({ row }) => <EntityCell detail={row.original.connectionName} icon={MemoryStick} name={row.original.name} /> }, { accessorKey: 'externalNamespace', header: t('pages.memory.namespace') }, { accessorKey: 'accessMode', header: t('pages.memory.permission') }, { accessorKey: 'grantCount', header: t('pages.memory.workflows') }, { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status === 'active' ? 'active' : 'inactive'} /> }, { id: 'actions', header: '', cell: ({ row }) => <Button asChild size="sm" variant="ghost"><Link to={`/memory/${row.original.id}`}>{t('m2.details')}</Link></Button> },
  ], [t])
  const fields: EntityFormField[] = [{ name: 'connectionName', label: t('m2.connectionName'), required: true }, { name: 'endpoint', label: t('m2.endpoint'), required: true }, { name: 'healthPath', label: t('m2.healthPath'), defaultValue: '/health', required: true }, { name: 'credential', label: t('m2.credential'), type: 'select', options: [{ value: '', label: t('m2.noCredential') }, ...(credentials.data?.items ?? []).map((item) => ({ value: item.id, label: item.name }))] }, { name: 'department', label: t('m2.department'), type: 'select', required: true, options: (departments.data ?? []).map((item) => ({ value: item.id, label: item.name })) }, { name: 'name', label: t('m2.resourceName'), required: true }, { name: 'namespace', label: t('pages.memory.namespace'), required: true }, { name: 'accessMode', label: t('m2.accessMode'), type: 'select', defaultValue: 'read_write', options: [{ value: 'read', label: t('m2.readOnly') }, { value: 'read_write', label: t('m2.readWrite') }] }]
  return <><ListPage action={auth.hasPermission('memory:manage') ? <Button onClick={() => setOpen(true)}><Plus className="size-4" />{t('m2.createMemory')}</Button> : undefined} columns={columns} data={resources.data?.items ?? []} description={t('pages.memory.description')} getSearchText={(row) => `${row.name} ${row.connectionName} ${row.externalNamespace}`} getStatus={(row) => row.status === 'active' ? 'active' : 'inactive'} searchPlaceholder={t('pages.memory.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'inactive', label: t('common.inactive') }]} title={t('pages.memory.title')} />{open && <EntityFormDialog cancelLabel={t('common.cancel')} fields={fields} onClose={() => setOpen(false)} onSubmit={(values) => create.mutateAsync(values).then(() => undefined)} open submitLabel={t('common.save')} title={t('m2.createMemory')} />}</>
}
