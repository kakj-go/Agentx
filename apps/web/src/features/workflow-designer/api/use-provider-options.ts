import { useQueries } from '@tanstack/react-query'

import { apiRequest } from '../../../shared/api/client'
import type { NodeManifest, ResourceOption } from '../model/types'

type ProviderResponse = { items: Array<{ value: string; label: string; description?: string | null }> }

export function useProviderOptions(manifest?: NodeManifest) {
  const fields = Object.entries(manifest?.uiSchema.fields ?? {}).filter((entry) => entry[1].control === 'provider_options' && entry[1].provider)
  const queries = useQueries({
    queries: fields.map(([, ui]) => ({
      queryKey: ['node-provider-options', manifest?.nodeType, manifest?.version, ui.provider],
      queryFn: () => apiRequest<ProviderResponse>(`/node-definitions/${encodeURIComponent(manifest!.nodeType)}/versions/${manifest!.version}/providers/${encodeURIComponent(ui.provider!)}`),
      enabled: Boolean(manifest && ui.provider),
      staleTime: 30_000,
      retry: false,
      select: (response: ProviderResponse): ResourceOption[] => response.items.map((item) => ({ value: item.value, label: item.label })),
    })),
  })
  return Object.fromEntries(fields.map(([field], index) => [field, queries[index]?.data ?? []])) as Record<string, ResourceOption[]>
}
