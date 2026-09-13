import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { apiRequest, jsonBody } from '../../shared/api/client'
import type { ResourceOption } from '../workflow-designer/model/types'

type DependencyType = 'credential' | 'sandbox_profile'
type ResourceOptionPage = {
  items: Array<{
    id: string
    name: string
    detail: string
    status: string
    resourceVersionId?: string | null
    accessState: ResourceOption['accessState']
    pendingRequestId?: string | null
    requirements?: ResourceOption['requirements']
  }>
  page: number
  pageSize: number
  total: number
}

export function useMcpDependencyOptions(departmentId: string, resourceType: DependencyType) {
  const queryClient = useQueryClient()
  const queryKey = ['mcp-dependency-options', departmentId, resourceType] as const
  const query = useQuery({
    queryKey,
    queryFn: () => loadMcpDependencyOptions(departmentId, resourceType),
    enabled: Boolean(departmentId),
    retry: false,
    staleTime: 10_000,
    refetchInterval: (value) => value.state.data?.some((item) => item.accessState === 'pending') ? 5_000 : false,
  })
  const authorize = useMutation({
    mutationFn: (option: ResourceOption) => apiRequest(`/departments/${departmentId}/resource-authorizations`, {
      method: 'POST',
      headers: { 'Idempotency-Key': crypto.randomUUID() },
      body: jsonBody({ resourceType, resourceId: option.value, resourceVersionId: option.versionId ?? null, operation: 'use' }),
    }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey }),
  })
  const request = useMutation({
    mutationFn: ({ option, message }: { option: ResourceOption; message?: string }) => apiRequest(`/departments/${departmentId}/resource-grant-requests`, {
      method: 'POST',
      headers: { 'Idempotency-Key': crypto.randomUUID() },
      body: jsonBody({ resourceType, resourceId: option.value, resourceVersionId: option.versionId ?? null, operation: 'use', message }),
    }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey }),
  })
  return {
    options: query.data ?? [],
    loading: query.isLoading,
    error: query.error,
    authorize: (option: ResourceOption) => authorize.mutateAsync(option).then(() => undefined),
    request: (option: ResourceOption, message?: string) => request.mutateAsync({ option, message }).then(() => undefined),
  }
}

export async function loadMcpDependencyOptions(departmentId: string, resourceType: DependencyType) {
  const options: ResourceOption[] = []
  let page = 1
  while (true) {
    const response = await apiRequest<ResourceOptionPage>(`/departments/${departmentId}/resource-options?resourceType=${resourceType}&operation=use&page=${page}&pageSize=100`)
    options.push(...response.items.map((item) => ({
      value: item.id,
      label: item.name,
      detail: item.detail,
      status: item.status,
      resourceType,
      operation: 'use' as const,
      versionId: item.resourceVersionId,
      accessState: item.accessState,
      pendingRequestId: item.pendingRequestId,
      requirements: item.requirements,
    })))
    if (options.length >= response.total || response.items.length === 0) return options
    page += 1
  }
}
