import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, describe, expect, it } from 'vitest'

import { i18n } from '../../../../app/i18n'
import type { ActionNodeData } from '../../model/types'
import { studioManifest } from '../../testing/studio-catalog'
import { SetPanel } from './set-panel'

const manifest = studioManifest('set')

function SetHarness() {
  const [current, setCurrent] = useState<ActionNodeData>({
    editorKind: 'action', nodeType: 'set', typeVersion: 1, label: '字段编辑', key: 'set_fields',
    parameters: { values: { kind: 'object', fields: { amount: { kind: 'literal', value: 'approved' } } }, keepOnlySet: false },
    contextWrites: [], resourceReferences: [], settings: {}, disabled: false,
  })
  return <>
    <SetPanel
      data={current}
      fieldErrors={{}}
      manifest={manifest}
      onChange={(patch) => setCurrent((value) => ({ ...value, ...patch }))}
      providerOptions={{}}
      resources={{}}
    />
    <output data-testid="panel-state">{JSON.stringify(current.parameters)}</output>
  </>
}

afterEach(async () => { await i18n.changeLanguage('zh-CN') })

describe('Set panel', () => {
  it('renders the assignment rows and the keep-only-set option', async () => {
    await i18n.changeLanguage('zh-CN')
    render(<SetHarness />)

    expect(screen.getByTestId('set-panel')).toBeInTheDocument()
    expect(screen.getByTestId('set-values-section')).toHaveTextContent('赋值')
    expect(screen.getByTestId('set-value-rows')).toBeInTheDocument()
    expect(screen.getByTestId('set-options-section')).toHaveTextContent('选项')
    expect(screen.getByRole('checkbox', { name: /keep only set|仅保留已设置字段/i })).not.toBeChecked()
    expect(screen.queryByRole('button', { name: /添加自定义输出|Add custom output/ })).not.toBeInTheDocument()
  })

  it('adds an assignment row writing into the values object', () => {
    render(<SetHarness />)

    const buttons = screen.getByTestId('set-value-rows').querySelectorAll('button')
    fireEvent.click(buttons[buttons.length - 1])

    const state = JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}')
    expect(state.values).toEqual({ kind: 'object', fields: { amount: { kind: 'literal', value: 'approved' }, field_2: { kind: 'literal', value: '' } } })
    expect(state.keepOnlySet).toBe(false)

    fireEvent.click(screen.getByRole('checkbox', { name: /keep only set|仅保留已设置字段/i }))
    expect(JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}').keepOnlySet).toBe(true)
  })

  it('keeps the field-name editor mounted while typing', () => {
    render(<SetHarness />)
    const name = screen.getByRole('textbox', { name: /Name|名称/ })
    name.focus()

    fireEvent.change(name, { target: { value: 'a' } })
    expect(document.activeElement).toBe(name)
    fireEvent.change(name, { target: { value: 'answer' } })
    expect(document.activeElement).toBe(name)
    fireEvent.blur(name)

    const state = JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}')
    expect(state.values.fields.answer).toEqual({ kind: 'literal', value: 'approved' })
  })
})
