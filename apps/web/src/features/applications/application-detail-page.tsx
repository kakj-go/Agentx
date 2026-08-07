import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Copy, Edit3, KeyRound, Plus, RotateCw, ShieldOff } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { Application, ApplicationApiKey, ApplicationDeployment, ApplicationSchedule, ApplicationSession, ApplicationWebhook, WorkflowEnvironment, WorkflowVersion } from '../../shared/api/types'
import { ConfirmDialog } from '../../shared/components/confirm-dialog'
import { EmptyState } from '../../shared/components/empty-state'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { PrerequisiteAction } from '../../shared/components/prerequisite-action'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../../shared/ui/tabs'
import { useToast } from '../../shared/ui/toast'

type DialogKind = 'edit' | 'deployment' | 'key' | 'webhook' | 'webhookEdit' | 'schedule' | 'scheduleEdit' | 'sessionUpgrade' | null
type KeyAction = { keyId: string; action: 'rotate' | 'revoke' } | null

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
  const [shownSecret, setShownSecret] = useState<string | null>(null)
  const [shownWebhookSecret, setShownWebhookSecret] = useState<string | null>(null)

  const application = useQuery({ queryKey: ['application', id], queryFn: () => apiRequest<Application>(`/applications/${id}`) })
  const deployments = useQuery({ queryKey: ['application-deployments', id], queryFn: () => apiRequest<ApplicationDeployment[]>(`/applications/${id}/deployments`) })
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
      if (kind === 'deployment') return apiRequest<ApplicationDeployment>(`/applications/${id}/deployments`, { method: 'POST', body: jsonBody({ workflowVersionId: values.workflowVersionId, environmentId: values.environmentId, inputSchema: JSON.parse(values.inputSchema), outputSchema: JSON.parse(values.outputSchema), outputExpression: values.outputExpression?.trim() || null, sessionVersionPolicy: values.sessionVersionPolicy }) })
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
      showToast(t('m3.saved'))
    },
  })
  const keyAction = useMutation({
    mutationFn: ({ keyId, action }: Exclude<KeyAction, null>) => apiRequest<ApplicationApiKey | undefined>(`/applications/${id}/api-keys/${keyId}/${action}`, { method: 'POST' }),
    onSuccess: async (value) => { if (value?.secret) setShownSecret(value.secret); setKeyConfirmation(null); await refresh(); showToast(t('m3.saved')) },
  })

  if (application.isLoading) return <PageContainer><p className="text-sm text-muted-foreground">{t('m2.loading')}</p></PageContainer>
  if (!application.data) return <PageContainer><EmptyState title={t('m2.loadFailed')} description={String(application.error ?? '')} /></PageContainer>
  const value = application.data
  const schedule = selectedSchedule
  const fields: Record<Exclude<DialogKind, null>, EntityFormField[]> = {
    edit: [
      { name: 'name', label: t('common.name'), required: true, defaultValue: value.name },
      { name: 'description', label: t('common.description'), type: 'textarea', defaultValue: value.description ?? '' },
      { name: 'visibility', label: t('m2.visibility'), type: 'select', defaultValue: value.visibility, options: ['private', 'department', 'company'].map((item) => ({ value: item, label: t(`m2.${item}`) })) },
      { name: 'status', label: t('common.status'), type: 'select', defaultValue: value.status, options: ['draft', 'active', 'disabled'].map((item) => ({ value: item, label: t(`m3.${item}`) })) },
    ],
    deployment: [
      { name: 'workflowVersionId', label: t('pages.evaluations.workflowVersion'), type: 'select', required: true, options: (versions.data ?? []).map((item) => ({ value: item.id, label: `v${item.versionNumber}` })) },
      { name: 'environmentId', label: t('pages.applications.environment'), type: 'select', required: true, options: (environments.data ?? []).filter((item) => item.status === 'active').map((item) => ({ value: item.id, label: item.name })) },
      { name: 'sessionVersionPolicy', label: t('m3.sessionPolicy'), type: 'select', defaultValue: 'pinned', options: ['pinned', 'follow_deployment', 'manual_upgrade'].map((item) => ({ value: item, label: t(`m3.${item}`) })) },
      { name: 'inputSchema', label: t('m2.inputSchema'), type: 'textarea', defaultValue: '{"type":"object"}' },
      { name: 'outputSchema', label: t('m2.outputSchema'), type: 'textarea', defaultValue: '{"type":"object"}' },
      { name: 'outputExpression', label: t('common.outputExpression'), defaultValue: '' },
    ],
    key: [{ name: 'name', label: t('common.name'), required: true }],
    webhook: [{ name: 'name', label: t('common.name'), required: true }],
    webhookEdit: [{ name: 'name', label: t('common.name'), required: true, defaultValue: selectedWebhook?.name ?? '' }, { name: 'status', label: t('common.status'), type: 'select', defaultValue: selectedWebhook?.status ?? 'active', options: ['active', 'disabled'].map((item) => ({ value: item, label: t(`m3.${item}`) })) }],
    schedule: scheduleFields(t),
    scheduleEdit: scheduleFields(t, schedule),
    sessionUpgrade: [{ name: 'workflowVersionId', label: t('pages.evaluations.workflowVersion'), type: 'select', defaultValue: selectedSession?.workflowVersionId ?? '', options: (versions.data ?? []).map((item) => ({ value: item.id, label: `v${item.versionNumber}` })) }],
  }
  return <PageContainer>
    <PageHeader action={auth.hasPermission('application:manage') ? <Button onClick={() => setDialog('edit')} variant="secondary"><Edit3 className="size-4" />{t('common.edit')}</Button> : undefined} description={`${value.workflowName} · ${value.slug}`} title={value.name} />
    <div className="mt-5"><StatusBadge status={value.status === 'active' ? 'active' : value.status === 'disabled' ? 'inactive' : 'draft'} /></div>
    <Tabs className="mt-6" defaultValue="deployments">
      <TabsList className="border-b border-border"><TabsTrigger value="deployments">{t('m3.deployments')}</TabsTrigger><TabsTrigger value="keys">API Keys</TabsTrigger><TabsTrigger value="triggers">{t('m3.triggers')}</TabsTrigger><TabsTrigger value="sessions">{t('pages.playground.sessions')}</TabsTrigger></TabsList>
      <TabsContent className="pt-5" value="deployments"><Section action={auth.hasPermission('application:manage') && <PrerequisiteAction description={t('prerequisites.applicationDeploymentDescription')} loading={versions.isLoading || environments.isLoading} onReady={() => setDialog('deployment')} requirements={[{ key: 'workflow-version', label: t('prerequisites.workflowVersion'), met: Boolean(versions.data?.length), href: `/workflows/${value.workflowId}`, actionLabel: t('prerequisites.goWorkflows') }, { key: 'environment', label: t('prerequisites.activeEnvironment'), met: Boolean(environments.data?.some((item) => item.status === 'active')) }]} size="sm"><Plus className="size-4" />{t('m3.deploy')}</PrerequisiteAction>} title={t('m3.deployments')}>{deployments.data?.map((item) => <Row detail={`${item.sessionVersionPolicy} · #${item.sequenceNumber}`} key={item.id} status={item.status} title={`${item.environmentName} · v${item.workflowVersionNumber}`} />)}</Section></TabsContent>
      <TabsContent className="pt-5" value="keys"><Section action={auth.hasPermission('application:manage_key') && <Button onClick={() => setDialog('key')} size="sm"><Plus className="size-4" />{t('m3.createKey')}</Button>} title="API Keys">
        {shownSecret && <div className="my-4 rounded-lg border border-warning/30 bg-warning/10 p-4"><p className="text-xs font-semibold">{t('m3.secretOnce')}</p><div className="mt-2 flex items-center gap-2"><code className="min-w-0 flex-1 break-all text-xs">{shownSecret}</code><Button aria-label={t('common.copy')} onClick={() => void navigator.clipboard.writeText(shownSecret)} size="icon" variant="ghost"><Copy className="size-4" /></Button></div></div>}
        {keys.data?.map((item) => <div className="flex items-center border-t border-border py-3 first:border-0" key={item.id}><KeyRound className="mr-3 size-4 text-muted-foreground" /><div><p className="text-xs font-medium">{item.name}</p><p className="mt-1 text-[11px] text-muted-foreground">{item.prefix} · {item.status}</p></div><div className="flex-1" />{item.status === 'active' && <><Button aria-label={t('m2.rotate')} onClick={() => setKeyConfirmation({ keyId: item.id, action: 'rotate' })} size="icon" variant="ghost"><RotateCw className="size-4" /></Button><Button aria-label={t('m3.revoke')} onClick={() => setKeyConfirmation({ keyId: item.id, action: 'revoke' })} size="icon" variant="ghost"><ShieldOff className="size-4" /></Button></>}</div>)}
      </Section></TabsContent>
      <TabsContent className="grid grid-cols-2 gap-5 pt-5" value="triggers">
        <Section action={auth.hasPermission('application:manage') && <Button onClick={() => setDialog('webhook')} size="sm"><Plus className="size-4" />{t('common.create')}</Button>} title="Webhooks">{shownWebhookSecret && <div className="my-4 rounded-lg border border-warning/30 bg-warning/10 p-4"><p className="text-xs font-semibold">{t('m3.secretOnce')}</p><div className="mt-2 flex items-center gap-2"><code className="min-w-0 flex-1 break-all text-xs">{shownWebhookSecret}</code><Button aria-label={t('common.copy')} onClick={() => void navigator.clipboard.writeText(shownWebhookSecret)} size="icon" variant="ghost"><Copy className="size-4" /></Button></div></div>}{webhooks.data?.map((item) => <Row action={auth.hasPermission('application:manage') ? <Button aria-label={t('common.edit')} onClick={() => { setSelectedWebhook(item); setDialog('webhookEdit') }} size="icon" variant="ghost"><Edit3 className="size-4" /></Button> : undefined} detail={item.path} key={item.id} status={item.status} title={item.name} />)}</Section>
        <Section action={auth.hasPermission('application:manage') && <Button onClick={() => { setSelectedSchedule(null); setDialog('schedule') }} size="sm"><Plus className="size-4" />{t('common.create')}</Button>} title={t('m3.schedules')}>{schedules.data?.map((item) => <Row action={auth.hasPermission('application:manage') ? <Button aria-label={t('common.edit')} onClick={() => { setSelectedSchedule(item); setDialog('scheduleEdit') }} size="icon" variant="ghost"><Edit3 className="size-4" /></Button> : undefined} detail={`${item.cronExpression} · ${item.timezone}`} key={item.id} status={item.status} title={item.name} />)}</Section>
      </TabsContent>
      <TabsContent className="pt-5" value="sessions"><Section title={t('pages.playground.sessions')}>{sessions.data?.map((item) => <Row action={item.versionPolicy === 'manual_upgrade' && auth.hasPermission('application:manage') ? <Button onClick={() => { setSelectedSession(item); setDialog('sessionUpgrade') }} size="sm" variant="ghost">{t('m3.upgrade')}</Button> : undefined} detail={`${item.versionPolicy} · ${item.workflowVersionId ?? '—'}`} key={item.id} status={item.status} title={item.title ?? item.id} />)}</Section></TabsContent>
    </Tabs>
    {dialog && <EntityFormDialog cancelLabel={t('common.cancel')} fields={fields[dialog]} onClose={() => setDialog(null)} onSubmit={(values) => save.mutateAsync({ kind: dialog, values }).then(() => undefined)} open submitLabel={t('common.save')} title={dialog === 'webhookEdit' ? `${t('common.edit')} Webhook` : t(`m3.${dialog}`)} />}
    <ConfirmDialog cancelLabel={t('common.cancel')} confirmLabel={keyConfirmation?.action === 'rotate' ? t('m2.rotate') : t('m3.revoke')} description={keyConfirmation?.action === 'rotate' ? t('m3.confirmRotate') : t('m3.confirmRevoke')} onClose={() => setKeyConfirmation(null)} onConfirm={async () => { if (keyConfirmation) await keyAction.mutateAsync(keyConfirmation) }} open={Boolean(keyConfirmation)} pending={keyAction.isPending} title={keyConfirmation?.action === 'rotate' ? t('m3.rotateKey') : t('m3.revokeKey')} />
  </PageContainer>
}

function scheduleFields(t: (key: string) => string, value?: ApplicationSchedule | null): EntityFormField[] {
  return [
    { name: 'name', label: t('common.name'), required: true, defaultValue: value?.name ?? '' },
    { name: 'cron', label: 'Cron', defaultValue: value?.cronExpression ?? '0 9 * * 1' },
    { name: 'timezone', label: t('m3.timezone'), defaultValue: value?.timezone ?? 'Asia/Shanghai' },
    { name: 'misfirePolicy', label: t('m3.misfirePolicy'), type: 'select', defaultValue: value?.misfirePolicy ?? 'fire_once', options: ['fire_once', 'skip'].map((item) => ({ value: item, label: t(`m3.${item}`) })) },
    { name: 'input', label: t('m3.input'), type: 'textarea', defaultValue: JSON.stringify(value?.input ?? {}, null, 2) },
    ...(value ? [{ name: 'status', label: t('common.status'), type: 'select' as const, defaultValue: value.status, options: ['active', 'disabled'].map((item) => ({ value: item, label: t(`m3.${item}`) })) }] : []),
  ]
}

function Section({ title, action, children }: { title: string; action?: React.ReactNode; children?: React.ReactNode }) { return <Card className="overflow-hidden"><div className="flex min-h-14 items-center justify-between border-b border-border px-5"><h2 className="text-sm font-semibold">{title}</h2>{action}</div><div className="px-5">{children || <p className="py-8 text-center text-xs text-muted-foreground">—</p>}</div></Card> }
function Row({ title, detail, status, action }: { title: string; detail: string; status: string; action?: React.ReactNode }) { return <div aria-label={title} className="flex min-h-16 items-center border-t border-border first:border-0" role="group"><div><p className="text-xs font-medium">{title}</p><p className="mt-1 text-[11px] text-muted-foreground">{detail}</p></div><div className="flex-1" /><span className="text-[11px] text-muted-foreground">{status}</span>{action && <div className="ml-2">{action}</div>}</div> }
