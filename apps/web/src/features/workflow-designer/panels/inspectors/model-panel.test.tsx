import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, describe, expect, it } from 'vitest'

import { i18n } from '../../../../app/i18n'
import type { ActionNodeData } from '../../model/types'
import { studioManifest } from '../../testing/studio-catalog'
import { ModelPanel } from './model-panel'

const manifest = studioManifest('model')

function ModelHarness() {
  const [current, setCurrent] = useState<ActionNodeData>({
    editorKind: 'action', nodeType: 'model', typeVersion: 1, label: '模型', key: 'model',
    parameters: { prompt: { kind: 'template', segments: [{ kind: 'text', text: '你是工单审批助手。' }] }, userQuestion: { kind: 'template', segments: [] } },
    contextWrites: [], resourceReferences: [], settings: {}, disabled: false,
  })
  return <>
    <ModelPanel
      data={current}
      fieldErrors={{}}
      manifest={manifest}
      onChange={(patch) => setCurrent((value) => ({ ...value, ...patch }))}
      providerOptions={{}}
      resources={{ model: [{ value: 'model-1', label: 'GPT-4o mini', resourceType: 'model', operation: 'use', accessState: 'authorized' }] }}
    />
    <output data-testid="panel-state">{JSON.stringify(current.parameters)}</output>
  </>
}

afterEach(async () => { await i18n.changeLanguage('zh-CN') })

describe('Model panel', () => {
  it('renders the model resource, prompt group and one output contract', async () => {
    await i18n.changeLanguage('zh-CN')
    render(<ModelHarness />)

    expect(screen.getByTestId('model-panel')).toBeInTheDocument()
    expect(screen.getByTestId('model-model-section')).toBeInTheDocument()
    expect(screen.getByTestId('resource-selector-model')).toBeInTheDocument()
    expect(screen.getByTestId('model-prompt-section')).toHaveTextContent('提示词')
    expect(screen.getByTestId('parameter-prompt').querySelector('[contenteditable="true"]')).toHaveTextContent('你是工单审批助手。')
    expect(screen.getByTestId('parameter-userQuestion')).toBeInTheDocument()
    expect(screen.getByTestId('model-prompt-section')).toHaveTextContent('{{')
    const output = screen.getByTestId('output-contract-hint')
    expect(output).toHaveTextContent('text')
    expect(output).toHaveTextContent('usage')
    expect(screen.queryByRole('button', { name: /添加自定义输出|Add custom output/ })).not.toBeInTheDocument()
  })

  it('creates an explicit object schema for native structured output', () => {
    render(<ModelHarness />)

    fireEvent.click(screen.getByTestId('parameter-responseMode').querySelector('[role="combobox"]')!)
    fireEvent.click(screen.getByRole('option', { name: 'json_schema' }))
    fireEvent.click(screen.getByTestId('model-schema-template'))

    const state = JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}')
    expect(state.responseMode).toBe('json_schema')
    expect(state.structuredSchema).toEqual({ type: 'object', properties: { answer: { type: 'string' } }, required: ['answer'], additionalProperties: false })
    expect(screen.getByTestId('output-contract-hint')).toHaveTextContent('answer')
  })
})
