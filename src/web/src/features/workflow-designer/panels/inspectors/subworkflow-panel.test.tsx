import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, describe, expect, it } from 'vitest'

import { i18n } from '../../../../app/i18n'
import type { ActionNodeData } from '../../model/types'
import { studioManifest } from '../../testing/studio-catalog'
import { SubworkflowPanel } from './subworkflow-panel'

const manifest = studioManifest('sub_workflow')
const versionId = '018f0000-0000-7000-8000-000000000003'
const childManifest = {
  ...structuredClone(manifest),
  nodeType: 'workflow.018f0000000070008000000000000003',
  displayName: '报销入账流程 v3',
  parameterSchema: {
    ...manifest.parameterSchema,
    properties: {
      ...manifest.parameterSchema.properties,
      workflowVersionId: { type: 'string', const: versionId },
      inputs: {
        allOf: [{ type: 'object', required: ['question'], properties: { question: { type: 'string' } }, additionalProperties: false }],
        'x-agentx-binding': { acceptedKinds: ['literal', 'reference', 'template', 'array', 'object'], allowedNamespaces: ['inputs', 'outputs', 'contexts', 'execution'], acceptedCardinality: ['single'], missingPolicies: ['error', 'null', 'omit'], recursive: true },
      },
    },
  },
  outputSchema: { type: 'object', required: ['answer'], properties: { answer: { type: 'string' } }, additionalProperties: false },
} as unknown as typeof manifest

function SubworkflowHarness() {
  const [current, setCurrent] = useState<ActionNodeData>({
    editorKind: 'action', nodeType: 'sub_workflow', typeVersion: 1, label: '子流程', key: 'subflow',
    parameters: { workflowVersionId: '' },
    contextWrites: [], resourceReferences: [], settings: {}, disabled: false,
  })
  return <>
    <SubworkflowPanel
      data={current}
      fieldErrors={{}}
      manifest={manifest}
      onChange={(patch) => setCurrent((value) => ({ ...value, ...patch, parameters: { ...value.parameters, ...(patch.parameters ?? {}) } }))}
      providerOptions={{ workflowVersionId: [
        { value: versionId, label: '报销入账流程 v3', manifest: childManifest },
        { value: 'version-2', label: '报销入账流程 v2' },
      ] }}
      resources={{}}
    />
    <output data-testid="panel-state">{JSON.stringify(current.parameters)}</output>
  </>
}

afterEach(async () => { await i18n.changeLanguage('zh-CN') })

describe('Sub-workflow panel', () => {
  it('renders the target workflow picker and the output hint', async () => {
    await i18n.changeLanguage('zh-CN')
    render(<SubworkflowHarness />)

    expect(screen.getByTestId('subworkflow-panel')).toBeInTheDocument()
    expect(screen.getByTestId('subworkflow-target-section')).toHaveTextContent('目标工作流')
    expect(screen.getByTestId('parameter-workflowVersionId')).toBeInTheDocument()
    expect(screen.getByTestId('subworkflow-output-section')).toHaveTextContent('子流程结束节点的输出即本节点输出')
  })

  it('picks a published workflow version and writes workflowVersionId', () => {
    render(<SubworkflowHarness />)

    fireEvent.click(screen.getByTestId('parameter-workflowVersionId').querySelector('[role="combobox"]')!)
    fireEvent.click(screen.getByRole('option', { name: '报销入账流程 v3' }))

    const state = JSON.parse(screen.getByTestId('panel-state').textContent ?? '{}')
    expect(state.workflowVersionId).toBe(versionId)
    expect(state.inputs).toEqual({ kind: 'object', fields: {} })
    expect(screen.getByText('question')).toBeInTheDocument()
    expect(screen.getByTestId('subworkflow-output-section')).toHaveTextContent('answer')
  })
})
