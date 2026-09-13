import { fireEvent, render, screen, within } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, describe, expect, it } from 'vitest'

import { i18n } from '../../../../app/i18n'
import type { WorkflowStart } from '../../model/types'
import { StartPanel } from './start-panel'

const start: WorkflowStart = {
  inputs: { type: 'object', properties: {}, additionalProperties: false },
  contexts: {},
}

afterEach(async () => { await i18n.changeLanguage('zh-CN') })

function Harness({ start: initial = start }: { start?: WorkflowStart }) {
  const [startValue, setStart] = useState(initial)
  return <><StartPanel onClose={() => undefined} onStartChange={setStart} start={startValue} /><output data-testid="definition-state">{JSON.stringify({ start: startValue })}</output></>
}

describe('Start boundary panel', () => {
  it('renders the Start interface in Chinese without English fallback copy', async () => {
    await i18n.changeLanguage('zh-CN')
    render(<Harness />)
    expect(screen.getByTestId('start-panel')).toBeInTheDocument()
    expect(screen.getByText('开始')).toBeInTheDocument()
    expect(screen.getByText('配置每次执行的输入和全局变量')).toBeInTheDocument()
    expect(screen.getByText('每次执行时由调用方提供的值')).toBeInTheDocument()
    expect(screen.getByText('执行树或会话范围内共享的变量')).toBeInTheDocument()
    expect(screen.getByText('系统变量')).toBeInTheDocument()
    expect(screen.getByText(/execution\.id/)).toBeInTheDocument()
  })

  it('creates Start input fields through visible controls', () => {
    render(<Harness />)

    fireEvent.click(screen.getAllByRole('button', { name: /Add field|添加字段/ })[0])
    fireEvent.change(screen.getByLabelText(/Title|标题/), { target: { value: 'Question' } })
    fireEvent.click(screen.getByLabelText(/Required|必填/))
    fireEvent.click(screen.getByRole('button', { name: /Save|保存/ }))

    const state = JSON.parse(screen.getByTestId('definition-state').textContent ?? '{}')
    expect(state.start.inputs.properties.input_1.title).toBe('Question')
    expect(state.start.inputs.required).toEqual(['input_1'])
  })

  it('labels controls inside input more settings', () => {
    render(<Harness />)

    fireEvent.click(screen.getAllByRole('button', { name: /Add field|添加字段/ })[0])
    fireEvent.click(screen.getByText(/Constraints and default|约束与默认值/))

    expect(screen.getByLabelText(/Initial value|Default value|初始值|默认值/)).toBeInTheDocument()
    expect(screen.getByLabelText(/Format|格式/)).toBeInTheDocument()
    expect(screen.getByLabelText(/Min length|最短长度/)).toBeInTheDocument()
    expect(screen.getByLabelText(/Max length|最长长度/)).toBeInTheDocument()
  })

  it('shows only constraints supported by the selected input type', () => {
    render(<Harness />)

    fireEvent.click(screen.getAllByRole('button', { name: /Add field|添加字段/ })[0])
    fireEvent.click(screen.getByRole('combobox', { name: /Type|类型/ }))
    fireEvent.click(screen.getByRole('option', { name: /Boolean|布尔值/ }))
    fireEvent.click(screen.getByText(/Constraints and default|约束与默认值/))

    expect(screen.getByLabelText(/Default value|默认值/)).toBeInTheDocument()
    expect(screen.queryByLabelText(/Format|格式/)).not.toBeInTheDocument()
    expect(screen.queryByLabelText(/Enum options|枚举选项/)).not.toBeInTheDocument()
  })

  it('uses file constraints without exposing a JSON default and preserves metadata', () => {
    render(<Harness />)

    fireEvent.click(screen.getAllByRole('button', { name: /Add field|添加字段/ })[0])
    fireEvent.change(screen.getByLabelText(/Title|标题/), { target: { value: 'Attachments' } })
    fireEvent.click(screen.getByRole('combobox', { name: /Type|类型/ }))
    fireEvent.click(screen.getByRole('option', { name: /Artifact array|文件数组/ }))

    expect(screen.getByLabelText(/Title|标题/)).toHaveValue('Attachments')
    expect(screen.getByLabelText(/Allowed content types|允许的文件类型/)).toBeInTheDocument()
    expect(screen.getByLabelText(/Minimum files|最少文件数/)).toBeInTheDocument()
    expect(screen.getByLabelText(/Maximum total file size|文件总大小上限/)).toBeInTheDocument()
    expect(screen.queryByText(/Constraints and default|约束与默认值/)).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /Save|保存/ }))

    const state = JSON.parse(screen.getByTestId('definition-state').textContent ?? '{}')
    expect(state.start.inputs.properties.input_1.items).toMatchObject({
      type: 'object',
      additionalProperties: false,
      required: ['artifactId'],
      properties: {
        artifactId: { type: 'string', format: 'uuid' },
        sha256: { type: 'string' },
      },
    })
  })

  it('offers array cardinality and uniqueness constraints', () => {
    render(<Harness />)

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
    render(<Harness />)

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
    render(<Harness />)

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

  it('rejects invalid global variable names and marks protocol-required labels', () => {
    render(<Harness />)

    fireEvent.click(screen.getByRole('button', { name: /Add global variable|添加全局变量/ }))
    const name = screen.getByText(/Global variable name|全局变量名称/).closest('label')!.querySelector('input')!
    fireEvent.change(name, { target: { value: '2bad-name' } })

    expect(screen.getByRole('alert')).toHaveTextContent(/first character|不能以数字开头/i)
    expect(screen.getByRole('button', { name: /Save|保存/ })).toBeDisabled()
    expect(screen.getByText(/Global variable name|全局变量名称/).parentElement).toHaveTextContent('*')
  })
})
