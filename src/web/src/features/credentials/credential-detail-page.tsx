import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { KeyRound, Pencil, Power, RefreshCw } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { Credential } from '../../shared/api/types'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { ResourceDetailLayout } from '../../shared/components/resource-detail-layout'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { useToast } from '../../shared/ui/toast'

export function CredentialDetailPage() {
  const { id = '' } = useParams()
  const { t } = useTranslation()
  const auth = useAuth()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const [rotateOpen, setRotateOpen] = useState(false)
  const [editOpen, setEditOpen] = useState(false)
  const credential = useQuery({ queryKey: ['credential', id], queryFn: () => apiRequest<Credential>(`/credentials/${id}`) })
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['credential', id] })
  const toggle = useMutation({
    mutationFn: () => apiRequest<Credential>(`/credentials/${id}`, { method: 'PATCH', body: jsonBody({ name: credential.data?.name, status: credential.data?.status === 'active' ? 'disabled' : 'active', version: credential.data?.version }) }),
    onSuccess: () => void refresh(),
    onError: (error: Error) => showToast(error.message),
  })
  const rotate = async (values: Record<string, string>) => {
    const secret = credential.data?.credentialType === 'basic' || credential.data?.credentialType === 'custom_json' ? JSON.parse(values.secret) as unknown : values.secret
    await apiRequest(`/credentials/${id}/rotate`, { method: 'POST', body: jsonBody({ secret, version: credential.data?.version }) })
    await refresh()
    showToast(t('credentials.saved'))
  }
  const rename = async (values: Record<string, string>) => {
    await apiRequest(`/credentials/${id}`, { method: 'PATCH', body: jsonBody({ name: values.name, status: credential.data?.status, version: credential.data?.version }) })
    await Promise.all([refresh(), queryClient.invalidateQueries({ queryKey: ['credentials'] })])
    showToast(t('credentials.saved'))
  }
  const value = credential.data
  const rotateFields: EntityFormField[] = [{ name: 'secret', label: t('credentials.secret'), type: 'password', required: true, placeholder: t('credentials.passwordMask') }]
  const actions = auth.hasPermission('credential:manage') && value ? <div className="flex gap-2">
    <Button onClick={() => setEditOpen(true)} variant="secondary"><Pencil className="size-4" />{t('common.edit')}</Button>
    <Button onClick={() => setRotateOpen(true)} variant="secondary"><RefreshCw className="size-4" />{t('credentials.rotate')}</Button>
    <Button disabled={toggle.isPending} onClick={() => toggle.mutate()} variant="secondary"><Power className="size-4" />{value.status === 'active' ? t('credentials.disable') : t('credentials.enable')}</Button>
  </div> : undefined
  return <>
    <ResourceDetailLayout actions={actions} description={t('credentials.credentialDescription')} details={value ? [
      { label: t('credentials.credentialType'), value: value.credentialType },
      { label: t('credentials.storageMode'), value: value.storageMode },
      { label: t('credentials.secret'), value: value.maskedHint },
      { label: t('common.version'), value: `v${value.currentSecretVersion}` },
      { label: 'Credential ID', value: value.id },
      { label: t('credentials.department'), value: value.ownerDepartmentId },
    ] : []} error={credential.error} loading={credential.isLoading} name={value?.name} status={value?.status}>
      <Card className="flex items-center gap-3 p-4 text-xs text-muted-foreground"><KeyRound className="size-4 text-primary" />{t('credentials.passwordMask')}</Card>
    </ResourceDetailLayout>
    {rotateOpen && <EntityFormDialog cancelLabel={t('common.cancel')} fields={rotateFields} onClose={() => setRotateOpen(false)} onSubmit={rotate} open submitLabel={t('credentials.rotate')} title={t('credentials.rotate')} />}
    {editOpen && value && <EntityFormDialog cancelLabel={t('common.cancel')} fields={[{ name: 'name', label: t('common.name'), defaultValue: value.name, required: true }]} onClose={() => setEditOpen(false)} onSubmit={rename} open submitLabel={t('common.save')} title={t('credentials.renameCredential')} />}
  </>
}
