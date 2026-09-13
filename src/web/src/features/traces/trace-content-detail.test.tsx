import { render, screen } from '@testing-library/react'
import { afterEach, expect, it, vi } from 'vitest'

import { ThemeProvider } from '../../app/providers/theme-provider'
import { TraceContents } from './trace-content-detail'

afterEach(() => vi.unstubAllGlobals())

it('keeps standard plugin Trace data visible when the frozen renderer is missing', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ code: 'PLUGIN_RENDERER_MISSING' }), { status: 404, headers: { 'Content-Type': 'application/json' } })))
  render(<ThemeProvider><TraceContents contents={[{
    eventId: 'event-plugin', kind: 'plugin_content', occurredAt: '2026-09-06T00:00:00Z', contentRef: null,
    preview: { nodeType: 'acme.mapper', typeVersion: 1, bundleDigest: `sha256:${'a'.repeat(64)}`, contentType: 'acme.mapper/table', contentVersion: 1, data: { rows: 3 } },
  }]} executionId="execution-1" /></ThemeProvider>)

  expect(await screen.findByText(/标准数据仍可查看|Standard data remains visible/)).toBeInTheDocument()
  expect(screen.getByText('rows')).toBeInTheDocument()
  expect(screen.getByText('3')).toBeInTheDocument()
})
