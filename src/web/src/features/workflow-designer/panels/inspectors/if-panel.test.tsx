import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, describe, expect, it } from 'vitest'

import { i18n } from '../../../../app/i18n'
import type { ActionNodeData } from '../../model/types'
import { studioManifest } from '../../testing/studio-catalog'
import { IfPanel } from './if-panel'

const manifest = studioManifest('if')
const data: ActionNodeData = {
  editorKind: 'action', nodeType: 'if', typeVersion: 1, label: '条件分支', key: 'if',
  parameters: { cases: [{ id: 'case_1', name: '满足条件', conditions: [{ condition: { left: { kind: 'literal', value: 'priority' }, operator: 'eq', right: { kind: 'literal', value: 'high' } } }], logicalOp: 'and' }] },
  contextWrites: [], resourceReferences: [], settings: {}, disabled: false,
}

function IfHarness() {
  const [current, setCurrent] = useState<ActionNodeData>(data)
  return <>
    <IfPanel
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

describe('If panel', () => {
  it('renders the branch cards, implicit ELSE block and connection hint', async () => {
    await i18n.changeLanguage('zh-CN')
    render(<IfHarness />)

    expect(screen.getByTestId('if-panel')).toBeInTheDocument()
    expect(screen.getByTestId('condition-builder')).toBeInTheDocument()
    expect(screen.getByTestId('condition-branch-0')).toBeInTheDocument()
    expect(screen.getByTestId('condition-comparison-row')).toBeInTheDocument()
    expect(screen.getByTestId('if-else-branch')).toHaveTextContent('不满足以上任何条件时走此分支')
    expect(screen.getByTestId('if-notes-section')).toHaveTextContent('每个分支在节点右侧生成独立连接点')
  })

  it('adds an ELIF branch matching the registry cases contract', () => {
    render(<IfHarness />)

    fireEvent.click(screen.getByRole('button', { name: /添加分支|Add branch/ }))

    const state = JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}')
    expect(state.cases).toHaveLength(2)
    expect(state.cases[1]).toMatchObject({ name: '', conditions: [{ condition: { left: { kind: 'literal', value: '' }, operator: 'eq', right: { kind: 'literal', value: '' } } }], logicalOp: 'and' })
    expect(state.cases[1].id).toMatch(/^case_[0-9a-f-]{36}$/)
    expect(Object.keys(state.cases[1]).sort()).toEqual(['conditions', 'id', 'logicalOp', 'name'])
  })

  it('initializes a fresh empty branch with one editable condition row', () => {
    function EmptyHarness() {
      const [current, setCurrent] = useState<ActionNodeData>({ ...data, parameters: { cases: [{ id: 'case_1', name: '', conditions: [], logicalOp: 'and' }] } })
      return <><IfPanel data={current} fieldErrors={{}} manifest={manifest} onChange={(patch) => setCurrent((value) => ({ ...value, ...patch, parameters: { ...value.parameters, ...(patch.parameters ?? {}) } }))} providerOptions={{}} resources={{}} /><output data-testid="empty-state">{JSON.stringify(current.parameters)}</output></>
    }
    render(<EmptyHarness />)

    const state = JSON.parse(screen.getByTestId('empty-state').textContent ?? '{}')
    expect(state.cases[0].conditions).toHaveLength(1)
    expect(screen.getByTestId('condition-comparison-row')).toBeInTheDocument()
  })
})
