import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, describe, expect, it } from 'vitest'

import { i18n } from '../../../../app/i18n'
import { localizeManifest } from '../../model/manifest-localization'
import type { ActionNodeData, ResourceOption } from '../../model/types'
import { studioManifest } from '../../testing/studio-catalog'
import { AgentPanel } from './agent-panel'

const manifest = studioManifest('agent')

const modelOptions: ResourceOption[] = [
  {
    value: 'model-1',
    label: 'GLM-4.7',
    resourceType: 'model',
    operation: 'use',
    accessState: 'authorized',
    metadata: { maxInputTokens: 200000, maxOutputTokens: 32000, currency: 'CNY' },
  },
]

type PanelResources = Parameters<typeof AgentPanel>[0]['resources'];

function AgentHarness({ resources = {} }: { resources?: PanelResources }) {
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
      onChange={(patch) => setCurrent((value) => ({ ...value, ...patch, parameters: { ...value.parameters, ...(patch.parameters ?? {}) } }))}
      providerOptions={{}}
      resources={resources}
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

  it('localizes budget unit badges', async () => {
    await i18n.changeLanguage('zh-CN')
    render(<AgentHarness />)

    fireEvent.click(screen.getByRole('button', { name: /toggle advanced|展开或收起高级配置/i }))
    expect(screen.getByTestId('parameter-maxIterations')).toHaveTextContent('次数')
    expect(screen.getByTestId('parameter-maxModelCalls')).toHaveTextContent('次数')
    expect(screen.getByTestId('parameter-maxToolCalls')).toHaveTextContent('次数')
    expect(screen.getByTestId('parameter-maxDurationSeconds')).toHaveTextContent('秒')
    expect(screen.getByTestId('parameter-maxCost')).toHaveTextContent('货币单位')
  })

  it('seeds token budgets from the selected model and follows its currency', async () => {
    await i18n.changeLanguage('zh-CN')
    render(<AgentHarness resources={{ model: modelOptions }} />)

    fireEvent.click(screen.getByTestId('agent-inspector-model').querySelector('[role="combobox"]')!)
    fireEvent.click(screen.getByRole('option', { name: /GLM-4\.7/ }))

    await waitFor(() => {
      const state = JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}')
      expect(state.maxTotalTokens).toBe(200000)
      expect(state.maxOutputTokens).toBe(32000)
    })
    fireEvent.click(screen.getByRole('button', { name: /toggle advanced|展开或收起高级配置/i }))
    expect(screen.getByTestId('parameter-maxCost')).toHaveTextContent('CNY')
  })

  it('keeps manually edited budgets until the model is selected again', async () => {
    await i18n.changeLanguage('zh-CN')
    render(<AgentHarness resources={{ model: modelOptions }} />)

    fireEvent.click(screen.getByTestId('agent-inspector-model').querySelector('[role="combobox"]')!)
    fireEvent.click(screen.getByRole('option', { name: /GLM-4\.7/ }))
    await waitFor(() => {
      expect(JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}').maxTotalTokens).toBe(200000)
    })

    fireEvent.click(screen.getByRole('button', { name: /toggle advanced|展开或收起高级配置/i }))
    const tokenInput = screen.getByTestId('parameter-maxTotalTokens').querySelector('input')!
    fireEvent.change(tokenInput, { target: { value: '1000' } })
    expect(JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}').maxTotalTokens).toBe(1000)

    fireEvent.change(screen.getByTestId('parameter-maxCost').querySelector('input')!, { target: { value: '2.5' } })
    expect(JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}').maxCost).toBe(2.5)
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
