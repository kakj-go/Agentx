import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Check, ShieldCheck, X } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useParams } from 'react-router-dom'

import { apiRequest, jsonBody } from '../../shared/api/client'
import type { ResourceGrantRequest } from '../../shared/api/types'
import { ConfirmDialog } from '../../shared/components/confirm-dialog'
import { EmptyState } from '../../shared/components/empty-state'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { StatusBadge } from '../../shared/components/status-badge'
import { useLocaleFormat } from '../../shared/lib/locale-format'
import { localizedValue } from '../../shared/lib/localized-value'
import { Badge } from '../../shared/ui/badge'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { useAuth } from '../../app/providers/auth-provider'
import { useToast } from '../../shared/ui/toast'

type Decision = { reviewId: string; departmentId: string; action: 'approve' | 'reject'; version: number }

function resourceGrantStatusTone(status: string) {
  if (status === 'approved') return 'active' as const
  if (status === 'pending') return 'pending' as const
  return 'inactive' as const
}

export function ResourceGrantRequestDetailPage() {
  const { id = '' } = useParams()
  const { t } = useTranslation()
  const { formatDateTime } = useLocaleFormat()
  const auth = useAuth()
  const { showToast } = useToast()
  const queryClient = useQueryClient()
  const [decision, setDecision] = useState<Decision>()
  const request = useQuery({ queryKey: ['resource-grant-request', id], queryFn: () => apiRequest<ResourceGrantRequest>(`/resource-grant-requests/${id}`), refetchInterval: (query) => query.state.data?.status === 'pending' ? 5000 : false })
  const refresh = async () => Promise.all([
    queryClient.invalidateQueries({ queryKey: ['resource-grant-request', id] }),
    queryClient.invalidateQueries({ queryKey: ['resource-grant-requests'] }),
    queryClient.invalidateQueries({ queryKey: ['notifications'] }),
  ])
  const act = useMutation({
    mutationFn: async (value: Decision) => apiRequest<ResourceGrantRequest>(`/resource-grant-requests/${id}/reviews/${value.departmentId}/${value.action}`, { method: 'POST', headers: { 'Idempotency-Key': crypto.randomUUID() }, body: jsonBody({ expectedVersion: value.version, comment: null }) }),
    onSuccess: async () => { setDecision(undefined); await refresh(); showToast(t('approvals.resourceGrantUpdated')) },
    onError: (error: Error) => showToast(error.message),
  })
  if (!request.data) return <PageContainer><EmptyState title={request.isLoading ? t('common.loading') : t('common.loadFailed')} description={String(request.error ?? '')} /></PageContainer>
  const value = request.data
  const canCancel = value.status === 'pending' && value.requestedBy === auth.user?.id
  return <PageContainer>
    <PageHeader action={<Button asChild size="sm" variant="ghost"><Link to="/approvals?tab=resource-grants">{t('studio.back')}</Link></Button>} description={`${value.workflowName ?? value.subjectDepartmentName ?? value.subjectId} · ${value.requestedByName}`} title={value.primaryResourceName ?? value.primaryResourceId} />
    <div className="mt-5 flex items-center gap-3"><StatusBadge label={localizedValue(t, 'approvals.resourceGrantStatuses', value.status)} status={resourceGrantStatusTone(value.status)} /><Badge tone="neutral">{localizedValue(t, 'resourceGrants.resourceTypes', value.primaryResourceType)}</Badge><span className="text-xs text-muted-foreground">{localizedValue(t, 'resourceGrants.operations', value.operation)}</span>{canCancel && <Button className="ml-auto" onClick={() => void apiRequest(`/resource-grant-requests/${id}/cancel`, { method: 'POST', headers: { 'Idempotency-Key': crypto.randomUUID() }, body: jsonBody({ expectedVersion: value.version }) }).then(refresh)} size="sm" variant="secondary"><X className="size-3.5" />{t('common.cancel')}</Button>}</div>
    <div className="mt-6 grid gap-5 lg:grid-cols-[minmax(0,1.2fr)_minmax(320px,0.8fr)]">
      <Card className="p-5"><div className="flex items-center gap-2"><ShieldCheck className="size-4 text-primary" /><h2 className="text-sm font-semibold">{t('approvals.resourceGrantPackage')}</h2></div><p className="mt-2 text-xs leading-5 text-muted-foreground">{value.message || t('approvals.resourceGrantNoMessage')}</p><div className="mt-5 divide-y divide-border rounded-lg border border-border">{value.items.map((item) => <div className="flex items-center justify-between gap-3 px-4 py-3 text-xs" key={`${item.resourceType}:${item.resourceId}`}><div className="min-w-0"><p className="truncate font-medium">{item.name ?? t('approvals.controlledDependency')}</p><p className="mt-0.5 text-[10px] text-muted-foreground">{localizedValue(t, 'resourceGrants.resourceTypes', item.resourceType)} · {localizedValue(t, 'resourceGrants.operations', item.operation)}</p></div><Badge tone={item.authorized ? 'success' : item.active ? 'warning' : 'danger'}>{item.authorized ? t('approvals.authorized') : item.active ? t('common.pending') : t('studio.inspector.resourceStates.unavailable')}</Badge></div>)}</div></Card>
      <Card className="overflow-hidden"><div className="border-b border-border px-5 py-4"><h2 className="text-sm font-semibold">{t('approvals.resourceGrantReviews')}</h2></div><div className="divide-y divide-border">{value.reviews.map((review) => <div className="space-y-3 px-5 py-4" key={review.id}><div className="flex items-center justify-between gap-3"><div><p className="text-xs font-medium">{review.ownerDepartmentName}</p><p className="mt-1 text-[10px] text-muted-foreground">{review.reviewedByName ?? t('approvals.awaitingReviewer')}</p></div><StatusBadge label={localizedValue(t, 'approvals.resourceGrantStatuses', review.status)} status={resourceGrantStatusTone(review.status)} /></div>{review.canAct && review.status === 'pending' && value.status === 'pending' && <div className="flex gap-2"><Button onClick={() => setDecision({ reviewId: review.id, departmentId: review.ownerDepartmentId, action: 'approve', version: review.version })} size="sm"><Check className="size-3.5" />{t('approvals.approve')}</Button><Button onClick={() => setDecision({ reviewId: review.id, departmentId: review.ownerDepartmentId, action: 'reject', version: review.version })} size="sm" variant="danger"><X className="size-3.5" />{t('approvals.reject')}</Button></div>}</div>)}</div></Card>
    </div>
    <Card className="mt-5 overflow-hidden"><div className="border-b border-border px-5 py-4"><h2 className="text-sm font-semibold">{t('approvals.auditHistory')}</h2></div><div className="divide-y divide-border">{value.history.map((event) => <div className="flex items-center gap-3 px-5 py-3 text-xs" key={event.id}><span className="font-medium">{localizedValue(t, 'approvals.auditActions', event.action)}</span><span className="text-muted-foreground">{event.actorName ?? t('approvals.systemActor')}</span><time className="ml-auto text-[10px] text-muted-foreground">{formatDateTime(event.occurredAt)}</time></div>)}</div></Card>
    <ConfirmDialog cancelLabel={t('common.cancel')} confirmLabel={decision?.action === 'approve' ? t('approvals.approve') : t('approvals.reject')} description={decision?.action === 'reject' ? t('approvals.confirmResourceReject') : t('approvals.confirmApprove')} onClose={() => setDecision(undefined)} onConfirm={async () => { if (decision) await act.mutateAsync(decision) }} open={Boolean(decision)} pending={act.isPending} title={decision?.action === 'approve' ? t('approvals.approve') : t('approvals.reject')} />
  </PageContainer>
}
