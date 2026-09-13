import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, describe, expect, it } from 'vitest'

import { i18n } from '../../../../app/i18n'
import type { ActionNodeData } from '../../model/types'
import { studioManifest } from '../../testing/studio-catalog'
import { ListPanel } from './list-panel'

const manifest = studioManifest('list')

function ListHarness() {
  const [current, setCurrent] = useState<ActionNodeData>({
    editorKind: 'action', nodeType: 'list', typeVersion: 1, label: '列表操作', key: 'list_op',
    parameters: { input: { kind: 'literal', value: [] }, filter: { conditions: [], logicalOp: 'and' }, sort: [], takeN: 20 },
    contextWrites: [], resourceReferences: [], settings: {}, disabled: false,
  })
  return <>
    <ListPanel
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

describe('List panel', () => {
  it('renders the input hint, filter builder, sort rows and the take-N limit', async () => {
    await i18n.changeLanguage('zh-CN')
    render(<ListHarness />)

    expect(screen.getByTestId('list-panel')).toBeInTheDocument()
    expect(screen.getByTestId('parameter-input')).toBeInTheDocument()
    expect(screen.getByTestId('parameter-filter').querySelector('[data-testid="condition-builder"]')).toBeInTheDocument()
    expect(screen.getByTestId('list-sort-rows')).toBeInTheDocument()
    expect(screen.getByTestId('parameter-takeN').querySelector('input')).toHaveValue(20)
  })

  it('adds a filter condition matching the registry object structure', () => {
    render(<ListHarness />)

    fireEvent.click(screen.getByTestId('parameter-filter').querySelector('button[aria-label="添加条件"]')!)

    const state = JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}')
    expect(state.filter.conditions).toEqual([{ condition: { left: { kind: 'literal', value: '' }, operator: 'eq', right: { kind: 'literal', value: '' } } }])
    expect(state.filter.logicalOp).toBe('and')

    fireEvent.change(screen.getByTestId('parameter-takeN').querySelector('input')!, { target: { value: '50' } })
    expect(JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}').takeN).toBe(50)
  })
})
