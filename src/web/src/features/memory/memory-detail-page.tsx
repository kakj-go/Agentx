import { useMutation, useQuery } from '@tanstack/react-query'
import { Activity } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest } from '../../shared/api/client'
import type { ExternalConnection, HealthCheck, MemoryResource } from '../../shared/api/types'
import { ResourceDetailLayout } from '../../shared/components/resource-detail-layout'
import { localizedValue } from '../../shared/lib/localized-value'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { useToast } from '../../shared/ui/toast'

export function MemoryDetailPage() {
  const { id = '' } = useParams(); const { t } = useTranslation(); const auth = useAuth(); const { showToast } = useToast(); const [health, setHealth] = useState<HealthCheck>()
  const resource = useQuery({ queryKey: ['memory', id], queryFn: () => apiRequest<MemoryResource>(`/memory/namespaces/${id}`) })
  const connections = useQuery({ queryKey: ['memory-connections'], queryFn: () => apiRequest<ExternalConnection[]>('/memory/connections') })
  const test = useMutation({ mutationFn: () => apiRequest<HealthCheck>(`/memory/connections/${resource.data?.connectionId}/test-connection`, { method: 'POST' }), onSuccess: (value) => { setHealth(value); showToast(localizedValue(t, 'memory', value.status)) }, onError: (error: Error) => showToast(error.message) })
  const value = resource.data; const connection = connections.data?.find((item) => item.id === value?.connectionId)
  return <ResourceDetailLayout actions={auth.hasPermission('memory:manage') ? <Button disabled={test.isPending} onClick={() => test.mutate()} variant="secondary"><Activity className="size-4" />{t('memory.connectionTest')}</Button> : undefined} description={t('memory.description')} details={value ? [{ label: t('memory.connectionName'), value: value.connectionName }, { label: t('memory.endpoint'), value: connection?.endpoint }, { label: t('memory.healthPath'), value: connection?.healthPath }, { label: t('memory.namespace'), value: value.externalNamespace }, { label: t('memory.accessMode'), value: localizedValue(t, 'memory.accessModes', value.accessMode) }, { label: t('memory.department'), value: value.ownerDepartmentId }] : []} error={resource.error} loading={resource.isLoading} name={value?.name} status={value?.status}>{health && <Card className="p-4 text-xs text-muted-foreground">{localizedValue(t, 'memory', health.status)} · {health.latencyMs ?? 0} ms{health.errorMessage ? ` · ${health.errorMessage}` : ''}</Card>}</ResourceDetailLayout>
}
