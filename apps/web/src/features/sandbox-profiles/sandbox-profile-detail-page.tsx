import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { History, Pencil, Plus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { SandboxProfile, SandboxProfileVersion } from '../../shared/api/types'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { ResourceDetailLayout } from '../../shared/components/resource-detail-layout'
import { useLocaleFormat } from '../../shared/lib/locale-format'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { useToast } from '../../shared/ui/toast'
import { bytesToGb, bytesToKb, sandboxVersionBody, sandboxVersionFields } from './sandbox-profile-form'

export function SandboxProfileDetailPage() {
  const { id = '' } = useParams()
  const { t } = useTranslation()
  const { formatDateTime } = useLocaleFormat()
  const auth = useAuth()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const [editOpen, setEditOpen] = useState(false)
  const [versionOpen, setVersionOpen] = useState(false)
  const profile = useQuery({ queryKey: ['sandbox-profile', id], queryFn: () => apiRequest<SandboxProfile>(`/sandbox-profiles/${id}`) })
  const invalidate = async () => Promise.all([
    queryClient.invalidateQueries({ queryKey: ['sandbox-profile', id] }),
    queryClient.invalidateQueries({ queryKey: ['sandbox-profiles'] }),
  ])
  const update = useMutation({
    mutationFn: (values: Record<string, string>) => apiRequest<SandboxProfile>(`/sandbox-profiles/${id}`, { method: 'PATCH', body: jsonBody({ name: values.name.trim(), description: values.description.trim() || null, status: values.status, version: profile.data?.version }) }),
    onSuccess: async () => { await invalidate(); showToast(t('sandbox.saved')) },
  })
  const createVersion = useMutation({
    mutationFn: (values: Record<string, string>) => apiRequest<SandboxProfileVersion>(`/sandbox-profiles/${id}/versions`, { method: 'POST', body: jsonBody(sandboxVersionBody(values, t('sandbox.invalidJson'), t('sandbox.invalidImageTag'))) }),
    onSuccess: async () => { await invalidate(); showToast(t('sandbox.versionCreated')) },
  })
  const value = profile.data
  const editFields: EntityFormField[] = value ? [
    { name: 'name', label: t('common.name'), required: true, defaultValue: value.name },
    { name: 'description', label: t('common.description'), type: 'textarea', defaultValue: value.description ?? '' },
    { name: 'status', label: t('common.status'), type: 'select', defaultValue: value.status, required: true, options: [{ value: 'active', label: t('common.active') }, { value: 'disabled', label: t('common.inactive') }] },
  ] : []
  const actions = auth.hasPermission('sandbox:manage') ? <div className="flex gap-2"><Button onClick={() => setEditOpen(true)} variant="secondary"><Pencil className="size-4" />{t('common.edit')}</Button><Button onClick={() => setVersionOpen(true)}><Plus className="size-4" />{t('sandbox.newVersion')}</Button></div> : undefined
  return <>
    <ResourceDetailLayout actions={actions} description={t('sandbox.description')} details={value ? [
      { label: t('sandbox.runner'), value: value.current.runner },
      { label: t('sandbox.imageTag'), value: <span className="font-mono text-[11px]">{value.current.imageDigest}</span> },
      { label: t('sandbox.limits'), value: `${(value.current.cpuMillis / 1000).toFixed(3).replace(/\.?(0+)$/, '')} cores CPU · ${bytesToGb(value.current.memoryBytes)} GB RAM · ${value.current.pidsLimit} PIDs · ${bytesToGb(value.current.diskBytes)} GB disk` },
      { label: t('sandbox.timeoutSeconds'), value: `${value.current.timeoutSeconds}s` },
      { label: t('sandbox.outputKb'), value: `${bytesToKb(value.current.outputLimitBytes)} KB` },
      { label: t('sandbox.department'), value: value.ownerDepartmentId },
      { label: t('sandbox.networkPolicy'), value: <pre className="whitespace-pre-wrap text-[11px]">{JSON.stringify(value.current.networkPolicy, null, 2)}</pre> },
      { label: t('sandbox.configurationHash'), value: <span className="font-mono text-[11px]">{value.current.configurationHash}</span> },
    ] : []} error={profile.error} loading={profile.isLoading} name={value?.name} status={value?.status}>
      <Card className="p-5"><div className="flex items-center gap-2"><History className="size-4 text-primary" /><h2 className="text-sm font-semibold">{t('sandbox.versionHistory')}</h2></div><div className="mt-3 divide-y divide-border">{value?.versions.map((version) => <div className="grid grid-cols-[70px_100px_minmax(0,1fr)_auto] gap-3 py-3 text-xs" key={version.id}><span>v{version.versionNumber}</span><span>{version.runner}</span><span className="truncate font-mono text-[11px]" title={version.imageDigest}>{version.imageDigest}</span><span className="text-muted-foreground">{formatDateTime(version.createdAt)}</span></div>)}</div></Card>
    </ResourceDetailLayout>
    {editOpen && <EntityFormDialog cancelLabel={t('common.cancel')} fields={editFields} onClose={() => setEditOpen(false)} onSubmit={(values) => update.mutateAsync(values).then(() => undefined)} open submitLabel={t('common.save')} title={t('sandbox.edit')} />}
    {versionOpen && value && <EntityFormDialog cancelLabel={t('common.cancel')} fields={sandboxVersionFields(t, value.current)} onClose={() => setVersionOpen(false)} onSubmit={(values) => createVersion.mutateAsync(values).then(() => undefined)} open submitLabel={t('common.save')} title={t('sandbox.newVersion')} />}
  </>
}
