import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, describe, expect, it } from 'vitest'

import { i18n } from '../../../../app/i18n'
import type { ActionNodeData } from '../../model/types'
import { studioManifest } from '../../testing/studio-catalog'
import { ApprovalPanel } from './approval-panel'

const manifest = studioManifest('approval')

function ApprovalHarness() {
  const [current, setCurrent] = useState<ActionNodeData>({
    editorKind: 'action', nodeType: 'approval', typeVersion: 1, label: '审批', key: 'approval',
    parameters: {
      title: { kind: 'template', segments: [{ kind: 'text', text: '工单金额审批' }] },
      description: { kind: 'template', segments: [{ kind: 'text', text: '请审核该工单的报销金额与事由。' }] },
      candidateUserId: '',
      buttons: [{ id: 'approved', label: '审批通过' }, { id: 'rejected', label: '审批拒绝' }],
    },
    contextWrites: [], resourceReferences: [], settings: {}, disabled: false,
  })
  return <>
    <ApprovalPanel
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

describe('Approval panel', () => {
  it('renders approval content, the button editor and duration timeout', async () => {
    await i18n.changeLanguage('zh-CN')
    render(<ApprovalHarness />)

    expect(screen.getByTestId('approval-panel')).toBeInTheDocument()
    expect(screen.getByTestId('approval-content-section')).toBeInTheDocument()
    expect(screen.getByTestId('parameter-title').querySelector('[contenteditable="true"]')).toHaveTextContent('工单金额审批')
    expect(screen.getByTestId('parameter-description').querySelector('[contenteditable="true"]')).toHaveTextContent('请审核该工单的报销金额与事由。')
    expect(screen.getByTestId('parameter-candidateUserId')).toBeInTheDocument()
    expect(screen.getByTestId('buttons-editor')).toBeInTheDocument()
    expect(screen.getByTestId('approval-timeout-enabled')).not.toBeChecked()
    fireEvent.click(screen.getByTestId('approval-timeout-enabled'))
    expect(screen.getByTestId('parameter-timeoutMs')).toBeInTheDocument()
    expect(screen.queryByTestId('parameter-timeoutAt')).not.toBeInTheDocument()
    expect(screen.getByTestId('approval-branches-section')).toHaveTextContent('每个按钮对应节点右侧一个决策分支连接点')
    expect(screen.getByTestId('approval-timeout-section')).toHaveTextContent('decision = timed_out')
  })

  it('adds a decision button matching the registry buttons contract', () => {
    render(<ApprovalHarness />)

    fireEvent.click(screen.getByRole('button', { name: /添加按钮|Add button/ }))

    const state = JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}')
    expect(state.buttons).toHaveLength(3)
    expect(state.buttons[2].label).toBe('')
    expect(state.buttons[2].id).toMatch(/^decision_[0-9a-f-]{36}$/)
    expect(state.buttons.slice(0, 2).map((button: { id: string }) => button.id)).toEqual(['approved', 'rejected'])
  })
})
