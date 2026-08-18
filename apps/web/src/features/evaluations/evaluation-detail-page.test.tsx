import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { i18n } from '../../app/i18n'
import { ToastProvider } from '../../shared/ui/toast'
import { EvaluationDetailPage } from './evaluation-detail-page'

describe('evaluation details', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('zh-CN')
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      run: {
        id: 'evaluation-1', name: 'Runtime evaluation', workflowVersionId: 'version-1', workflowName: 'Runtime workflow',
        datasetVersionId: 'dataset-version-1', datasetName: 'Runtime dataset', evaluationProfileVersionId: 'profile-version-1',
        visibility: 'company', ownerDepartmentId: 'department-1', status: 'completed', resultCount: 0, parameters: {}, createdAt: '2026-08-17T00:00:00Z',
      },
      results: [],
      metrics: { passRate: 1, averageScore: 0.75, totalCostMicros: 12 },
      reportStatus: 'completed',
    }), { headers: { 'Content-Type': 'application/json' } })))
  })

  it('does not crash if a stale projection returns the typed Runtime metrics object', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><ToastProvider><MemoryRouter initialEntries={['/evaluations/evaluation-1']}><Routes><Route element={<EvaluationDetailPage />} path="/evaluations/:id" /></Routes></MemoryRouter></ToastProvider></QueryClientProvider>)

    expect(await screen.findByRole('heading', { name: 'Runtime evaluation' })).toBeVisible()
    expect(screen.getByText('100.0%')).toBeVisible()
    expect(screen.getByText('0.750')).toBeVisible()
  })
})
