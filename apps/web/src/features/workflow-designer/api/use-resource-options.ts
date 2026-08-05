import { useQueries } from '@tanstack/react-query'

import { apiRequest } from '../../../shared/api/client'
import type { PageResponse } from '../../../shared/api/types'
import type { ResourceOption, ResourceType } from '../model/types'

type ResourceRecord = { id: string; name?: string; alias?: string; title?: string; modelName?: string; versionId?: string | null; currentVersionId?: string | null; currentDeploymentId?: string | null; enabled?: boolean; availability?: string; status?: string }

const endpoints: Partial<Record<ResourceType, string>> = {
  credential: '/credentials?pageSize=100&status=active', model: '/models/aliases?pageSize=100&status=active', mcp_tool: '/mcp/tools', skill: '/skills?pageSize=100&status=active', rag: '/knowledge/resources?pageSize=100&status=active', memory: '/memory/namespaces?pageSize=100&status=active', sandbox_profile: '/sandbox-profiles?pageSize=100&status=active',
}

export function useResourceOptions() {
  const keys = Object.keys(endpoints) as ResourceType[]
  const queries = useQueries({ queries: keys.map((resourceType) => ({ queryKey: ['studio-resources', resourceType], queryFn: async () => normalize(await apiRequest<unknown>(endpoints[resourceType]!), resourceType), staleTime: 30_000, retry: false })) })
  const options = Object.fromEntries(keys.map((key, index) => [key, queries[index].data ?? []])) as Partial<Record<ResourceType, ResourceOption[]>>
  return { options, loading: queries.some((query) => query.isLoading), errors: queries.filter((query) => query.error).map((query) => query.error) }
}

function normalize(value: unknown, resourceType: ResourceType): ResourceOption[] {
  const items = Array.isArray(value) ? value : ((value as PageResponse<ResourceRecord> | undefined)?.items ?? [])
  return (items as ResourceRecord[]).filter((item) => item.enabled !== false && item.availability !== 'unavailable' && item.status !== 'disabled').map((item) => ({ value: item.id, label: item.alias ? `${item.alias}${item.modelName ? ` · ${item.modelName}` : ''}` : item.title ? `${item.title}${item.name ? ` · ${item.name}` : ''}` : item.name ?? item.id, versionId: item.currentVersionId ?? item.versionId ?? (resourceType === 'model' ? item.currentDeploymentId : undefined) }))
}
