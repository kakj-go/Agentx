import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, describe, expect, it } from 'vitest'

import { i18n } from '../../../../app/i18n'
import { localizeManifest } from '../../model/manifest-localization'
import type { ActionNodeData } from '../../model/types'
import { studioManifest } from '../../testing/studio-catalog'
import { AgentPanel } from './agent-panel'

const manifest = studioManifest('agent')

function AgentHarness() {
  const [current, setCurrent] = useState<ActionNodeData>({
    editorKind: 'action', nodeType: 'agent', typeVersion: 2, label: '智能体', key: 'agent',
    parameters: { systemPrompt: { kind: 'template', segments: [{ kind: 'text', text: '你是工单处理助手。' }] }, userQuestion: { kind: 'template', segments: [] }, sessionPolicy: { mode: 'application_session' } },
    contextWrites: [], resourceReferences: [], settings: {}, disabled: false,
  })
  return <>
    <AgentPanel
      data={current}
      fieldErrors={{}}
      localized={localizeManifest(manifest, 'zh-CN')}
      manifest={manifest}
      onChange={(patch) => setCurrent((value) => ({ ...value, ...patch }))}
      providerOptions={{}}
      resources={{}}
    />
    <output data-testid="panel-state">{JSON.stringify(current.parameters)}</output>
  </>
}

afterEach(async () => { await i18n.changeLanguage('zh-CN') })

describe('Agent panel', () => {
  it('renders all six resource slots with task and budget sections', async () => {
    await i18n.changeLanguage('zh-CN')
    render(<AgentHarness />)

    expect(screen.getByTestId('agent-panel')).toBeInTheDocument()
    expect(screen.getByTestId('agent-core-configuration')).toBeInTheDocument()
    expect(screen.getByTestId('agent-inspector-model')).toBeInTheDocument()
    expect(screen.getByTestId('agent-inspector-workspace_sandbox')).toBeInTheDocument()
    expect(screen.getByTestId('agent-task-section')).toHaveTextContent('任务')
    expect(screen.getByTestId('parameter-systemPrompt').querySelector('[contenteditable="true"]')).toHaveTextContent('你是工单处理助手。')
    expect(screen.getByTestId('parameter-userQuestion')).toBeInTheDocument()
    expect(screen.getByTestId('agent-inspector-mcp_tools')).toHaveTextContent('MCP 工具')
    expect(screen.getByTestId('agent-inspector-skills')).toHaveTextContent('技能')
    expect(screen.getByTestId('agent-inspector-knowledge')).toHaveTextContent('知识库')
    expect(screen.getByTestId('agent-inspector-long_term_memory')).toHaveTextContent('长期记忆')
    expect(screen.queryByTestId('agent-attachments-section')).not.toBeInTheDocument()
    expect(screen.getByTestId('agent-budget-section')).toHaveTextContent('预算')
    expect(screen.queryByTestId('parameter-maxIterations')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /toggle advanced|展开或收起高级配置/i }))
    expect(screen.getByTestId('parameter-maxIterations')).toBeInTheDocument()
    expect(screen.getByTestId('parameter-maxToolCalls')).toBeInTheDocument()
    expect(screen.getByTestId('parameter-limitAction')).toBeInTheDocument()
  })

  it('switches the session policy through the core configuration', () => {
    render(<AgentHarness />)

    const sessionSelect = screen.getByTestId('agent-core-configuration').querySelector('[data-field-path="parameters.sessionPolicy"] [role="combobox"]')!
    fireEvent.click(sessionSelect)
    fireEvent.click(screen.getByRole('option', { name: '仅本次调用' }))

    const state = JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}')
    expect(state.sessionPolicy).toEqual({ mode: 'invocation' })
  })
})
