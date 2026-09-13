import { describe, expect, it } from 'vitest'

import type { TraceSpan } from '../../shared/api/types'
import { buildTraceRows } from './trace-model'

const spans = [
  span('root', null, 'execution', 'Workflow', 'succeeded', 0),
  span('node', 'root', 'node', 'Agent node', 'succeeded', 10),
  span('attempt', 'node', 'attempt', 'Attempt 1', 'succeeded', 20),
  span('call', 'attempt', 'runtime_call', 'Model call', 'failed', 30),
]

describe('Trace waterfall tree', () => {
  it('orders parents before children and hides collapsed descendants', () => {
    expect(buildTraceRows(spans, new Set()).map(({ span, depth }) => [span.spanId, depth])).toEqual([
      ['root', 0], ['node', 1], ['attempt', 2], ['call', 3],
    ])
    expect(buildTraceRows(spans, new Set(['node'])).map(({ span }) => span.spanId)).toEqual(['root', 'node'])
  })

  it('keeps ancestor context for search and error filters', () => {
    expect(buildTraceRows(spans, new Set(), 'Model').map(({ span }) => span.spanId)).toEqual(['root', 'node', 'attempt', 'call'])
    expect(buildTraceRows(spans, new Set(), '', 'all', true).map(({ span }) => span.spanId)).toEqual(['root', 'node', 'attempt', 'call'])
  })

  it('searches the human-readable error message while retaining ancestors', () => {
    const failed = { ...spans[3], errorCode: 'MODEL_FAILED', errorMessage: 'Provider quota exhausted' }
    expect(buildTraceRows([...spans.slice(0, 3), failed], new Set(), 'quota exhausted').map(({ span }) => span.spanId)).toEqual(['root', 'node', 'attempt', 'call'])
  })
})

function span(spanId: string, parentSpanId: string | null, spanKind: TraceSpan['spanKind'], spanName: string, status: string, offset: number): TraceSpan {
  return { spanId, parentSpanId, spanKind, spanName, status, startedAt: new Date(Date.UTC(2026, 7, 19, 0, 0, 0, offset)).toISOString(), endedAt: null, durationMs: null, costMicros: 0, hasDetails: false }
}
