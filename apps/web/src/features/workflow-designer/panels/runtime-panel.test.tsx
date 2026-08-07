import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ToastProvider } from '../../../shared/ui/toast'
import { RuntimePanel } from './runtime-panel'

describe('RuntimePanel execution rail', () => {
  it('collapses to 40px, expands, and switches between Workflow executions', async () => {
    const onExecutionChange = vi.fn()
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ items: [{ id: 'execution-1', workflowId: 'workflow-1', workflowName: 'Workflow', status: 'succeeded', startedAt: '2026-08-07T01:00:00Z' }], page: 1, pageSize: 100, total: 1 }), { headers: { 'Content-Type': 'application/json' } })))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><ToastProvider><RuntimePanel events={[]} onExecutionChange={onExecutionChange} workflowId="workflow-1" /></ToastProvider></QueryClientProvider>)

    const expand = screen.getByRole('button', { name: /Expand execution rail|展开执行轨道/ })
    expect(expand.closest('section')).toHaveClass('h-10')
    fireEvent.click(expand)
    expect(screen.getByRole('tab', { name: /Events|事件/ })).toBeInTheDocument()
    const select = await screen.findByRole('combobox', { name: /Switch execution|切换 Execution/ })
    await waitFor(() => expect(select).not.toBeDisabled())
    fireEvent.click(select)
    fireEvent.click(await screen.findByRole('option', { name: /succeeded/ }))
    expect(onExecutionChange).toHaveBeenCalledWith('execution-1')
  })
})
