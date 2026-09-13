import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { i18n } from '../../app/i18n'
import type { TraceSpan } from '../../shared/api/types'
import { TraceDetail } from './trace-detail'

const span: TraceSpan = { spanId: 'span-1', parentSpanId: null, spanKind: 'runtime_call', spanName: 'Model call', status: 'succeeded', startedAt: '2026-08-19T00:00:00Z', endedAt: '2026-08-19T00:00:01Z', durationMs: 1000, inputTokens: 3, outputTokens: 5, costMicros: 42, hasDetails: true }
const failedSpan: TraceSpan = { ...span, status: 'failed', errorCode: 'MODEL_FAILED', errorMessage: 'Provider quota exhausted' }

describe('Trace Span detail', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('zh-CN')
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      executionId: 'execution-1', traceId: 'trace-1', span,
      contents: [
        { eventId: 'event-1', kind: 'runtime_request', preview: { prompt: 'hello' }, contentRef: 'artifact-1', occurredAt: '2026-08-19T00:00:00Z' },
        { eventId: 'event-2', kind: 'runtime_response', preview: { answer: 'world' }, contentRef: null, occurredAt: '2026-08-19T00:00:01Z' },
      ],
      events: [{ eventId: 'event-1', eventKind: 'started', eventType: 'runtime_call.started', occurredAt: '2026-08-19T00:00:00Z', status: 'running', attributes: { provider: 'fixture' }, contentRef: 'artifact-1', contentKind: 'runtime_request', contentPreview: { prompt: 'hello' } }],
    }), { headers: { 'Content-Type': 'application/json' } })))
  })

  it('offers five detail tabs and loads Artifact only after the user requests it', async () => {
    const download = vi.fn()
    render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><TraceDetail executionId="execution-1" onDownloadArtifact={download} span={span} /></QueryClientProvider>)
    expect(await screen.findAllByRole('tab')).toHaveLength(5)
    fireEvent.click(screen.getByRole('tab', { name: 'Provider / Runtime 请求' }))
    expect(await screen.findByText(/hello/)).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /Artifact/ }))
    expect(download).toHaveBeenCalledWith('artifact-1')
    fireEvent.click(screen.getByRole('tab', { name: '事件' }))
    expect(screen.getByText('runtime_call.started')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('tab', { name: '原始数据' }))
    expect(screen.getByText(/trace-1/)).toBeInTheDocument()
    expect(screen.getByTestId('trace-detail')).toHaveClass('flex', 'h-full', 'min-h-0', 'overflow-hidden')
    expect(screen.getByRole('tabpanel')).toHaveClass('min-h-0', 'flex-1', 'overflow-auto')
  })

  it('shows both the stable error code and readable error message', async () => {
    render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><TraceDetail executionId="execution-1" span={failedSpan} /></QueryClientProvider>)
    expect(await screen.findByText('MODEL_FAILED')).toBeInTheDocument()
    expect(screen.getByText('Provider quota exhausted')).toBeInTheDocument()
  })

  it('does not invent input or output tabs for a lifecycle-only span', () => {
    const lifecycle = { ...span, spanId: 'span-lifecycle', spanKind: 'boundary' as const, spanName: 'Start', hasDetails: false }
    render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><TraceDetail executionId="execution-1" span={lifecycle} /></QueryClientProvider>)
    expect(screen.getAllByRole('tab')).toHaveLength(2)
    expect(screen.getByText('这是生命周期 Span，没有业务内容。')).toBeInTheDocument()
    expect(screen.queryByRole('tab', { name: '工作流输入' })).not.toBeInTheDocument()
    expect(vi.mocked(fetch)).not.toHaveBeenCalledWith(expect.stringContaining('/trace/spans/'), expect.anything())
  })
})
