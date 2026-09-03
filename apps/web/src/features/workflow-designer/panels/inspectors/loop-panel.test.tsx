import { fireEvent, render, screen, within } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, describe, expect, it } from 'vitest'

import { i18n } from '../../../../app/i18n'
import type { ActionNodeData, ReferenceCatalog } from '../../model/types'
import { studioManifest } from '../../testing/studio-catalog'
import { LoopPanel } from './loop-panel'

const manifest = studioManifest('loop_over_items')
const defaultCatalog: ReferenceCatalog = { inputs: [], contexts: [], outputs: [{ id: 'outer', label: 'Outer result', path: 'outputs.outer', children: [] }] }
const outputCatalog: ReferenceCatalog = { inputs: [], contexts: [], loop: [], outputs: [{ id: 'body', label: 'Body result', path: 'outputs.body', children: [] }] }

function LoopHarness() {
  const [current, setCurrent] = useState<ActionNodeData>({
    editorKind: 'action', nodeType: 'loop_over_items', typeVersion: 1, label: '循环处理', key: 'loop',
    parameters: {},
    contextWrites: [], resourceReferences: [], settings: {}, disabled: false,
  })
  return <>
    <LoopPanel
      data={current}
      fieldErrors={{}}
      manifest={manifest}
      onChange={(patch) => setCurrent((value) => ({ ...value, ...patch }))}
      providerOptions={{}}
      referenceCatalog={defaultCatalog}
      parameterCatalogs={{ outputSelector: outputCatalog }}
      resources={{}}
    />
    <output data-testid="panel-state">{JSON.stringify(current.parameters)}</output>
  </>
}

afterEach(async () => { await i18n.changeLanguage('zh-CN') })

describe('Loop panel', () => {
  it('renders the three error mode cards, parallelism and the loop.item builtins hint', async () => {
    await i18n.changeLanguage('zh-CN')
    render(<LoopHarness />)

    expect(screen.getByTestId('loop-panel')).toBeInTheDocument()
    expect(screen.getByTestId('mode-card-terminate')).toHaveTextContent('终止整个迭代')
    expect(screen.getByTestId('mode-card-continue')).toHaveTextContent('失败项输出 null')
    expect(screen.getByTestId('mode-card-remove')).toHaveTextContent('剔除失败项')
    expect(screen.getByTestId('mode-card-terminate')).toHaveClass('border-primary')
    expect(screen.getByTestId('parameter-parallelism')).toBeInTheDocument()
    expect(screen.getByTestId('loop-builtins-section')).toHaveTextContent('loop.item')
    expect(screen.getByTestId('loop-builtins-section')).toHaveTextContent('loop.items')
    expect(screen.getByTestId('loop-builtins-section')).toHaveTextContent('loop.index')
  })

  it('switches the failure policy and parallelism within the registry bounds', () => {
    render(<LoopHarness />)

    fireEvent.click(screen.getByTestId('mode-card-remove'))
    expect(JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}').errorMode).toBe('remove')

    fireEvent.change(screen.getByTestId('parameter-parallelism').querySelector('input')!, { target: { value: '4' } })
    const state = JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}')
    expect(state.parallelism).toBe(4)
    expect(Object.keys(state).sort()).toEqual(['errorMode', 'parallelism'])
  })

  it('uses the iteration-end predecessor catalog for each-round output', () => {
    render(<LoopHarness />)
    const output = screen.getByTestId('parameter-outputSelector')
    fireEvent.click(within(output).getByTestId('reference-input').querySelector('button')!)
    fireEvent.click(within(screen.getByTestId('reference-picker')).getByRole('button', { name: /^(Outputs|输出)$/ }))

    expect(screen.getByText('Body result')).toBeInTheDocument()
    expect(screen.queryByText('Outer result')).not.toBeInTheDocument()
  })
})
