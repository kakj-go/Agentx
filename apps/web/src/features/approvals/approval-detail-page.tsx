import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Check, Hand, RotateCcw, UserRoundCog, X } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { Approval, ApprovalAction } from '../../shared/api/types'
import { ConfirmDialog } from '../../shared/components/confirm-dialog'
import { EmptyState } from '../../shared/components/empty-state'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { useToast } from '../../shared/ui/toast'
import { approvalStatus } from './approval-status'

type Candidate = { userId: string; displayName: string }
type ApprovalActionName = 'claim' | 'release' | 'reassign' | 'approve' | 'reject' | 'cancel'
type TerminalAction = 'approve' | 'reject' | 'cancel' | null

export function ApprovalDetailPage() {
  const { id = '' } = useParams()
  const { t } = useTranslation()
  const auth = useAuth()
  const { showToast } = useToast()
  const queryClient = useQueryClient()
  const [terminalAction, setTerminalAction] = useState<TerminalAction>(null)
  const [reassignOpen, setReassignOpen] = useState(false)
  const approval = useQuery({ queryKey: ['approval', id], queryFn: () => apiRequest<Approval>(`/approvals/${id}`) })
  const actions = useQuery({ queryKey: ['approval-actions', id], queryFn: () => apiRequest<ApprovalAction[]>(`/approvals/${id}/actions`) })
  const candidates = useQuery({ queryKey: ['approval-candidates', id], queryFn: () => apiRequest<Candidate[]>(`/approvals/${id}/candidates`) })
  const refresh = async () => Promise.all([
    queryClient.invalidateQueries({ queryKey: ['approval', id] }),
    queryClient.invalidateQueries({ queryKey: ['approval-actions', id] }),
    queryClient.invalidateQueries({ queryKey: ['approvals'] }),
    queryClient.invalidateQueries({ queryKey: ['notifications'] }),
  ])
  const act = useMutation({
    mutationFn: ({ action, targetUserId }: { action: ApprovalActionName; targetUserId?: string }) => apiRequest<Approval>(`/approvals/${id}/${action}`, { method: 'POST', body: jsonBody({ version: approval.data?.version, ...(action === 'approve' || action === 'reject' ? { input: null } : {}), ...(action === 'reassign' ? { targetUserId } : {}) }) }),
    onSuccess: async () => { setTerminalAction(null); setReassignOpen(false); await refresh(); showToast(t('m3.approvalUpdated')) },
    onError: async (error: Error) => { showToast(error.message); await refresh() },
  })
  if (!approval.data) return <PageContainer><EmptyState title={approval.isLoading ? t('m2.loading') : t('m2.loadFailed')} description={String(approval.error ?? '')} /></PageContainer>
  const value = approval.data
  const mine = value.claimedBy === auth.user?.id
  const active = value.status === 'pending' || value.status === 'claimed'
  const candidateFields: EntityFormField[] = [{ name: 'targetUserId', label: t('m3.assignee'), type: 'select', required: true, options: (candidates.data ?? []).map((item) => ({ value: item.userId, label: item.displayName })) }]
  const actionBar = <div className="flex flex-wrap gap-2">
    {auth.hasPermission('approval:manage') && active && <Button onClick={() => setReassignOpen(true)} variant="secondary"><UserRoundCog className="size-4" />{t('m3.assignee')}</Button>}
    {auth.hasPermission('approval:manage') && active && <Button onClick={() => setTerminalAction('cancel')} variant="secondary"><X className="size-4" />{t('common.cancel')}</Button>}
    {auth.hasPermission('approval:act') && value.status === 'pending' && <Button onClick={() => act.mutate({ action: 'claim' })}><Hand className="size-4" />{t('m3.claim')}</Button>}
    {auth.hasPermission('approval:act') && value.status === 'claimed' && mine && <><Button onClick={() => setTerminalAction('approve')}><Check className="size-4" />{t('m3.approve')}</Button><Button onClick={() => setTerminalAction('reject')} variant="danger"><X className="size-4" />{t('m3.reject')}</Button><Button onClick={() => act.mutate({ action: 'release' })} variant="secondary"><RotateCcw className="size-4" />{t('m3.release')}</Button></>}
  </div>
  return <PageContainer>
    <PageHeader action={active ? actionBar : undefined} description={`${value.workflowName} · ${value.nodeId}`} title={value.title} />
    <div className="mt-5 flex items-center gap-3"><StatusBadge status={approvalStatus(value.status)} /><span className="text-xs text-muted-foreground">{t('m3.resumeStatus')}: {value.resumeStatus}</span></div>
    <div className="mt-6 grid grid-cols-[minmax(0,1.4fr)_minmax(340px,0.6fr)] gap-5"><Card className="p-5"><h2 className="text-sm font-semibold">{t('m3.request')}</h2><p className="mt-3 text-xs leading-6 text-muted-foreground">{value.description ?? '—'}</p><pre className="mt-5 overflow-auto rounded-lg bg-canvas p-4 text-[11px]">{JSON.stringify(value.requestPayload, null, 2)}</pre></Card><Card className="overflow-hidden"><div className="border-b border-border px-5 py-4"><h2 className="text-sm font-semibold">{t('m3.actionHistory')}</h2></div><div className="divide-y divide-border px-5">{actions.data?.map((item) => <div className="py-4" key={item.id}><div className="flex justify-between text-xs"><strong>{item.actorName}</strong><span className="text-muted-foreground">{item.actionType}</span></div><p className="mt-1 text-[11px] text-muted-foreground">{item.fromStatus} → {item.toStatus} · {new Date(item.createdAt).toLocaleString()}</p></div>)}</div></Card></div>
    {reassignOpen && <EntityFormDialog cancelLabel={t('common.cancel')} fields={candidateFields} onClose={() => setReassignOpen(false)} onSubmit={(values) => act.mutateAsync({ action: 'reassign', targetUserId: values.targetUserId }).then(() => undefined)} open submitLabel={t('common.save')} title={t('m3.assignee')} />}
    <ConfirmDialog cancelLabel={t('common.cancel')} confirmLabel={terminalAction ? t(terminalAction === 'cancel' ? 'common.confirm' : `m3.${terminalAction}`) : t('common.confirm')} description={terminalAction === 'reject' ? t('m3.confirmReject') : `${terminalAction === 'approve' ? t('m3.approve') : t('common.cancel')}?`} onClose={() => setTerminalAction(null)} onConfirm={async () => { if (terminalAction) await act.mutateAsync({ action: terminalAction }) }} open={Boolean(terminalAction)} pending={act.isPending} title={terminalAction === 'reject' ? t('m3.reject') : terminalAction === 'approve' ? t('m3.approve') : t('common.cancel')} />
  </PageContainer>
}
