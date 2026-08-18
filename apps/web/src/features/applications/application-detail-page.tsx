import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Copy, Edit3, KeyRound, Plus, RotateCw, ShieldOff } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { Application, ApplicationApiKey, ApplicationDeployment, ApplicationSchedule, ApplicationSession, ApplicationWebhook, PublishAttempt, WorkflowEnvironment, WorkflowVersion } from '../../shared/api/types'
import { ConfirmDialog } from '../../shared/components/confirm-dialog'
import { EmptyState } from '../../shared/components/empty-state'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { EntityDeleteButton } from '../../shared/components/entity-delete-button'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { PrerequisiteAction } from '../../shared/components/prerequisite-action'
import { StatusBadge } from '../../shared/components/status-badge'
import { localizedValue } from '../../shared/lib/localized-value'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../../shared/ui/tabs'
import { useToast } from '../../shared/ui/toast'

type DialogKind = 'edit' | 'deployment' | 'key' | 'webhook' | 'webhookEdit' | 'schedule' | 'scheduleEdit' | 'sessionUpgrade' | null
type KeyAction = { keyId: string; action: 'rotate' | 'revoke' } | null
type DeploymentAction = { deploymentId: string; attemptId?: string; action: 'retry' | 'rollback' } | null
const pendingDeploymentStates = new Set(['building', 'copying', 'preparing', 'prepared', 'activating'])

export function ApplicationDetailPage() {
  const { id = '' } = useParams()
  const { t } = useTranslation()
  const auth = useAuth()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const [dialog, setDialog] = useState<DialogKind>(null)
  const [selectedSchedule, setSelectedSchedule] = useState<ApplicationSchedule | null>(null)
  const [selectedWebhook, setSelectedWebhook] = useState<ApplicationWebhook | null>(null)
  const [selectedSession, setSelectedSession] = useState<ApplicationSession | null>(null)
  const [keyConfirmation, setKeyConfirmation] = useState<KeyAction>(null)
  const [deploymentConfirmation, setDeploymentConfirmation] = useState<DeploymentAction>(null)
  const [shownSecret, setShownSecret] = useState<string | null>(null)
  const [shownWebhookSecret, setShownWebhookSecret] = useState<string | null>(null)

  const application = useQuery({ queryKey: ['application', id], queryFn: () => apiRequest<Application>(`/applications/${id}`) })
  const deployments = useQuery({
    queryKey: ['application-deployments', id],
    queryFn: () => apiRequest<ApplicationDeployment[]>(`/applications/${id}/deployments`),
    refetchInterval: (query) => (query.state.data as ApplicationDeployment[] | undefined)?.some((item) => pendingDeploymentStates.has(item.status)) ? 2000 : false,
  })
  const keys = useQuery({ queryKey: ['application-keys', id], enabled: auth.hasPermission('application:manage_key'), queryFn: () => apiRequest<ApplicationApiKey[]>(`/applications/${id}/api-keys`) })
  const webhooks = useQuery({ queryKey: ['application-webhooks', id], queryFn: () => apiRequest<ApplicationWebhook[]>(`/applications/${id}/webhooks`) })
  const schedules = useQuery({ queryKey: ['application-schedules', id], queryFn: () => apiRequest<ApplicationSchedule[]>(`/applications/${id}/schedules`) })
  const sessions = useQuery({ queryKey: ['application-sessions', id], queryFn: () => apiRequest<ApplicationSession[]>(`/applications/${id}/sessions`) })
  const versions = useQuery({ queryKey: ['workflow-versions', application.data?.workflowId], enabled: Boolean(application.data), queryFn: () => apiRequest<WorkflowVersion[]>(`/workflows/${application.data?.workflowId}/versions`) })
  const environments = useQuery({ queryKey: ['environments'], queryFn: () => apiRequest<WorkflowEnvironment[]>('/environments') })
  const refresh = async () => queryClient.invalidateQueries({ predicate: (query) => String(query.queryKey[0]).startsWith('application') })

  const save = useMutation<unknown, Error, { kind: Exclude<DialogKind, null>; values: Record<string, string> }>({
    mutationFn: ({ kind, values }) => {
      if (kind === 'edit') return apiRequest<Application>(`/applications/${id}`, { method: 'PATCH', body: jsonBody({ name: values.name, description: values.description || null, visibility: values.visibility, status: values.status, version: application.data?.version }) })
      if (kind === 'deployment') return apiRequest<ApplicationDeployment>(`/applications/${id}/deployments`, { method: 'POST', body: jsonBody({ workflowVersionId: values.workflowVersionId, environmentId: values.environmentId, sessionVersionPolicy: values.sessionVersionPolicy }) })
      if (kind === 'key') return apiRequest<ApplicationApiKey>(`/applications/${id}/api-keys`, { method: 'POST', body: jsonBody({ name: values.name }) })
      if (kind === 'webhook') return apiRequest<ApplicationWebhook>(`/applications/${id}/webhooks`, { method: 'POST', body: jsonBody({ name: values.name }) })
      if (kind === 'webhookEdit' && selectedWebhook) return apiRequest<ApplicationWebhook>(`/applications/${id}/webhooks/${selectedWebhook.id}`, { method: 'PATCH', body: jsonBody({ name: values.name, status: values.status, version: selectedWebhook.version }) })
      if (kind === 'sessionUpgrade' && selectedSession) return apiRequest<ApplicationSession>(`/sessions/${selectedSession.id}/upgrade`, { method: 'POST', body: jsonBody({ workflowVersionId: values.workflowVersionId, version: selectedSession.version }) })
      const path = kind === 'scheduleEdit' && selectedSchedule ? `/applications/${id}/schedules/${selectedSchedule.id}` : `/applications/${id}/schedules`
      return apiRequest<ApplicationSchedule>(path, { method: kind === 'scheduleEdit' ? 'PATCH' : 'POST', body: jsonBody({ name: values.name, cronExpression: values.cron, timezone: values.timezone, input: JSON.parse(values.input), misfirePolicy: values.misfirePolicy, ...(kind === 'scheduleEdit' ? { status: values.status, version: selectedSchedule?.version } : {}) }) })
    },
    onSuccess: async (result, variables) => {
      if (result && typeof result === 'object' && 'secret' in result && typeof result.secret === 'string') {
        if (variables.kind === 'webhook') setShownWebhookSecret(result.secret)
        else setShownSecret(result.secret)
      }
      await refresh()
      showToast(t('applications.saved'))
    },
  })
  const keyAction = useMutation({
    mutationFn: ({ keyId, action }: Exclude<KeyAction, null>) => apiRequest<ApplicationApiKey | undefined>(`/applications/${id}/api-keys/${keyId}/${action}`, { method: 'POST' }),
    onSuccess: async (value) => { if (value?.secret) setShownSecret(value.secret); setKeyConfirmation(null); await refresh(); showToast(t('applications.saved')) },
  })
  const deploymentAction = useMutation({
    mutationFn: ({ deploymentId, attemptId, action }: Exclude<DeploymentAction, null>) => {
      const path = action === 'retry'
        ? `/applications/${id}/publish-attempts/${attemptId}:retry`
        : `/applications/${id}/deployments/${deploymentId}:rollback`
      return apiRequest<PublishAttempt>(path, { method: 'POST' })
    },
    onSuccess: async () => { setDeploymentConfirmation(null); await refresh(); showToast(t('applications.publishActionAccepted')) },
    onError: (error: Error) => showToast(error.message),
  })

  if (application.isLoading) return <PageContainer><p className="text-sm text-muted-foreground">{t('common.loading')}</p></PageContainer>
  if (!application.data) return <PageContainer><EmptyState title={t('common.loadFailed')} description={String(application.error ?? '')} /></PageContainer>
  const value = application.data
  const schedule = selectedSchedule
  const fields: Record<Exclude<DialogKind, null>, EntityFormField[]> = {
    edit: [
      { name: 'name', label: t('common.name'), required: true, defaultValue: value.name },
      { name: 'description', label: t('common.description'), type: 'textarea', defaultValue: value.description ?? '' },
      { name: 'visibility', label: t('applications.visibility'), type: 'select', defaultValue: value.visibility, options: ['private', 'department', 'company'].map((item) => ({ value: item, label: t(`applications.${item}`) })) },
      { name: 'status', label: t('common.status'), type: 'select', defaultValue: value.status, options: ['draft', 'active', 'disabled'].map((item) => ({ value: item, label: t(`applications.${item}`) })) },
    ],
    deployment: [
      { name: 'workflowVersionId', label: t('evaluations.workflowVersion'), type: 'select', required: true, options: (versions.data ?? []).map((item) => ({ value: item.id, label: `v${item.versionNumber}` })) },
      { name: 'environmentId', label: t('applications.environment'), type: 'select', required: true, options: (environments.data ?? []).filter((item) => item.status === 'active').map((item) => ({ value: item.id, label: item.name })) },
      { name: 'sessionVersionPolicy', label: t('applications.sessionPolicy'), type: 'select', defaultValue: 'pinned', options: ['pinned', 'follow_deployment', 'manual_upgrade'].map((item) => ({ value: item, label: t(`applications.${item}`) })) },
    ],
    key: [{ name: 'name', label: t('common.name'), required: true }],
    webhook: [{ name: 'name', label: t('common.name'), required: true }],
    webhookEdit: [{ name: 'name', label: t('common.name'), required: true, defaultValue: selectedWebhook?.name ?? '' }, { name: 'status', label: t('common.status'), type: 'select', defaultValue: selectedWebhook?.status ?? 'active', options: ['active', 'disabled'].map((item) => ({ value: item, label: t(`applications.${item}`) })) }],
    schedule: scheduleFields(t),
    scheduleEdit: scheduleFields(t, schedule),
    sessionUpgrade: [{ name: 'workflowVersionId', label: t('evaluations.workflowVersion'), type: 'select', defaultValue: selectedSession?.workflowVersionId ?? '', options: (versions.data ?? []).map((item) => ({ value: item.id, label: `v${item.versionNumber}` })) }],
  }
  return <PageContainer>
    <PageHeader action={auth.hasPermission('application:manage') ? <Button onClick={() => setDialog('edit')} variant="secondary"><Edit3 className="size-4" />{t('common.edit')}</Button> : undefined} description={`${value.workflowName} · ${value.slug}`} title={value.name} />
    <div className="mt-5"><StatusBadge status={value.status === 'active' ? 'active' : value.status === 'disabled' ? 'inactive' : 'draft'} /></div>
    <Tabs className="mt-6" defaultValue="deployments">
      <TabsList className="border-b border-border"><TabsTrigger value="deployments">{t('applications.deployments')}</TabsTrigger><TabsTrigger value="keys">{t('applications.apiKeys')}</TabsTrigger><TabsTrigger value="triggers">{t('applications.triggers')}</TabsTrigger><TabsTrigger value="sessions">{t('applications.playground.sessions')}</TabsTrigger></TabsList>
      <TabsContent className="pt-5" value="deployments"><Section action={auth.hasPermission('application:manage') && <PrerequisiteAction description={t('applications.prerequisites.deploymentDescription')} loading={versions.isLoading || environments.isLoading} onReady={() => setDialog('deployment')} requirements={[{ key: 'workflow-version', label: t('applications.prerequisites.workflowVersion'), met: Boolean(versions.data?.length), href: `/workflows/${value.workflowId}`, actionLabel: t('applications.prerequisites.goWorkflows') }, { key: 'environment', label: t('applications.prerequisites.activeEnvironment'), met: Boolean(environments.data?.some((item) => item.status === 'active')) }]} size="sm"><Plus className="size-4" />{t('applications.deploy')}</PrerequisiteAction>} title={t('applications.deployments')}>{deployments.data?.map((item) => <Row action={auth.hasPermission('application:manage') ? <div className="flex gap-1">{item.status === 'rejected' && item.publishAttemptId && <Button onClick={() => setDeploymentConfirmation({ deploymentId: item.id, attemptId: item.publishAttemptId ?? undefined, action: 'retry' })} size="sm" variant="ghost">{t('applications.retryPublish')}</Button>}{item.status === 'superseded' && <Button onClick={() => setDeploymentConfirmation({ deploymentId: item.id, action: 'rollback' })} size="sm" variant="ghost">{t('applications.rollback')}</Button>}</div> : undefined} detail={<>{localizedValue(t, 'applications', item.sessionVersionPolicy)} · #{item.sequenceNumber}{item.publishErrorMessage && <span className="mt-1 block text-danger">{item.publishErrorCode ? `${item.publishErrorCode}: ` : ''}{item.publishErrorMessage}</span>}</>} key={item.id} status={<StatusBadge label={deploymentStatusLabel(t, item.status)} status={deploymentStatus(item.status)} />} title={`${item.environmentName} · v${item.workflowVersionNumber}`} />)}</Section></TabsContent>
      <TabsContent className="pt-5" value="keys"><Section action={auth.hasPermission('application:manage_key') && <Button onClick={() => setDialog('key')} size="sm"><Plus className="size-4" />{t('applications.createKey')}</Button>} title={t('applications.apiKeys')}>
        {shownSecret && <div className="my-4 rounded-lg border border-warning/30 bg-warning/10 p-4"><p className="text-xs font-semibold">{t('applications.secretOnce')}</p><div className="mt-2 flex items-center gap-2"><code className="min-w-0 flex-1 break-all text-xs">{shownSecret}</code><Button aria-label={t('common.copy')} onClick={() => void navigator.clipboard.writeText(shownSecret)} size="icon" variant="ghost"><Copy className="size-4" /></Button></div></div>}
        {keys.data?.map((item) => <div className="flex items-center border-t border-border py-3 first:border-0" key={item.id}><KeyRound className="mr-3 size-4 text-muted-foreground" /><div><p className="text-xs font-medium">{item.name}</p><p className="mt-1 text-[11px] text-muted-foreground">{item.prefix} · {localizedValue(t, 'applications', item.status)}</p></div><div className="flex-1" />{item.status === 'active' && <><Button aria-label={t('applications.rotateKey')} onClick={() => setKeyConfirmation({ keyId: item.id, action: 'rotate' })} size="icon" variant="ghost"><RotateCw className="size-4" /></Button><Button aria-label={t('applications.revoke')} onClick={() => setKeyConfirmation({ keyId: item.id, action: 'revoke' })} size="icon" variant="ghost"><ShieldOff className="size-4" /></Button></>}</div>)}
      </Section></TabsContent>
      <TabsContent className="grid grid-cols-2 gap-5 pt-5" value="triggers">
        {value.runtimeConfigRevision > value.publishedRuntimeConfigRevision && <div className="col-span-2"><StatusBadge label={t('applications.pendingPublish')} status="pending" /></div>}
        <Section action={auth.hasPermission('application:manage') && <Button onClick={() => setDialog('webhook')} size="sm"><Plus className="size-4" />{t('common.create')}</Button>} title={t('applications.webhooks')}>{shownWebhookSecret && <div className="my-4 rounded-lg border border-warning/30 bg-warning/10 p-4"><p className="text-xs font-semibold">{t('applications.secretOnce')}</p><div className="mt-2 flex items-center gap-2"><code className="min-w-0 flex-1 break-all text-xs">{shownWebhookSecret}</code><Button aria-label={t('common.copy')} onClick={() => void navigator.clipboard.writeText(shownWebhookSecret)} size="icon" variant="ghost"><Copy className="size-4" /></Button></div></div>}{webhooks.data?.map((item) => <Row action={<div className="flex gap-1">{auth.hasPermission('application:manage') && <Button aria-label={t('common.edit')} onClick={() => { setSelectedWebhook(item); setDialog('webhookEdit') }} size="icon" variant="ghost"><Edit3 className="size-4" /></Button>}<EntityDeleteButton canDelete={auth.hasPermission('application:delete')} deletePath={`/applications/${id}/webhooks/${item.id}`} entityId={item.id} entityName={item.name} entityType="application_webhook" onDeleted={async () => { await queryClient.invalidateQueries({ queryKey: ['application-webhooks', id] }); showToast(t('applications.deleted')) }} /></div>} detail={item.path} key={item.id} status={localizedValue(t, 'applications', item.status)} title={item.name} />)}</Section>
        <Section action={auth.hasPermission('application:manage') && <Button onClick={() => { setSelectedSchedule(null); setDialog('schedule') }} size="sm"><Plus className="size-4" />{t('common.create')}</Button>} title={t('applications.schedules')}>{schedules.data?.map((item) => <Row action={<div className="flex gap-1">{auth.hasPermission('application:manage') && <Button aria-label={t('common.edit')} onClick={() => { setSelectedSchedule(item); setDialog('scheduleEdit') }} size="icon" variant="ghost"><Edit3 className="size-4" /></Button>}<EntityDeleteButton canDelete={auth.hasPermission('application:delete')} deletePath={`/applications/${id}/schedules/${item.id}`} entityId={item.id} entityName={item.name} entityType="application_schedule" onDeleted={async () => { await queryClient.invalidateQueries({ queryKey: ['application-schedules', id] }); showToast(t('applications.deleted')) }} /></div>} detail={`${item.cronExpression} · ${item.timezone}`} key={item.id} status={localizedValue(t, 'applications', item.status)} title={item.name} />)}</Section>
      </TabsContent>
      <TabsContent className="pt-5" value="sessions"><Section title={t('applications.playground.sessions')}>{sessions.data?.map((item) => <Row action={item.versionPolicy === 'manual_upgrade' && auth.hasPermission('application:manage') ? <Button onClick={() => { setSelectedSession(item); setDialog('sessionUpgrade') }} size="sm" variant="ghost">{t('applications.upgrade')}</Button> : undefined} detail={`${localizedValue(t, 'applications', item.versionPolicy)} · ${item.workflowVersionId ?? '—'}`} key={item.id} status={localizedValue(t, 'applications', item.status)} title={item.title ?? item.id} />)}</Section></TabsContent>
    </Tabs>
    {dialog && <EntityFormDialog cancelLabel={t('common.cancel')} fields={fields[dialog]} onClose={() => setDialog(null)} onSubmit={(values) => save.mutateAsync({ kind: dialog, values }).then(() => undefined)} open submitLabel={t('common.save')} title={dialog === 'webhookEdit' ? t('applications.editWebhook') : t(`applications.${dialog}`)} />}
    <ConfirmDialog cancelLabel={t('common.cancel')} confirmLabel={keyConfirmation?.action === 'rotate' ? t('applications.rotateKey') : t('applications.revoke')} description={keyConfirmation?.action === 'rotate' ? t('applications.confirmRotate') : t('applications.confirmRevoke')} onClose={() => setKeyConfirmation(null)} onConfirm={async () => { if (keyConfirmation) await keyAction.mutateAsync(keyConfirmation) }} open={Boolean(keyConfirmation)} pending={keyAction.isPending} title={keyConfirmation?.action === 'rotate' ? t('applications.rotateKey') : t('applications.revokeKey')} />
    <ConfirmDialog cancelLabel={t('common.cancel')} confirmLabel={deploymentConfirmation?.action === 'retry' ? t('applications.retryPublish') : t('applications.rollback')} description={deploymentConfirmation?.action === 'retry' ? t('applications.confirmRetryPublish') : t('applications.confirmRollback')} onClose={() => setDeploymentConfirmation(null)} onConfirm={async () => { if (deploymentConfirmation) await deploymentAction.mutateAsync(deploymentConfirmation) }} open={Boolean(deploymentConfirmation)} pending={deploymentAction.isPending} title={deploymentConfirmation?.action === 'retry' ? t('applications.retryPublish') : t('applications.rollback')} variant="primary" />
  </PageContainer>
}

function scheduleFields(t: (key: string) => string, value?: ApplicationSchedule | null): EntityFormField[] {
  return [
    { name: 'name', label: t('common.name'), required: true, defaultValue: value?.name ?? '' },
    { name: 'cron', label: 'Cron', defaultValue: value?.cronExpression ?? '0 9 * * 1' },
    { name: 'timezone', label: t('applications.timezone'), defaultValue: value?.timezone ?? 'Asia/Shanghai' },
    { name: 'misfirePolicy', label: t('applications.misfirePolicy'), type: 'select', defaultValue: value?.misfirePolicy ?? 'fire_once', options: ['fire_once', 'skip'].map((item) => ({ value: item, label: t(`applications.${item}`) })) },
    { name: 'input', label: t('applications.input'), type: 'textarea', defaultValue: JSON.stringify(value?.input ?? {}, null, 2) },
    ...(value ? [{ name: 'status', label: t('common.status'), type: 'select' as const, defaultValue: value.status, options: ['active', 'disabled'].map((item) => ({ value: item, label: t(`applications.${item}`) })) }] : []),
  ]
}

function Section({ title, action, children }: { title: string; action?: React.ReactNode; children?: React.ReactNode }) { return <Card className="overflow-hidden"><div className="flex min-h-14 items-center justify-between border-b border-border px-5"><h2 className="text-sm font-semibold">{title}</h2>{action}</div><div className="px-5">{children || <p className="py-8 text-center text-xs text-muted-foreground">—</p>}</div></Card> }
function Row({ title, detail, status, action }: { title: string; detail: React.ReactNode; status: React.ReactNode; action?: React.ReactNode }) { return <div aria-label={title} className="flex min-h-16 items-center border-t border-border first:border-0" role="group"><div><p className="text-xs font-medium">{title}</p><div className="mt-1 text-[11px] text-muted-foreground">{detail}</div></div><div className="flex-1" /><span className="text-[11px] text-muted-foreground">{status}</span>{action && <div className="ml-2">{action}</div>}</div> }

function deploymentStatus(status: string): 'active' | 'inactive' | 'pending' | 'running' | 'failed' {
  if (status === 'active') return 'active'
  if (status === 'rejected') return 'failed'
  if (status === 'superseded') return 'inactive'
  if (status === 'activating') return 'running'
  return 'pending'
}

function deploymentStatusLabel(t: (key: string) => string, status: string): string {
  const labels: Record<string, string> = {
    building: t('applications.building'),
    copying: t('applications.copying'),
    preparing: t('applications.preparing'),
    prepared: t('applications.prepared'),
    activating: t('applications.activating'),
    active: t('applications.active'),
    rejected: t('applications.rejected'),
    superseded: t('applications.superseded'),
  }
  return labels[status] ?? status
}
