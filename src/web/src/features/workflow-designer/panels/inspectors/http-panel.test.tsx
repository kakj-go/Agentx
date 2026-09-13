import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, describe, expect, it } from 'vitest'

import { i18n } from '../../../../app/i18n'
import type { ActionNodeData } from '../../model/types'
import { studioManifest } from '../../testing/studio-catalog'
import { HttpPanel } from './http-panel'

const manifest = studioManifest('declarative_http')

function HttpHarness() {
  const [current, setCurrent] = useState<ActionNodeData>({
    editorKind: 'action', nodeType: 'declarative_http', typeVersion: 1, label: 'HTTP 请求', key: 'http',
    parameters: { method: 'GET', url: { kind: 'template', segments: [{ kind: 'text', text: 'https://api.internal/tickets' }] }, query: [], headers: [], body: {} },
    contextWrites: [], resourceReferences: [], settings: {}, disabled: false,
  })
  return <>
    <HttpPanel
      data={current}
      fieldErrors={{}}
      manifest={manifest}
      onChange={(patch) => setCurrent((value) => ({ ...value, ...patch, parameters: { ...value.parameters, ...(patch.parameters ?? {}) } }))}
      providerOptions={{}}
      resources={{}}
    />
    <output data-testid="panel-state">{JSON.stringify({ parameters: current.parameters, settings: current.settings })}</output>
  </>
}

afterEach(async () => { await i18n.changeLanguage('zh-CN') })

describe('HTTP panel', () => {
  it('renders the method and url row with query, header and body tabs', async () => {
    await i18n.changeLanguage('zh-CN')
    render(<HttpHarness />)

    expect(screen.getByTestId('http-panel')).toBeInTheDocument()
    const request = screen.getByTestId('http-request-section')
    expect(request.querySelector('[data-testid="parameter-method"] [role="combobox"]')).toBeInTheDocument()
    expect(request.querySelector('[data-testid="parameter-url"] [contenteditable="true"]')).toHaveTextContent('https://api.internal/tickets')
    expect(screen.getByRole('tab', { name: '请求头' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: '请求体' })).toBeInTheDocument()
    expect(screen.getByTestId('http-query-rows')).toBeVisible()
    expect(screen.queryByTestId('http-headers-rows')).not.toBeInTheDocument()
    expect(screen.queryByTestId('parameter-body')).not.toBeInTheDocument()
    expect(screen.getByTestId('http-timeout-seconds')).toHaveValue(30)
    const output = screen.getByTestId('output-contract-hint')
    expect(output).toHaveTextContent('statusCode')
    expect(output).toHaveTextContent('files')
  })

  it('switches the request tab and writes method and timeout with registry parameter names', () => {
    render(<HttpHarness />)

    const bodyTab = screen.getByRole('tab', { name: '请求体' })
    bodyTab.focus()
    fireEvent.keyDown(bodyTab, { key: 'Enter' })
    expect(screen.getByTestId('parameter-body')).toBeVisible()
    expect(screen.queryByTestId('http-headers-rows')).not.toBeInTheDocument()

    fireEvent.click(screen.getByTestId('parameter-method').querySelector('[role="combobox"]')!)
    fireEvent.click(screen.getByRole('option', { name: 'POST' }))
    expect(JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}').parameters.method).toBe('POST')

    fireEvent.change(screen.getByTestId('http-timeout-seconds'), { target: { value: '5' } })
    expect(JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}').settings.timeoutMs).toBe(5_000)
  })
})
