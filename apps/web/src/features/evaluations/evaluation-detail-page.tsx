import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Ban, Play } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router-dom'

import { apiRequest } from '../../shared/api/client'
import type { EvaluationReport } from '../../shared/api/types'
import { ApiClientError } from '../../shared/api/client'
import { EmptyState } from '../../shared/components/empty-state'
import { ConfirmDialog } from '../../shared/components/confirm-dialog'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { useToast } from '../../shared/ui/toast'

export function EvaluationDetailPage() {
  const { id = '' } = useParams()
  const { t } = useTranslation()
  const { showToast } = useToast()
  const queryClient = useQueryClient()
  const [confirmCancel, setConfirmCancel] = useState(false)
  const report = useQuery({ queryKey: ['evaluation-report', id], queryFn: () => apiRequest<EvaluationReport>(`/evaluations/${id}/report`) })
  const action = useMutation({ mutationFn: (name: 'start' | 'cancel') => apiRequest(`/evaluations/${id}/${name}`, { method: 'POST' }), onSuccess: async () => { setConfirmCancel(false); await queryClient.invalidateQueries({ queryKey: ['evaluation-report', id] }) }, onError: (error: Error) => showToast(error instanceof ApiClientError && error.detail.code === 'RUNTIME_UNAVAILABLE' ? t('m3.runtimeUnavailable') : error.message) })
  if (!report.data) return <PageContainer><EmptyState title={report.isLoading ? t('m2.loading') : t('m2.loadFailed')} description={String(report.error ?? '')} /></PageContainer>
  const run = report.data.run
  return <PageContainer><PageHeader action={run.status === 'created' ? <div className="flex gap-2"><Button onClick={() => action.mutate('start')}><Play className="size-4" />{t('m3.start')}</Button><Button onClick={() => setConfirmCancel(true)} variant="secondary"><Ban className="size-4" />{t('common.cancel')}</Button></div> : undefined} description={`${run.workflowName} · ${run.datasetName}`} title={run.name} /><div className="mt-5"><StatusBadge status={run.status === 'created' ? 'draft' : run.status === 'cancelled' ? 'inactive' : run.status as 'running' | 'completed' | 'failed'} /></div><Card className="mt-6 p-8"><EmptyState title={t('m3.notRun')} description={t('m3.notRunDescription')} /><div className="mt-6 grid grid-cols-3 gap-4 border-t border-border pt-6 text-xs"><Info label={t('m3.results')} value={String(run.resultCount)} /><Info label={t('m3.metrics')} value={String(report.data.metrics.length)} /><Info label={t('m3.reportStatus')} value={report.data.reportStatus} /></div></Card><ConfirmDialog cancelLabel={t('common.cancel')} confirmLabel={t('common.confirm')} description={t('m3.confirmCancel')} onClose={() => setConfirmCancel(false)} onConfirm={() => action.mutateAsync('cancel').then(() => undefined)} open={confirmCancel} pending={action.isPending} title={t('common.cancel')} /></PageContainer>
}
function Info({ label, value }: { label: string; value: string }) { return <div><p className="text-muted-foreground">{label}</p><p className="mt-2 font-semibold">{value}</p></div> }
