import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, describe, expect, it } from 'vitest'

import { i18n } from '../../../../app/i18n'
import type { ActionNodeData } from '../../model/types'
import { studioManifest } from '../../testing/studio-catalog'
import { CodePanel } from './code-panel'

const manifest = studioManifest('code')

function CodeHarness() {
  const [current, setCurrent] = useState<ActionNodeData>({
    editorKind: 'action', nodeType: 'code', typeVersion: 1, label: '代码', key: 'code',
    parameters: { runner: 'python', inputs: { kind: 'object', fields: { items: { kind: 'array', items: [] } } }, source: 'def main(**inputs):\n    return {"items": inputs["items"]}', outputExample: { items: [] }, networkPolicy: { mode: 'deny', destinations: [] } },
    contextWrites: [], resourceReferences: [], settings: {}, disabled: false,
  })
  return <>
    <CodePanel
      data={current}
      fieldErrors={{}}
      manifest={manifest}
      onChange={(patch) => setCurrent((value) => ({ ...value, ...patch, parameters: { ...value.parameters, ...(patch.parameters ?? {}) } }))}
      providerOptions={{}}
      resources={{}}
    />
    <output data-testid="panel-state">{JSON.stringify(current.parameters)}</output>
  </>
}

afterEach(async () => { await i18n.changeLanguage('zh-CN') })

describe('Code panel', () => {
  it('renders runtime, inputs, code block, resources and the output contract', async () => {
    await i18n.changeLanguage('zh-CN')
    render(<CodeHarness />)

    expect(screen.getByTestId('code-panel')).toBeInTheDocument()
    expect(screen.getByTestId('parameter-runner')).toBeInTheDocument()
    expect(screen.getByTestId('code-network-policy')).toBeInTheDocument()
    expect(screen.getByTestId('parameter-inputs')).toBeInTheDocument()
    expect(screen.getByTestId('parameter-source')).toBeInTheDocument()
    expect(screen.getByTestId('code-output-example')).toBeInTheDocument()
    expect(screen.getByTestId('code-output-example')).toHaveTextContent(/完整返回对象|complete returned object/)
    expect(screen.getByTestId('resource-selector-sandbox_profile')).toBeInTheDocument()
    const output = screen.getByTestId('output-contract-hint')
    expect(output).toHaveTextContent('stdout')
    expect(output).toHaveTextContent('items')
    expect(output).toHaveTextContent('exitCode')
  })

  it('switches the runner and writes back the registry parameter name', () => {
    render(<CodeHarness />)

    fireEvent.click(screen.getByTestId('parameter-runner').querySelector('[role="combobox"]')!)
    fireEvent.click(screen.getByRole('option', { name: 'javascript' }))

    expect(JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}').runner).toBe('javascript')
  })

  it('commits a complete JSON5 output example on blur', () => {
    render(<CodeHarness />)
    const editor = screen.getByTestId('code-output-example').querySelector('textarea')!

    fireEvent.change(editor, { target: { value: '{ name: "", count: 0 }' } })
    fireEvent.blur(editor)

    expect(JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}').outputExample).toEqual({ name: '', count: 0 })
  })
})
