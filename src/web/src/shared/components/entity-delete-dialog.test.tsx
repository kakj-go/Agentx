import * as TooltipPrimitive from '@radix-ui/react-tooltip'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { i18n } from '../../app/i18n'
import { EntityDeleteButton } from './entity-delete-button'
import { EntityDeleteDialog } from './entity-delete-dialog'

describe('safe entity deletion', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
    vi.unstubAllGlobals()
  })

  it('hides actions without permission and disables immutable entities', () => {
    const { rerender } = render(<TooltipPrimitive.Provider><EntityDeleteButton canDelete={false} deletePath="/roles/role-1" entityId="role-1" entityName="Member" entityType="role" onDeleted={vi.fn()} /></TooltipPrimitive.Provider>)
    expect(screen.queryByRole('button')).not.toBeInTheDocument()

    rerender(<TooltipPrimitive.Provider><EntityDeleteButton canDelete deletePath="/roles/role-1" entityId="role-1" entityName="Member" entityType="role" immutableReason="Built-in role" onDeleted={vi.fn()} /></TooltipPrimitive.Provider>)
    const immutableButton = screen.getByRole('button', { name: 'Built-in role' })
    expect(immutableButton).toBeDisabled()
    expect(immutableButton).toHaveTextContent('Delete')
    expect(immutableButton.querySelector('svg')).not.toBeInTheDocument()
  })

  it('deletes after a clear preflight and invokes the refresh callback', async () => {
    const deleted = vi.fn()
    const closed = vi.fn()
    const fetchMock = vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => init?.method === 'DELETE'
      ? new Response(null, { status: 204 })
      : jsonResponse(impact({ deletable: true, targetVersion: 3 })))
    vi.stubGlobal('fetch', fetchMock)
    renderDialog({ onClose: closed, onDeleted: deleted })

    const confirm = await screen.findByRole('button', { name: 'Delete' })
    await waitFor(() => expect(confirm).toBeEnabled())
    fireEvent.click(confirm)

    await waitFor(() => expect(deleted).toHaveBeenCalledOnce())
    expect(closed).toHaveBeenCalledOnce()
    expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('/credentials/credential-1?expectedVersion=3'), expect.objectContaining({ method: 'DELETE' }))
  })

  it('shows references by module and loads subsequent pages', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      return jsonResponse(impact({
        total: 2,
        page: url.includes('page=2') ? 2 : 1,
        references: url.includes('page=2')
          ? [reference({ sourceId: 'version-2', sourceName: 'Agent v2', sourceType: 'workflow_version', immutable: true })]
          : [reference({ sourceId: 'workflow-1', sourceName: 'Support agent', nodeName: 'LLM', parentId: 'workflow-1' })],
      }))
    })
    vi.stubGlobal('fetch', fetchMock)
    renderDialog()

    expect(await screen.findByText('Support agent')).toBeInTheDocument()
    expect(screen.getByText(/LLM/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Delete' })).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'Load more' }))
    expect(await screen.findByText('Agent v2')).toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('page=2'), expect.anything())
  })

  it('replaces a stale clear preflight with pageable references returned by a 409', async () => {
    const closed = vi.fn()
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      if (String(input).includes('page=2')) return jsonResponse(impact({ total: 2, page: 2, references: [reference({ sourceId: 'draft-2', sourceName: 'Second workflow reference' })] }))
      if (init?.method !== 'DELETE') return jsonResponse(impact({ deletable: true, targetVersion: 1 }))
      return jsonResponse({
        code: 'ENTITY_IN_USE',
        message: 'Entity is in use',
        requestId: 'request-1',
        details: impact({ total: 2, references: [reference({ sourceName: 'New workflow reference' })] }),
      }, 409)
    })
    vi.stubGlobal('fetch', fetchMock)
    renderDialog({ onClose: closed })

    const confirm = await screen.findByRole('button', { name: 'Delete' })
    await waitFor(() => expect(confirm).toBeEnabled())
    fireEvent.click(confirm)

    expect(await screen.findByText('New workflow reference')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Delete' })).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'Load more' }))
    expect(await screen.findByText('Second workflow reference')).toBeInTheDocument()
    expect(closed).not.toHaveBeenCalled()
  })
})

function renderDialog(overrides: Partial<Parameters<typeof EntityDeleteDialog>[0]> = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const props = {
    deletePath: '/credentials/credential-1',
    entityId: 'credential-1',
    entityName: 'Provider key',
    entityType: 'credential',
    onClose: vi.fn(),
    onDeleted: vi.fn(),
    open: true,
    ...overrides,
  }
  return render(<QueryClientProvider client={queryClient}><MemoryRouter><EntityDeleteDialog {...props} /></MemoryRouter></QueryClientProvider>)
}

function impact(overrides: Record<string, unknown> = {}) {
  return { deletable: false, targetVersion: 1, total: 0, page: 1, pageSize: 20, references: [], ...overrides }
}

function reference(overrides: Record<string, unknown> = {}) {
  return { sourceModule: 'workflows', sourceType: 'workflow_draft', sourceId: 'draft-1', sourceName: 'Draft', relation: 'draft_resource', immutable: false, ...overrides }
}

function jsonResponse(value: unknown, status = 200) {
  return new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } })
}
