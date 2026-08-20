import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, describe, expect, it } from 'vitest'

import { i18n } from '../../../app/i18n'
import type { ReferenceCatalog, WorkflowEnd, WorkflowStart } from '../model/types'
import { WorkflowInterfacePanel } from './workflow-interface-panel'

const start: WorkflowStart = {
  inputs: { type: 'object', properties: {}, additionalProperties: false },
  contexts: {},
}

const end: WorkflowEnd = {
  outputs: {},
  error: {
    strategy: 'fail_fast',
    collectWindowMs: 5000,
    outputs: {
      failure_message: {
        value: { kind: 'literal', value: '' },
        schema: { type: 'string' },
        required: true,
        sensitive: false,
      },
    },
  },
}

const catalog: ReferenceCatalog = { inputs: [], outputs: [], contexts: [] }

afterEach(async () => { await i18n.changeLanguage('zh-CN') })

function Harness({ boundary }: { boundary: 'start' | 'end' }) {
  const [startValue, setStart] = useState(start)
  const [endValue, setEnd] = useState(end)
  return <><WorkflowInterfacePanel boundary={boundary} end={endValue} onClose={() => undefined} onEndChange={setEnd} onStartChange={setStart} referenceCatalog={catalog} start={startValue} /><output data-testid="definition-state">{JSON.stringify({ start: startValue, end: endValue })}</output></>
}

describe('Workflow boundary forms', () => {
  it('renders the Start and End interfaces in Chinese without English fallback copy', async () => {
    await i18n.changeLanguage('zh-CN')
    const { rerender } = render(<Harness boundary="start" />)
    expect(screen.getByText('开始')).toBeInTheDocument()
    expect(screen.getByText('配置每次执行的输入和全局变量')).toBeInTheDocument()
    expect(screen.getByText('每次执行时由调用方提供的值')).toBeInTheDocument()
    expect(screen.getByText('执行树或会话范围内共享的变量')).toBeInTheDocument()

    rerender(<Harness boundary="end" />)
    expect(screen.getByText('结束')).toBeInTheDocument()
    expect(screen.getByText('配置成功与错误终态输出')).toBeInTheDocument()
    expect(screen.getByText('成功输出')).toBeInTheDocument()
    expect(screen.getByText('错误输出')).toBeInTheDocument()
  })

  it('does not render a boundary form when no boundary is selected', () => {
    render(<WorkflowInterfacePanel boundary={undefined} end={end} onClose={() => undefined} onEndChange={() => undefined} onStartChange={() => undefined} referenceCatalog={catalog} start={start} />)
    expect(screen.queryByTestId('workflow-interface-panel')).not.toBeInTheDocument()
  })

  it('creates Start input fields through visible controls', () => {
    render(<Harness boundary="start" />)

    fireEvent.click(screen.getAllByRole('button', { name: /Add field|添加字段/ })[0])
    fireEvent.change(screen.getByLabelText(/Title|标题/), { target: { value: 'Question' } })
    fireEvent.click(screen.getByLabelText(/Required|必填/))
    fireEvent.click(screen.getByRole('button', { name: /Save|保存/ }))

    const state = JSON.parse(screen.getByTestId('definition-state').textContent ?? '{}')
    expect(state.start.inputs.properties.input_1.title).toBe('Question')
    expect(state.start.inputs.required).toEqual(['input_1'])
  })

  it('labels controls inside input more settings', () => {
    render(<Harness boundary="start" />)

    fireEvent.click(screen.getAllByRole('button', { name: /Add field|添加字段/ })[0])
    fireEvent.click(screen.getByText(/Constraints and default|约束与默认值/))

    expect(screen.getByLabelText(/Initial value|Default value|初始值|默认值/)).toBeInTheDocument()
    expect(screen.getByLabelText(/Format|格式/)).toBeInTheDocument()
    expect(screen.getByLabelText(/Min length|最短长度/)).toBeInTheDocument()
    expect(screen.getByLabelText(/Max length|最长长度/)).toBeInTheDocument()
  })

  it('shows only constraints supported by the selected input type', () => {
    render(<Harness boundary="start" />)

    fireEvent.click(screen.getAllByRole('button', { name: /Add field|添加字段/ })[0])
    fireEvent.click(screen.getByRole('combobox', { name: /Type|类型/ }))
    fireEvent.click(screen.getByRole('option', { name: /Boolean|布尔值/ }))
    fireEvent.click(screen.getByText(/Constraints and default|约束与默认值/))

    expect(screen.getByLabelText(/Default value|默认值/)).toBeInTheDocument()
    expect(screen.queryByLabelText(/Format|格式/)).not.toBeInTheDocument()
    expect(screen.queryByLabelText(/Enum options|枚举选项/)).not.toBeInTheDocument()
  })

  it('uses file constraints without exposing a JSON default and preserves metadata', () => {
    render(<Harness boundary="start" />)

    fireEvent.click(screen.getAllByRole('button', { name: /Add field|添加字段/ })[0])
    fireEvent.change(screen.getByLabelText(/Title|标题/), { target: { value: 'Attachments' } })
    fireEvent.click(screen.getByRole('combobox', { name: /Type|类型/ }))
    fireEvent.click(screen.getByRole('option', { name: /Artifact array|文件数组/ }))

    expect(screen.getByLabelText(/Title|标题/)).toHaveValue('Attachments')
    expect(screen.getByLabelText(/Allowed content types|允许的文件类型/)).toBeInTheDocument()
    expect(screen.getByLabelText(/Minimum files|最少文件数/)).toBeInTheDocument()
    expect(screen.getByLabelText(/Maximum total file size|文件总大小上限/)).toBeInTheDocument()
    expect(screen.queryByText(/Constraints and default|约束与默认值/)).not.toBeInTheDocument()
  })

  it('offers array cardinality and uniqueness constraints', () => {
    render(<Harness boundary="start" />)

    fireEvent.click(screen.getAllByRole('button', { name: /Add field|添加字段/ })[0])
    fireEvent.click(screen.getByRole('combobox', { name: /Type|类型/ }))
    fireEvent.click(screen.getByRole('option', { name: /^Array$|^数组$/ }))
    const constraintSections = screen.getAllByText(/Constraints and default|约束与默认值/)
    fireEvent.click(constraintSections[constraintSections.length - 1])

    expect(screen.getByLabelText(/Minimum items|最少元素数/)).toBeInTheDocument()
    expect(screen.getByLabelText(/Maximum items|最多元素数/)).toBeInTheDocument()
    expect(screen.getByLabelText(/Require unique items|元素不可重复/)).toBeInTheDocument()
    fireEvent.change(screen.getByLabelText(/Minimum items|最少元素数/), { target: { value: '5' } })
    fireEvent.change(screen.getByLabelText(/Maximum items|最多元素数/), { target: { value: '2' } })
    expect(screen.getByRole('alert')).toHaveTextContent(/minimum cannot exceed|最小值不能大于最大值/i)
    expect(screen.getByRole('button', { name: /Save|保存/ })).toBeDisabled()
  })

  it('defines nested Start input schemas without editing JSON', () => {
    render(<Harness boundary="start" />)

    fireEvent.click(screen.getAllByRole('button', { name: /Add field|添加字段/ })[0])
    fireEvent.click(screen.getByRole('combobox', { name: /Type|类型/ }))
    fireEvent.click(screen.getByRole('option', { name: /Object|对象/ }))
    const shape = screen.getByText(/Object fields|对象字段/).parentElement!
    fireEvent.click(within(shape).getByRole('button', { name: /Add field|添加字段/ }))
    fireEvent.click(screen.getByRole('button', { name: /Save|保存/ }))

    const state = JSON.parse(screen.getByTestId('definition-state').textContent ?? '{}')
    expect(state.start.inputs.properties.input_1).toMatchObject({
      type: 'object',
      additionalProperties: false,
      properties: { field_1: { type: 'string', title: 'field_1' } },
    })
  })

  it('defines a global variable schema and keeps runtime settings under more settings', () => {
    render(<Harness boundary="start" />)

    fireEvent.click(screen.getByRole('button', { name: /Add global variable|添加全局变量/ }))
    fireEvent.click(screen.getByRole('combobox', { name: /Type|类型/ }))
    fireEvent.click(screen.getByRole('option', { name: /Object|对象/ }))
    expect(screen.getByRole('combobox', { name: /Scope|范围/ })).not.toBeVisible()
    fireEvent.click(screen.getByText(/More settings|更多设置/))
    fireEvent.click(screen.getByRole('combobox', { name: /Scope|范围/ }))
    fireEvent.click(screen.getByRole('option', { name: /Session|会话/ }))
    fireEvent.click(screen.getByLabelText(/Mutable|可变/))
    fireEvent.click(screen.getByRole('button', { name: /Save|保存/ }))

    const state = JSON.parse(screen.getByTestId('definition-state').textContent ?? '{}')
    expect(state.start.contexts.variable_1).toMatchObject({
      schema: { type: 'object', additionalProperties: false, properties: {} },
      scope: 'session',
      mutable: false,
    })
  })

  it('inserts a fixed current-error field from the End picker', async () => {
    render(<Harness boundary="end" />)

    fireEvent.click(screen.getByRole('button', { name: /Edit field|编辑字段/ }))
    fireEvent.focus(screen.getByLabelText('Value'))
    fireEvent.click(screen.getByRole('button', { name: /Current data|当前数据/ }))
    fireEvent.click(screen.getByRole('button', { name: /Current error|当前错误/ }))
    fireEvent.click(screen.getByRole('button', { name: /Error message|错误消息/ }))
    await waitFor(() => expect(screen.getByTestId('variable-token-editor').querySelector('[data-agentx-variable]')).not.toBeNull())
    fireEvent.click(screen.getByRole('button', { name: /Save|保存/ }))

    await waitFor(() => {
      const state = JSON.parse(screen.getByTestId('definition-state').textContent ?? '{}')
      expect(state.end.error.outputs.failure_message.value).toMatchObject({ kind: 'reference', selector: { namespace: 'item', path: ['message'] } })
    })
  })

  it('preserves a success expression when a field rename blurs immediately before typing', async () => {
    render(<Harness boundary="end" />)

    fireEvent.click(screen.getAllByRole('button', { name: /Add field|添加字段/ })[0])
    const name = screen.getAllByLabelText(/Output name|输出名称/)[0]
    const expression = screen.getAllByLabelText('Value')[0]
    fireEvent.focus(name)
    fireEvent.change(name, { target: { value: 'answer' } })
    fireEvent.blur(name)
    fireEvent.focus(expression)
    fireEvent.click(screen.getAllByLabelText(/Required|必填/)[0])
    fireEvent.click(screen.getByRole('button', { name: /Save|保存/ }))

    const state = JSON.parse(screen.getByTestId('definition-state').textContent ?? '{}')
    expect(state.end.outputs.answer).toMatchObject({
      value: { kind: 'literal', value: '' },
      required: true,
    })
  })

  it('rejects invalid and duplicate End output names before changing the definition', () => {
    render(<Harness boundary="end" />)

    fireEvent.click(screen.getAllByRole('button', { name: /Add field|添加字段/ })[0])
    const name = screen.getByLabelText(/Output name|输出名称/)
    fireEvent.change(name, { target: { value: '中文输出' } })

    expect(screen.getByRole('alert')).toHaveTextContent(/lowercase letters|小写字母/i)
    expect(screen.getByRole('button', { name: /Save|保存/ })).toBeDisabled()
    const state = JSON.parse(screen.getByTestId('definition-state').textContent ?? '{}')
    expect(state.end.outputs).toEqual({})
  })

  it('rejects invalid global variable names and marks protocol-required labels', () => {
    render(<Harness boundary="start" />)

    fireEvent.click(screen.getByRole('button', { name: /Add global variable|添加全局变量/ }))
    const name = screen.getByText(/Global variable name|全局变量名称/).closest('label')!.querySelector('input')!
    fireEvent.change(name, { target: { value: '2bad-name' } })

    expect(screen.getByRole('alert')).toHaveTextContent(/first character|不能以数字开头/i)
    expect(screen.getByRole('button', { name: /Save|保存/ })).toBeDisabled()
    expect(screen.getByText(/Global variable name|全局变量名称/).parentElement).toHaveTextContent('*')
  })

  it('configures an End output through the field dialog', () => {
    render(<Harness boundary="end" />)

    fireEvent.click(screen.getAllByRole('button', { name: /Add field|添加字段/ })[0])
    fireEvent.click(screen.getByRole('combobox', { name: /Type|类型/ }))
    fireEvent.click(screen.getByRole('option', { name: /Number|数字/ }))
    fireEvent.change(screen.getByLabelText(/Output name|输出名称/), { target: { value: 'answer' } })
    fireEvent.change(screen.getByLabelText(/Title|标题/), { target: { value: 'Answer' } })
    fireEvent.change(screen.getByLabelText(/Description|说明/), { target: { value: 'Final answer' } })
    fireEvent.click(screen.getByLabelText(/Sensitive|敏感/))
    fireEvent.click(screen.getByRole('button', { name: /Save|保存/ }))

    const state = JSON.parse(screen.getByTestId('definition-state').textContent ?? '{}')
    expect(state.end.outputs.answer).toMatchObject({
      value: { kind: 'literal', value: '' },
      schema: { type: 'number', title: 'Answer', description: 'Final answer' },
      sensitive: true,
    })
  })

  it('places output behavior before the optional description', () => {
    render(<Harness boundary="end" />)

    fireEvent.click(screen.getAllByRole('button', { name: /Add field|添加字段/ })[0])
    const expression = screen.getByLabelText('Value')
    const required = screen.getByLabelText(/Required|必填/)
    const sensitive = screen.getByLabelText(/Sensitive|敏感/)
    const description = screen.getByLabelText(/Description|说明/)

    expect(expression.compareDocumentPosition(required) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(required.compareDocumentPosition(description) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(sensitive.compareDocumentPosition(description) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('configures the End error collect strategy and bounded window', () => {
    render(<Harness boundary="end" />)

    fireEvent.click(screen.getByRole('combobox', { name: /Error strategy|错误策略/ }))
    fireEvent.click(screen.getByRole('option', { name: /Collect|收集错误/ }))
    fireEvent.change(screen.getByLabelText(/Collect window|收集窗口/), { target: { value: '1200' } })

    const state = JSON.parse(screen.getByTestId('definition-state').textContent ?? '{}')
    expect(state.end.error).toMatchObject({ strategy: 'collect', collectWindowMs: 1200 })
  })
})
