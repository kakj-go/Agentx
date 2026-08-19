import { useInfiniteQuery } from '@tanstack/react-query'
import { useMemo } from 'react'

import { apiRequest } from '../../shared/api/client'
import type { Trace, TraceSpan } from '../../shared/api/types'

export function useExecutionTrace(executionId: string | undefined, enabled = true) {
  const query = useInfiniteQuery({
    queryKey: ['execution-trace-spans', executionId],
    queryFn: ({ pageParam }) => apiRequest<Trace>(`/executions/${executionId}/trace?limit=200${pageParam ? `&cursor=${encodeURIComponent(pageParam)}` : ''}`),
    initialPageParam: '',
    enabled: enabled && Boolean(executionId),
    getNextPageParam: (last) => last.nextCursor ?? undefined,
    refetchInterval: (current) => {
      const pages = current.state.data?.pages
      return pages?.at(-1)?.complete === false || pages?.some((page) => page.spans?.some((span) => !terminal.has(span.status))) ? 3_000 : false
    },
    retry: false,
  })
  const spans = useMemo(() => mergeTracePages(query.data?.pages ?? []), [query.data])
  const latest = query.data?.pages.at(-1)
  return { ...query, spans, trace: latest }
}

export function mergeTracePages(pages: Trace[]) {
  const unique = new Map<string, TraceSpan>()
  for (const page of pages) for (const span of page.spans ?? []) unique.set(span.spanId, span)
  return [...unique.values()].sort((left, right) => new Date(left.startedAt).getTime() - new Date(right.startedAt).getTime() || left.spanId.localeCompare(right.spanId))
}

const terminal = new Set(['succeeded', 'completed', 'failed', 'cancelled', 'timed_out', 'outcome_unknown', 'skipped'])
