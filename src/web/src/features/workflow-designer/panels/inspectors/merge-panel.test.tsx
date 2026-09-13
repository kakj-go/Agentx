import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, describe, expect, it } from 'vitest'

import { i18n } from '../../../../app/i18n'
import { localizeManifest } from '../../model/manifest-localization'
import type { ActionNodeData } from '../../model/types'
import { studioManifest } from '../../testing/studio-catalog'
import { MergePanel } from './merge-panel'

const manifest = studioManifest('merge')

function MergeHarness({ parameters }: { parameters?: Record<string, unknown> }) {
  const [current, setCurrent] = useState<ActionNodeData>({
    editorKind: 'action', nodeType: 'merge', typeVersion: 1, label: '合并', key: 'merge',
    parameters: parameters ?? {},
    contextWrites: [], resourceReferences: [], settings: {}, disabled: false,
  })
  return <>
    <MergePanel
      data={current}
      fieldErrors={{}}
      localized={localizeManifest(manifest, 'zh-CN')}
      manifest={manifest}
      onChange={(patch) => setCurrent((value) => ({ ...value, ...patch, parameters: { ...value.parameters, ...(patch.parameters ?? {}) } }))}
      providerOptions={{}}
      resources={{}}
    />
    <output data-testid="panel-state">{JSON.stringify(current.parameters)}</output>
  </>
}

afterEach(async () => { await i18n.changeLanguage('zh-CN') })

describe('Merge panel', () => {
  it('renders the three merge mode cards with append active by default', async () => {
    await i18n.changeLanguage('zh-CN')
    render(<MergeHarness />)

    expect(screen.getByTestId('merge-panel')).toBeInTheDocument()
    expect(screen.getByTestId('mode-card-append')).toHaveTextContent('追加')
    expect(screen.getByTestId('mode-card-append')).toHaveTextContent('按到达顺序汇合所有输入的条目')
    expect(screen.getByTestId('mode-card-combine_by_position')).toHaveTextContent('按位置合并')
    expect(screen.getByTestId('mode-card-combine_by_key')).toHaveTextContent('按键合并')
    expect(screen.getByTestId('mode-card-append')).toHaveClass('border-primary')
    expect(screen.getByTestId('merge-inputs-section')).toHaveTextContent(/输入.*任意多条|Connect any number/i)
    expect(screen.queryByTestId('merge-join-fields')).not.toBeInTheDocument()
  })

  it('switches to combine_by_key and exposes the join fields from the registry schema', () => {
    render(<MergeHarness />)

    fireEvent.click(screen.getByTestId('mode-card-combine_by_key'))
    let state = JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}')
    expect(state.mode).toBe('combine_by_key')

    expect(screen.getByTestId('merge-join-fields')).toBeInTheDocument()
    expect(screen.getByTestId('merge-left-field')).toBeInTheDocument()
    expect(screen.getByTestId('merge-right-field')).toBeInTheDocument()
    expect(screen.getByTestId('parameter-joinType')).toBeInTheDocument()
    expect(screen.getByTestId('parameter-conflictStrategy')).toBeInTheDocument()

    fireEvent.click(screen.getByTestId('mode-card-append'))
    state = JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}')
    expect(state.mode).toBe('append')
    expect(screen.queryByTestId('merge-join-fields')).not.toBeInTheDocument()
  })
})
