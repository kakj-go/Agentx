import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { i18n } from '../../app/i18n'
import { ToastProvider } from '../../shared/ui/toast'
import { WorkflowCanvas } from './workflow-canvas'

const draft = {
  id: 'draft-1', workflowId: 'workflow-1', revision: 1, schemaVersion: '1.0',
  definition: {
    schemaVersion: '1.0', settings: {},
    nodes: [{ id: 'trigger', type: 'manual_trigger', typeVersion: 1, name: 'Manual Trigger', position: { x: 100, y: 100 }, disabled: false, parameters: {}, resourceReferences: [] }],
    connections: [],
  },
  contentHash: 'sha256:test', updatedAt: '2026-08-02T10:00:00Z',
}

function json(value: unknown, status = 200) {
  return new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } })
}

describe('workflow draft editor', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('zh-CN')
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = new URL(String(input), 'http://agentx.test').pathname
      if (path.endsWith('/draft') && init?.method === 'PUT') {
        return json({ code: 'DRAFT_REVISION_CONFLICT', message: 'Draft changed', requestId: 'request-1' }, 409)
      }
      if (path.endsWith('/draft')) return json(draft)
      if (path.endsWith('/workflows/workflow-1')) return json({ id: 'workflow-1', name: 'Conflict Workflow' })
      if (path.endsWith('/mcp/tools')) return json([])
      return json({ items: [], page: 1, pageSize: 100, total: 0 })
    }))
  })

  it('keeps local edits and opens the recovery dialog on a revision conflict', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><ToastProvider><MemoryRouter initialEntries={['/workflows/workflow-1/editor']}><Routes><Route element={<WorkflowCanvas />} path="/workflows/:workflowId/editor" /></Routes></MemoryRouter></ToastProvider></QueryClientProvider>)

    fireEvent.click(await screen.findByRole('button', { name: /添加节点/ }))
    fireEvent.click(screen.getByRole('button', { name: '保存草稿' }))

    expect((await screen.findAllByText('草稿版本冲突')).length).toBeGreaterThan(0)
    expect(screen.getByRole('button', { name: '保留本地修改' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '载入服务器版本' })).toBeInTheDocument()
    await waitFor(() => expect(document.body.textContent).toContain('有未保存修改'))
  })
})
