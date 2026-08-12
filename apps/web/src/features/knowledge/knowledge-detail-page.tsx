import { useMutation, useQuery } from '@tanstack/react-query'
import { Activity } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest } from '../../shared/api/client'
import type { ExternalConnection, HealthCheck, Knowledge } from '../../shared/api/types'
import { ResourceDetailLayout } from '../../shared/components/resource-detail-layout'
import { localizedValue } from '../../shared/lib/localized-value'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { useToast } from '../../shared/ui/toast'

export function KnowledgeDetailPage() {
  const { id = '' } = useParams(); const { t } = useTranslation(); const auth = useAuth(); const { showToast } = useToast(); const [health, setHealth] = useState<HealthCheck>()
  const resource = useQuery({ queryKey: ['knowledge', id], queryFn: () => apiRequest<Knowledge>(`/knowledge/resources/${id}`) })
  const connections = useQuery({ queryKey: ['knowledge-connections'], queryFn: () => apiRequest<ExternalConnection[]>('/knowledge/connections') })
  const test = useMutation({ mutationFn: () => apiRequest<HealthCheck>(`/knowledge/connections/${resource.data?.connectionId}/test-connection`, { method: 'POST' }), onSuccess: (value) => { setHealth(value); showToast(localizedValue(t, 'knowledge', value.status)) }, onError: (error: Error) => showToast(error.message) })
  const value = resource.data; const connection = connections.data?.find((item) => item.id === value?.connectionId)
  return <ResourceDetailLayout actions={auth.hasPermission('knowledge:manage') ? <Button disabled={test.isPending} onClick={() => test.mutate()} variant="secondary"><Activity className="size-4" />{t('knowledge.connectionTest')}</Button> : undefined} description={t('knowledge.description')} details={value ? [{ label: t('knowledge.connectionName'), value: value.connectionName }, { label: t('knowledge.endpoint'), value: connection?.endpoint }, { label: t('knowledge.healthPath'), value: connection?.healthPath }, { label: t('knowledge.externalId'), value: value.externalResourceId }, { label: t('knowledge.sync'), value: localizedValue(t, 'common', value.syncStatus) }, { label: t('knowledge.department'), value: value.ownerDepartmentId }] : []} error={resource.error} loading={resource.isLoading} name={value?.name} status={value?.status}>{health && <Card className="p-4 text-xs text-muted-foreground">{localizedValue(t, 'knowledge', health.status)} · {health.latencyMs ?? 0} ms{health.errorMessage ? ` · ${health.errorMessage}` : ''}</Card>}</ResourceDetailLayout>
}
