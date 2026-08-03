import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Activity, History, Pencil, Plus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { Credential, HealthCheck, Model, ModelDeployment, ModelDeploymentHistory, ModelPrice, ModelProvider, PageResponse } from '../../shared/api/types'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { ResourceDetailLayout } from '../../shared/components/resource-detail-layout'
import { PrerequisiteAction } from '../../shared/components/prerequisite-action'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { useToast } from '../../shared/ui/toast'

export function ModelDetailPage() {
  const { id = '' } = useParams()
  const { t } = useTranslation()
  const auth = useAuth()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const [priceOpen, setPriceOpen] = useState(false)
  const [editOpen, setEditOpen] = useState(false)
  const model = useQuery({ queryKey: ['model', id], queryFn: () => apiRequest<Model>(`/models/aliases/${id}`) })
  const providers = useQuery({ queryKey: ['model-providers'], queryFn: () => apiRequest<ModelProvider[]>('/models/providers') })
  const deployments = useQuery({ queryKey: ['model-deployments', model.data?.providerId], enabled: Boolean(model.data), queryFn: () => apiRequest<ModelDeployment[]>(`/models/deployments?providerId=${model.data?.providerId}`) })
  const prices = useQuery({ queryKey: ['model-prices', model.data?.deploymentId], enabled: Boolean(model.data), queryFn: () => apiRequest<ModelPrice[]>(`/models/deployments/${model.data?.deploymentId}/prices`) })
  const history = useQuery({ queryKey: ['model-history', id], queryFn: () => apiRequest<ModelDeploymentHistory[]>(`/models/aliases/${id}/deployment-history`) })
  const credentials = useQuery({ queryKey: ['credentials', 'model-options'], queryFn: () => apiRequest<PageResponse<Credential>>('/credentials?pageSize=100&status=active') })
  const test = useMutation({
    mutationFn: () => apiRequest<HealthCheck>(`/models/aliases/${id}/test-connection`, { method: 'POST' }),
    onSuccess: async (result) => { await invalidate(); showToast(t(`m2.${result.status}`)) },
    onError: (error: Error) => showToast(error.message),
  })

  const invalidate = async () => Promise.all([
    queryClient.invalidateQueries({ queryKey: ['model', id] }),
    queryClient.invalidateQueries({ queryKey: ['models'] }),
    queryClient.invalidateQueries({ queryKey: ['model-providers'] }),
    queryClient.invalidateQueries({ queryKey: ['model-deployments'] }),
    queryClient.invalidateQueries({ queryKey: ['model-history', id] }),
    queryClient.invalidateQueries({ queryKey: ['model-prices'] }),
  ])
  const createPrice = async (values: Record<string, string>) => {
    await apiRequest(`/models/deployments/${model.data?.deploymentId}/prices`, { method: 'POST', body: jsonBody({ currency: values.currency, inputPerMillion: values.input, outputPerMillion: values.output }) })
    await invalidate()
  }
  const editModel = async (values: Record<string, string>) => {
    if (!model.data || !provider) return
    let parameters: unknown
    try { parameters = JSON.parse(values.parameters) as unknown } catch { throw new Error(t('m2.invalidJson')) }
    await apiRequest<ModelProvider>(`/models/providers/${provider.id}`, { method: 'PATCH', body: jsonBody({ name: values.providerName, endpoint: values.endpoint, credentialId: values.credential || null, status: values.providerStatus, version: provider.version }) })
    const alias = await apiRequest<Model>(`/models/aliases/${id}`, { method: 'PATCH', body: jsonBody({ alias: values.alias, status: values.aliasStatus, version: model.data.aliasVersion }) })
    const price = values.input && values.output ? { currency: values.currency || 'USD', inputPerMillion: values.input, outputPerMillion: values.output } : null
    await apiRequest<ModelDeployment>(`/models/aliases/${id}/deployment-revisions`, { method: 'POST', body: jsonBody({ providerId: provider.id, name: values.deploymentName, modelName: values.modelName, endpointOverride: values.endpointOverride || null, credentialId: values.credential || null, defaultParameters: parameters, expectedAliasVersion: alias.aliasVersion, price }) })
    await invalidate()
    showToast(t('m2.saved'))
  }

  const value = model.data
  const provider = providers.data?.find((item) => item.id === value?.providerId)
  const deployment = deployments.data?.find((item) => item.id === value?.deploymentId)
  const priceFields: EntityFormField[] = [
    { name: 'currency', label: t('m2.currency'), defaultValue: 'USD', required: true },
    { name: 'input', label: t('m2.inputPrice'), type: 'number', required: true, step: 'any' },
    { name: 'output', label: t('m2.outputPrice'), type: 'number', required: true, step: 'any' },
  ]
  const editFields: EntityFormField[] = value && provider && deployment ? [
    { name: 'alias', label: t('m2.alias'), defaultValue: value.alias, required: true },
    { name: 'aliasStatus', label: t('m2.aliasStatus'), type: 'select', defaultValue: value.status, options: statusOptions(t) },
    { name: 'providerName', label: t('m2.providerName'), defaultValue: provider.name, required: true },
    { name: 'providerStatus', label: t('m2.providerStatus'), type: 'select', defaultValue: provider.status, options: statusOptions(t) },
    { name: 'endpoint', label: t('m2.endpoint'), defaultValue: provider.endpoint, required: true },
    { name: 'deploymentName', label: t('m2.deploymentName'), defaultValue: deployment.name, required: true },
    { name: 'modelName', label: t('m2.modelName'), defaultValue: deployment.modelName, required: true },
    { name: 'endpointOverride', label: t('m2.endpointOverride'), defaultValue: deployment.endpointOverride ?? '' },
    { name: 'credential', label: t('m2.credential'), type: 'select', defaultValue: deployment.credentialId ?? '', options: [{ value: '', label: t('m2.noCredential') }, ...(credentials.data?.items ?? []).map((item) => ({ value: item.id, label: item.name }))] },
    { name: 'parameters', label: t('m2.defaultParameters'), type: 'textarea', defaultValue: JSON.stringify(deployment.defaultParameters, null, 2) },
    { name: 'currency', label: t('m2.newPriceOptional'), defaultValue: 'USD' },
    { name: 'input', label: t('m2.inputPrice'), type: 'number', step: 'any' },
    { name: 'output', label: t('m2.outputPrice'), type: 'number', step: 'any' },
  ] : []
  const actions = auth.hasPermission('model:manage') ? <div className="flex gap-2">
    <PrerequisiteAction description={t('prerequisites.modelEditDescription')} loading={deployments.isLoading} onReady={() => setEditOpen(true)} requirements={[{ key: 'deployment', label: t('prerequisites.modelDeployment'), met: Boolean(deployment) }]} variant="secondary"><Pencil className="size-4" />{t('m2.editModel')}</PrerequisiteAction>
    <Button disabled={test.isPending} onClick={() => test.mutate()} variant="secondary"><Activity className="size-4" />{t('m2.connectionTest')}</Button>
  </div> : undefined
  return <>
    <ResourceDetailLayout actions={actions} description={t('pages.models.description')} details={value ? [
      { label: t('m2.alias'), value: value.alias }, { label: t('m2.providerName'), value: value.providerName },
      { label: t('m2.providerType'), value: value.providerType }, { label: t('m2.modelName'), value: value.modelName },
      { label: t('m2.endpoint'), value: provider?.endpoint }, { label: t('m2.deploymentRevision'), value: deployment ? `r${deployment.revisionNumber}` : undefined },
      { label: t('m2.department'), value: value.ownerDepartmentId },
    ] : []} error={model.error} loading={model.isLoading} name={value?.alias} status={value?.status}>
      {value && <Card className="flex items-center gap-3 p-4 text-xs"><Activity className="size-4 text-primary" /><span className="font-medium">{t('models.connectionStatus')}</span><StatusBadge status={connectionStatus(value.connectionStatus)} />{value.connectionCheckedAt && <span className="text-muted-foreground">{t('models.checkedAt', { time: new Date(value.connectionCheckedAt).toLocaleString() })}</span>}</Card>}
      <Card className="p-5"><div className="flex items-center"><h2 className="text-sm font-semibold">{t('m2.priceVersions')}</h2><div className="flex-1" />{auth.hasPermission('model:manage') && <Button onClick={() => setPriceOpen(true)} size="sm" variant="secondary"><Plus className="size-3.5" />{t('m2.addPrice')}</Button>}</div><div className="mt-3 divide-y divide-border">{prices.data?.map((price) => <div className="grid grid-cols-3 gap-3 py-3 text-xs" key={price.id}><span>v{price.versionNumber} · {price.currency}</span><span>{t('m2.inputPrice')}: {price.inputPerMillion}</span><span>{t('m2.outputPrice')}: {price.outputPerMillion}</span></div>)}{prices.data?.length === 0 && <p className="py-3 text-xs text-muted-foreground">{t('m2.noData')}</p>}</div></Card>
      <Card className="p-5"><div className="flex items-center gap-2"><History className="size-4 text-primary" /><h2 className="text-sm font-semibold">{t('m2.deploymentHistory')}</h2></div><div className="mt-3 divide-y divide-border">{history.data?.map((item) => <div className="grid grid-cols-[90px_1fr_auto] gap-3 py-3 text-xs" key={item.id}><span>r{item.revisionNumber}</span><span>{item.modelName}</span><span className="text-muted-foreground">{new Date(item.changedAt).toLocaleString()}</span></div>)}{history.data?.length === 0 && <p className="py-3 text-xs text-muted-foreground">{t('m2.noData')}</p>}</div></Card>
    </ResourceDetailLayout>
    {priceOpen && <EntityFormDialog cancelLabel={t('common.cancel')} fields={priceFields} onClose={() => setPriceOpen(false)} onSubmit={createPrice} open submitLabel={t('common.save')} title={t('m2.addPrice')} />}
    {editOpen && <EntityFormDialog cancelLabel={t('common.cancel')} fields={editFields} onClose={() => setEditOpen(false)} onSubmit={editModel} open submitLabel={t('common.save')} title={t('m2.editModel')} />}
  </>
}

function statusOptions(t: (key: string) => string) {
  return [{ value: 'active', label: t('common.active') }, { value: 'disabled', label: t('common.inactive') }]
}

function connectionStatus(value: string): 'untested' | 'healthy' | 'unhealthy' {
  return value === 'healthy' || value === 'unhealthy' ? value : 'untested'
}
