import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Archive, GitBranch, Play, Plus, ShieldCheck, UsersRound } from 'lucide-react'
import { useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate, useParams } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { PageResponse, ResourceValidation, User, Workflow, WorkflowDeployment, WorkflowEnvironment, WorkflowMember, WorkflowVersion } from '../../shared/api/types'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { PrerequisiteAction } from '../../shared/components/prerequisite-action'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { useToast } from '../../shared/ui/toast'

type ExecutionCommandResponse = { executionId: string; status: string; replayed: boolean }

export function WorkflowDetailPage() {
  const { workflowId = '' } = useParams()
  const { t } = useTranslation()
  const auth = useAuth()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const [publishOpen, setPublishOpen] = useState(false)
  const [memberOpen, setMemberOpen] = useState(false)
  const workflow = useQuery({ queryKey: ['workflow', workflowId], queryFn: () => apiRequest<Workflow>(`/workflows/${workflowId}`) })
  const versions = useQuery({ queryKey: ['workflow-versions', workflowId], queryFn: () => apiRequest<WorkflowVersion[]>(`/workflows/${workflowId}/versions`) })
  const deployments = useQuery({ queryKey: ['workflow-deployments', workflowId], queryFn: () => apiRequest<WorkflowDeployment[]>(`/workflows/${workflowId}/deployments`) })
  const environments = useQuery({ queryKey: ['environments'], queryFn: () => apiRequest<WorkflowEnvironment[]>('/environments') })
  const members = useQuery({ queryKey: ['workflow-members', workflowId], queryFn: () => apiRequest<WorkflowMember[]>(`/workflows/${workflowId}/members`) })
  const validation = useQuery({ queryKey: ['workflow-validation', workflowId], queryFn: () => apiRequest<ResourceValidation>(`/workflows/${workflowId}/resource-validation`) })
  const users = useQuery({ queryKey: ['users', 'workflow-member'], queryFn: () => apiRequest<PageResponse<User>>('/users?pageSize=100') })
  const invalidate = () => Promise.all([
    queryClient.invalidateQueries({ queryKey: ['workflow', workflowId] }), queryClient.invalidateQueries({ queryKey: ['workflow-versions', workflowId] }),
    queryClient.invalidateQueries({ queryKey: ['workflow-deployments', workflowId] }), queryClient.invalidateQueries({ queryKey: ['workflow-members', workflowId] }), queryClient.invalidateQueries({ queryKey: ['workflow-validation', workflowId] }),
  ])
  const createVersion = useMutation({ mutationFn: () => apiRequest<WorkflowVersion>(`/workflows/${workflowId}/versions`, { method: 'POST', headers: { 'Idempotency-Key': crypto.randomUUID() }, body: jsonBody({ draftRevision: workflow.data?.draftRevision ?? 0 }) }), onSuccess: async () => { await invalidate(); showToast(t('m2.created')) }, onError: (error: Error) => showToast(error.message) })
  const runVersion = useMutation({
    mutationFn: (versionId: string) => apiRequest<ExecutionCommandResponse>(`/workflow-versions/${versionId}/executions`, { method: 'POST', body: jsonBody({ input: {}, idempotencyKey: crypto.randomUUID() }) }),
    onSuccess: (execution) => navigate(`/executions/${execution.executionId}`),
    onError: (error: Error) => showToast(error.message),
  })
  const archive = useMutation({ mutationFn: () => apiRequest(`/workflows/${workflowId}/archive`, { method: 'POST' }), onSuccess: async () => { await invalidate(); navigate('/workflows') }, onError: (error: Error) => showToast(error.message) })
  const publish = async (values: Record<string, string>) => { await apiRequest(`/workflows/${workflowId}/deployments`, { method: 'POST', headers: { 'Idempotency-Key': crypto.randomUUID() }, body: jsonBody({ environmentId: values.environment, workflowVersionId: values.version }) }); await invalidate() }
  const rollback = async (deployment: WorkflowDeployment) => { await apiRequest(`/workflows/${workflowId}/deployments/${deployment.environmentId}/rollback`, { method: 'POST', headers: { 'Idempotency-Key': crypto.randomUUID() }, body: jsonBody({ targetWorkflowVersionId: deployment.workflowVersionId }) }); await invalidate(); showToast(t('m2.saved')) }
  const addMember = async (values: Record<string, string>) => { await apiRequest(`/workflows/${workflowId}/members`, { method: 'POST', body: jsonBody({ userId: values.user, memberRole: values.role }) }); await invalidate() }
  const value = workflow.data
  if (workflow.isLoading) return <PageContainer><p className="text-sm text-muted-foreground">{t('m2.loading')}</p></PageContainer>
  if (!value) return <PageContainer><p className="text-sm text-danger">{String(workflow.error ?? t('m2.loadFailed'))}</p></PageContainer>
  const publishFields: EntityFormField[] = [
    { name: 'environment', label: t('m2.selectEnvironment'), type: 'select', required: true, options: (environments.data ?? []).filter((item) => item.status === 'active').map((item) => ({ value: item.id, label: item.name })) },
    { name: 'version', label: t('m2.selectVersion'), type: 'select', required: true, options: (versions.data ?? []).map((item) => ({ value: item.id, label: `v${item.versionNumber}` })) },
  ]
  const memberFields: EntityFormField[] = [
    { name: 'user', label: t('header.user'), type: 'select', options: (users.data?.items ?? []).map((item) => ({ value: item.id, label: `${item.displayName} · ${item.username}` })) },
    { name: 'role', label: t('header.roles'), type: 'select', defaultValue: 'viewer', options: ['viewer', 'editor', 'manager'].map((item) => ({ value: item, label: item })) },
  ]
  return <PageContainer>
    <PageHeader action={<div className="flex gap-2"><Button asChild variant="secondary"><Link to={`/workflows/${workflowId}/editor`}><GitBranch className="size-4" />{t('m2.openEditor')}</Link></Button>{auth.hasPermission('workflow:publish') && <PrerequisiteAction description={t('prerequisites.workflowPublishDescription')} loading={versions.isLoading} onReady={() => setPublishOpen(true)} requirements={[{ key: 'version', label: t('prerequisites.workflowVersion'), met: Boolean(versions.data?.length), actionLabel: t('m2.createVersion'), onAction: () => createVersion.mutate() }, { key: 'status', label: t('prerequisites.activeWorkflow'), met: value.status === 'active' }]}><Play className="size-4" />{t('m2.publish')}</PrerequisiteAction>}</div>} description={value.description ?? t('pages.workflows.description')} title={value.name} />
    <div className="mt-5 flex items-center gap-3 text-xs"><StatusBadge status={value.status === 'active' ? 'active' : 'inactive'} /><span>{t('m2.revision', { revision: value.draftRevision })}</span><span>{value.latestVersion ? t('m2.versionNumber', { version: value.latestVersion }) : t('m2.draftLabel')}</span><span className="text-muted-foreground">{value.ownerName}</span></div>
    <div className="mt-6 grid grid-cols-3 gap-5">
      <Card className="col-span-2 p-5"><h2 className="text-sm font-semibold">{t('m2.resources')}</h2><div className={`mt-4 rounded-lg border p-4 text-xs ${validation.data?.valid ? 'border-success/20 bg-success/10 text-success' : 'border-warning/20 bg-warning/10 text-warning'}`}><div className="flex items-center gap-2"><ShieldCheck className="size-4" />{validation.data?.valid ? t('m2.validationPassed') : t('m2.validationFailed', { count: validation.data?.missingGrants.length ?? 0 })}</div>{validation.data?.missingGrants.map((issue) => <p className="mt-2 font-mono text-[11px]" key={`${issue.nodeId}-${issue.resourceId}-${issue.operation}`}>{issue.nodeId}: {issue.resourceType}/{issue.resourceId} · {issue.reason}</p>)}</div></Card>
      <Card className="p-5"><h2 className="text-sm font-semibold">{t('m2.overview')}</h2><dl className="mt-4 space-y-3 text-xs"><Info label={t('m2.visibility')} value={value.visibility} /><Info label="Workflow ID" value={value.id} /><Info label="Service Identity" value={value.serviceIdentityId} /></dl>{auth.hasPermission('workflow:archive') && value.status === 'active' && <Button className="mt-5 w-full" disabled={archive.isPending} onClick={() => { if (window.confirm(t('m2.confirmArchive'))) archive.mutate() }} variant="secondary"><Archive className="size-4" />{t('m2.archive')}</Button>}</Card>
      <SectionCard action={auth.hasPermission('workflow:manage_member') ? <Button onClick={() => setMemberOpen(true)} size="sm" variant="secondary"><Plus className="size-3.5" />{t('m2.addMember')}</Button> : undefined} icon={UsersRound} title={t('m2.members')}>{members.data?.map((member) => <Row key={member.userId} primary={member.displayName} secondary={`${member.username} · ${member.memberRole}`} />)}{members.data?.length === 0 && <Empty />}</SectionCard>
      <SectionCard action={auth.hasPermission('workflow:publish') ? <Button disabled={createVersion.isPending || value.status !== 'active'} onClick={() => createVersion.mutate()} size="sm" variant="secondary"><Plus className="size-3.5" />{t('m2.createVersion')}</Button> : undefined} icon={GitBranch} title={t('m2.versions')}>{versions.data?.map((version) => <div className="flex items-center border-t border-border py-3 first:border-t-0" key={version.id}><div className="min-w-0"><p className="text-xs font-medium">v{version.versionNumber}</p><p className="mt-1 truncate text-[11px] text-muted-foreground">Revision {version.sourceRevision} · {version.contentHash.slice(0, 20)}…</p></div><div className="flex-1" />{auth.hasPermission('execution:run') && <Button disabled={runVersion.isPending} onClick={() => runVersion.mutate(version.id)} size="sm" variant="ghost"><Play className="size-3.5" />{t('m4.runVersion')}</Button>}</div>)}{versions.data?.length === 0 && <Empty />}</SectionCard>
      <SectionCard icon={Play} title={t('m2.deployments')}>{deployments.data?.map((deployment) => <div className="flex items-center border-t border-border py-3 first:border-t-0" key={deployment.id}><div><p className="text-xs font-medium">{deployment.environmentName} · v{deployment.versionNumber}</p><p className="mt-1 text-[11px] text-muted-foreground">{deployment.status} · {deployment.source}</p></div><div className="flex-1" />{auth.hasPermission('workflow:publish') && deployment.status !== 'active' && <Button onClick={() => void rollback(deployment)} size="sm" variant="ghost">{t('m2.rollback')}</Button>}</div>)}{deployments.data?.length === 0 && <Empty />}</SectionCard>
    </div>
    {publishOpen && <EntityFormDialog cancelLabel={t('common.cancel')} fields={publishFields} onClose={() => setPublishOpen(false)} onSubmit={publish} open submitLabel={t('m2.publish')} title={t('m2.publish')} />}
    {memberOpen && <EntityFormDialog cancelLabel={t('common.cancel')} fields={memberFields} onClose={() => setMemberOpen(false)} onSubmit={addMember} open submitLabel={t('common.save')} title={t('m2.addMember')} />}
  </PageContainer>
}

function Info({ label, value }: { label: string; value: string }) { return <div><dt className="text-muted-foreground">{label}</dt><dd className="mt-1 break-all font-mono text-[11px]">{value}</dd></div> }
function Row({ primary, secondary }: { primary: string; secondary: string }) { return <div className="border-t border-border py-3 first:border-t-0"><p className="text-xs font-medium">{primary}</p><p className="mt-1 text-[11px] text-muted-foreground">{secondary}</p></div> }
function Empty() { const { t } = useTranslation(); return <p className="py-4 text-xs text-muted-foreground">{t('m2.noData')}</p> }
function SectionCard({ action, children, icon: Icon, title }: { action?: ReactNode; children: ReactNode; icon: typeof GitBranch; title: string }) { return <Card className="p-5"><div className="flex items-center"><Icon className="mr-2 size-4 text-primary" /><h2 className="text-sm font-semibold">{title}</h2><div className="flex-1" />{action}</div><div className="mt-4">{children}</div></Card> }
