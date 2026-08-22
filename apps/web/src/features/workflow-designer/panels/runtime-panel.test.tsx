import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { i18n } from '../../../app/i18n'
import { ToastProvider } from '../../../shared/ui/toast'
import { RuntimePanel } from './runtime-panel'

describe('RuntimePanel execution rail', () => {
  beforeEach(async () => { vi.stubGlobal('innerHeight', 900); await i18n.changeLanguage('zh-CN') })
  afterEach(async () => { vi.unstubAllGlobals(); await i18n.changeLanguage('zh-CN') })

  it('collapses to 40px, expands, and switches between Workflow executions', async () => {
    const onExecutionChange = vi.fn()
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ items: [{ id: 'execution-1', workflowId: 'workflow-1', workflowName: 'Workflow', status: 'succeeded', startedAt: '2026-08-07T01:00:00Z' }], limit: 100, total: 1 }), { headers: { 'Content-Type': 'application/json' } })))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><ToastProvider><RuntimePanel events={[{ sequence: 1, eventType: 'runtime_call.started', status: 'reserved', summary: {}, occurredAt: '2026-08-21T04:59:47Z' }]} onExecutionChange={onExecutionChange} workflowId="workflow-1" /></ToastProvider></QueryClientProvider>)

    const expand = screen.getByRole('button', { name: /Expand execution rail|展开执行轨道/ })
    expect(expand.closest('section')).toHaveClass('h-10')
    fireEvent.click(expand)
    expect(screen.getByRole('tab', { name: /Events|事件/ })).toBeInTheDocument()
    expect(screen.getByTestId('execution-events')).toHaveTextContent('#1')
    expect(screen.getByTestId('execution-events')).toHaveTextContent('已预留')
    expect(screen.getByTestId('execution-events')).not.toHaveTextContent('未知（reserved）')
    fireEvent.click(screen.getByRole('tab', { name: 'Trace' }))
    expect(screen.getByTestId('runtime-rail')).toHaveStyle({ height: '560px' })
    const select = await screen.findByRole('combobox', { name: /切换执行/ })
    await waitFor(() => expect(select).not.toBeDisabled())
    expect(screen.getByTestId('execution-toolbar')).toHaveClass('border-y', 'px-3')
    expect(screen.getByTestId('runtime-rail')).not.toHaveClass('overflow-hidden')
    expect(screen.getByRole('button', { name: /调整运行面板高度/ })).toHaveClass('-top-2', 'z-30')
    expect(screen.getByTestId('execution-trace')).toHaveClass('min-h-0', 'flex-1', 'overflow-hidden')
    expect((fetch as ReturnType<typeof vi.fn>).mock.calls.some(([request]) => String(request).includes('/executions?workflowIds=workflow-1&limit=100'))).toBe(true)
    fireEvent.click(select)
    fireEvent.click(await screen.findByRole('option', { name: /成功/ }))
    expect(onExecutionChange).toHaveBeenCalledWith('execution-1')
  })

  it('does not shrink a manually enlarged rail when Trace opens', () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ items: [] }), { headers: { 'Content-Type': 'application/json' } })))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><ToastProvider><RuntimePanel events={[]} executionId="execution-1" onExecutionChange={vi.fn()} workflowId="workflow-1" /></ToastProvider></QueryClientProvider>)

    fireEvent.pointerDown(screen.getByRole('button', { name: /调整运行面板高度/ }), { clientY: 600 })
    fireEvent.pointerMove(window, { clientY: 200 })
    const enlargedHeight = screen.getByTestId('runtime-rail').style.height
    expect(Number.parseFloat(enlargedHeight)).toBeGreaterThan(560)
    fireEvent.click(screen.getByRole('tab', { name: 'Trace' }))
    expect(screen.getByTestId('runtime-rail').style.height).toBe(enlargedHeight)
    fireEvent.pointerUp(window)
  })

  it('allows the rail to reach four fifths of the viewport height', () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ items: [] }), { headers: { 'Content-Type': 'application/json' } })))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><ToastProvider><RuntimePanel events={[]} executionId="execution-1" onExecutionChange={vi.fn()} workflowId="workflow-1" /></ToastProvider></QueryClientProvider>)

    fireEvent.pointerDown(screen.getByRole('button', { name: /调整运行面板高度/ }), { clientY: 800 })
    fireEvent.pointerMove(window, { clientY: 0 })
    expect(screen.getByTestId('runtime-rail')).toHaveStyle({ height: '720px' })
    fireEvent.pointerUp(window)
  })

  it('gives every runtime tab a bounded scroll container', () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ items: [] }), { headers: { 'Content-Type': 'application/json' } })))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><ToastProvider><RuntimePanel events={[]} executionId="execution-1" onExecutionChange={vi.fn()} workflowId="workflow-1" /></ToastProvider></QueryClientProvider>)

    expect(screen.getByTestId('execution-events')).toHaveClass('min-h-0', 'flex-1', 'overflow-auto')
    expect(screen.getByTestId('execution-checkpoints')).toHaveClass('min-h-0', 'flex-1', 'overflow-auto')
    fireEvent.click(screen.getByRole('tab', { name: 'Trace' }))
    expect(screen.getByTestId('execution-trace')).toHaveClass('min-h-0', 'flex-1')
  })
})
