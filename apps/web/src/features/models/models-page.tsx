import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { BrainCircuit, Plus } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { Credential, Department, Model, ModelDeployment, ModelProvider, PageResponse } from '../../shared/api/types'
import { EntityCell } from '../../shared/components/entity-cell'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { useToast } from '../../shared/ui/toast'

export function ModelsPage() {
  const { t } = useTranslation()
  const auth = useAuth()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const [open, setOpen] = useState(false)
  const models = useQuery({ queryKey: ['models'], queryFn: () => apiRequest<PageResponse<Model>>('/models/aliases?pageSize=100') })
  const departments = useQuery({ queryKey: ['departments'], queryFn: () => apiRequest<Department[]>('/departments') })
  const credentials = useQuery({ queryKey: ['credentials', 'options'], queryFn: () => apiRequest<PageResponse<Credential>>('/credentials?pageSize=100&status=active') })
  const create = useMutation({
    mutationFn: async (values: Record<string, string>) => {
      let defaults: unknown = {}
      try { defaults = values.parameters ? JSON.parse(values.parameters) as unknown : {} } catch { throw new Error(t('m2.invalidJson')) }
      const provider = await apiRequest<ModelProvider>('/models/providers', { method: 'POST', body: jsonBody({ name: values.providerName, providerType: values.providerType, endpoint: values.endpoint, credentialId: values.credential || null, ownerDepartmentId: values.department }) })
      const deployment = await apiRequest<ModelDeployment>('/models/deployments', { method: 'POST', body: jsonBody({ providerId: provider.id, name: values.deploymentName, modelName: values.modelName, endpointOverride: null, credentialId: values.credential || null, defaultParameters: defaults }) })
      return apiRequest<Model>('/models/aliases', { method: 'POST', body: jsonBody({ alias: values.alias, deploymentId: deployment.id }) })
    },
    onSuccess: async () => { await queryClient.invalidateQueries({ queryKey: ['models'] }); showToast(t('m2.created')) },
  })
  const columns = useMemo<Array<ColumnDef<Model>>>(() => [
    { accessorKey: 'alias', header: t('pages.models.alias'), cell: ({ row }) => <EntityCell detail={`${row.original.providerName} · ${row.original.deploymentName}`} icon={BrainCircuit} name={row.original.alias} /> },
    { accessorKey: 'providerType', header: t('pages.models.provider') },
    { accessorKey: 'modelName', header: t('pages.models.model') },
    { accessorKey: 'connectionStatus', header: t('models.connectionStatus'), cell: ({ row }) => <StatusBadge status={connectionStatus(row.original.connectionStatus)} /> },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status === 'active' ? 'active' : 'inactive'} /> },
    { accessorKey: 'updatedAt', header: t('common.updatedAt'), cell: ({ row }) => new Date(row.original.updatedAt).toLocaleString() },
    { id: 'actions', header: '', cell: ({ row }) => <Button asChild size="sm" variant="ghost"><Link to={`/models/${row.original.id}`}>{t('m2.details')}</Link></Button> },
  ], [t])
  const fields: EntityFormField[] = [
    { name: 'providerName', label: t('m2.providerName'), required: true },
    { name: 'providerType', label: t('m2.providerType'), type: 'select', defaultValue: 'openai_compatible', options: [{ value: 'openai_compatible', label: t('m2.openaiCompatible') }, { value: 'custom_http', label: t('m2.customHttp') }] },
    { name: 'endpoint', label: t('m2.endpoint'), required: true, placeholder: 'https://api.example.com/v1' },
    { name: 'credential', label: t('m2.credential'), type: 'select', options: [{ value: '', label: t('m2.noCredential') }, ...(credentials.data?.items ?? []).map((item) => ({ value: item.id, label: item.name }))] },
    { name: 'department', label: t('m2.department'), type: 'select', required: true, options: (departments.data ?? []).map((item) => ({ value: item.id, label: item.name })) },
    { name: 'deploymentName', label: t('m2.deploymentName'), required: true },
    { name: 'modelName', label: t('m2.modelName'), required: true },
    { name: 'alias', label: t('m2.alias'), required: true },
    { name: 'parameters', label: t('m2.defaultParameters'), type: 'textarea', defaultValue: '{}' },
  ]
  return <>
    <ListPage action={auth.hasPermission('model:manage') ? <Button onClick={() => setOpen(true)}><Plus className="size-4" />{t('m2.createModel')}</Button> : undefined} columns={columns} data={models.data?.items ?? []} description={t('pages.models.description')} getSearchText={(row) => `${row.alias} ${row.providerName} ${row.modelName}`} getStatus={(row) => row.status === 'active' ? 'active' : 'inactive'} searchPlaceholder={t('pages.models.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'inactive', label: t('common.inactive') }]} title={t('pages.models.title')} />
    {open && <EntityFormDialog cancelLabel={t('common.cancel')} fields={fields} onClose={() => setOpen(false)} onSubmit={(values) => create.mutateAsync(values).then(() => undefined)} open submitLabel={t('common.save')} title={t('m2.createModel')} />}
  </>
}

function connectionStatus(value: string): 'untested' | 'healthy' | 'unhealthy' {
  return value === 'healthy' || value === 'unhealthy' ? value : 'untested'
}
