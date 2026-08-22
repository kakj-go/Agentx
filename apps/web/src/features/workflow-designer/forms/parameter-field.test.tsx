import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { useState } from 'react'
import { describe, expect, it, vi } from 'vitest'

import { ParameterField } from './parameter-field'

const referenceCatalog = {
  inputs: [{ id: 'inputs.question', label: 'question', path: 'inputs.question', selector: { namespace: 'inputs' as const, run: { kind: 'current' as const }, item: { kind: 'current' as const }, path: ['question'] }, type: 'string', children: [] }],
  outputs: [],
  contexts: [],
}

vi.mock('./code-editor', () => ({
  CodeEditor: ({ value, onChange }: { value: string; onChange: (value: string) => void }) => <textarea aria-label="code-editor" onChange={(event) => onChange(event.target.value)} value={value} />,
}))

describe('ParameterField', () => {
  it('renders Platform API provider options declared by the manifest', () => {
    render(
      <ParameterField
        name="workflowVersionId"
        onChange={vi.fn()}
        parameters={{ workflowVersionId: 'version-2' }}
        providerOptions={[{ value: 'version-1', label: 'Orders · v1' }, { value: 'version-2', label: 'Orders · v2' }]}
        required
        schema={{ type: 'string', format: 'uuid' }}
        ui={{ control: 'provider_options', provider: 'workflow_versions' }}
        value="version-2"
      />,
    )

    expect(screen.getByText('Orders · v2')).toBeInTheDocument()
    expect(screen.getByText('*')).toBeInTheDocument()
  })

  it('renders a dedicated key/value mapper declared by the manifest', () => {
    const onChange = vi.fn()
    render(<ParameterField name="values" onChange={onChange} parameters={{}} schema={{ type: 'object' }} ui={{ control: 'mapper' }} value={{ total: 'gross' }} />)

    expect(screen.getByTestId('mapper-control')).toBeInTheDocument()
    fireEvent.change(screen.getByLabelText('Value'), { target: { value: 'net' } })
    expect(onChange).toHaveBeenCalledWith({ total: 'net' })
  })

  it('edits object and array parameters through visible field controls', () => {
    const onChange = vi.fn()
    render(
      <ParameterField
        name="arguments"
        onChange={onChange}
        parameters={{}}
        schema={{
          type: 'object',
          properties: {
            text: { type: 'string', title: 'Text' },
            tags: { type: 'array', title: 'Tags', items: { type: 'string' } },
          },
        }}
        ui={{ control: 'json' }}
        value={{ text: '', tags: [] }}
      />,
    )

    const field = screen.getByTestId('parameter-arguments')
    fireEvent.change(within(field).getByRole('textbox'), {
      target: { value: 'Agentx E2E' },
    })
    expect(onChange).toHaveBeenCalledWith({ text: 'Agentx E2E', tags: [] })

    fireEvent.click(within(field).getByRole('button', { name: 'Add item' }))
    expect(onChange).toHaveBeenCalledWith({ text: '', tags: [''] })
    expect(within(field).queryByLabelText('code-editor')).not.toBeInTheDocument()
  })

  it('supports recursively typed values when the manifest leaves JSON children open', () => {
    const onChange = vi.fn()
    function Harness() {
      const [value, setValue] = useState<unknown[]>([])
      return <ParameterField name="items" onChange={(next) => { onChange(next); setValue(next as unknown[]) }} parameters={{}} schema={{ type: 'array' }} ui={{ control: 'json' }} value={value} />
    }
    render(<Harness />)

    const field = screen.getByTestId('parameter-items')
    fireEvent.click(within(field).getByRole('button', { name: 'Add item' }))
    const item = within(field).getByTestId('json-array-item')
    fireEvent.click(within(item).getByRole('combobox', { name: 'Value type' }))
    fireEvent.click(screen.getByRole('option', { name: 'Object' }))
    fireEvent.click(within(item).getByRole('button', { name: 'Add field' }))
    fireEvent.change(within(item).getByRole('textbox', { name: 'Key' }), { target: { value: 'nested' } })
    fireEvent.blur(within(item).getByRole('textbox', { name: 'Key' }))
    fireEvent.change(within(item).getByRole('textbox', { name: 'Nested' }), { target: { value: 'ok' } })

    expect(onChange).toHaveBeenLastCalledWith([{ nested: 'ok' }])
  })

  it('keeps recursively dynamic JSON literals on their selected scalar type', () => {
    render(
      <ParameterField
        name="items"
        onChange={vi.fn()}
        parameters={{}}
        referenceCatalog={referenceCatalog}
        schema={{
          type: 'array',
          "x-agentx-dynamicValue": { modes: ['literal', 'reference'], allowedNamespaces: ['inputs'], acceptedCardinality: ['single'], missingPolicies: ['error'], recursive: true },
        }}
        ui={{ control: 'json' }}
        value={[{ kind: 'literal', value: 7 }]}
      />,
    )

    const field = screen.getByTestId('parameter-items')
    expect(within(field).getByRole('combobox', { name: 'Value type' })).toHaveTextContent('Number')
    expect(within(field).getByRole('textbox', { name: 'Value' })).toHaveTextContent('7')
  })

  it('uses manifest localization paths for nested array fields and enum options', () => {
    render(
      <ParameterField
        name="messages"
        nestedLocalization={{
          label: (path) => path === 'messages[].role' ? '角色' : path,
          description: (path) => path === 'messages[].role' ? '消息发送者' : undefined,
          placeholder: () => undefined,
          enumLabel: (_path, value) => value === 'user' ? '用户' : value,
        }}
        onChange={vi.fn()}
        parameters={{}}
        schema={{ type: 'array', items: { type: 'object', properties: { role: { type: 'string', enum: ['user', 'assistant'] } }, additionalProperties: false } }}
        ui={{ control: 'json' }}
        value={[{ role: 'user' }]}
      />,
    )

    expect(screen.getByText('角色')).toBeInTheDocument()
    expect(screen.getByText('消息发送者')).toBeInTheDocument()
    expect(screen.getByRole('combobox', { name: '角色' })).toHaveTextContent('用户')
  })

  it('exposes stable manifest paths for fixed object children', () => {
    const onChange = vi.fn()
    render(
      <ParameterField
        name="operations"
        onChange={onChange}
        parameters={{}}
        schema={{
          type: 'array',
          items: {
            type: 'object',
            properties: {
              operation: { type: 'string', enum: ['count', 'sum'] },
              outputField: { type: 'string' },
            },
            additionalProperties: false,
          },
        }}
        ui={{ control: 'json' }}
        value={[{ operation: 'count', outputField: '' }]}
      />,
    )

    const operation = document.querySelector<HTMLElement>('[data-field-path="operations[].operation"]')
    const outputField = document.querySelector<HTMLElement>('[data-field-path="operations[].outputField"]')
    expect(operation).not.toBeNull()
    expect(outputField).not.toBeNull()
    fireEvent.change(within(outputField!).getByRole('textbox'), { target: { value: 'count' } })
    expect(onChange).toHaveBeenCalledWith([{ operation: 'count', outputField: 'count' }])
  })

  it('enables references only when the exact parameter schema declares them', () => {
    const rendered = render(
      <ParameterField
        name="prompt"
        onChange={vi.fn()}
        parameters={{}}
        referenceCatalog={referenceCatalog}
        schema={{ type: 'string' }}
        ui={{ control: 'prompt' }}
        value=""
      />,
    )

    fireEvent.focus(screen.getByRole('textbox'))
    expect(screen.queryByTestId('reference-picker')).not.toBeInTheDocument()

    rendered.rerender(
      <ParameterField
        name="userQuestion"
        onChange={vi.fn()}
        parameters={{}}
        referenceCatalog={referenceCatalog}
        schema={{ type: 'string', "x-agentx-dynamicValue": { modes: ['literal', 'reference'], allowedNamespaces: ['inputs'], acceptedCardinality: ['single'], missingPolicies: ['error'], recursive: false } }}
        ui={{ control: 'text' }}
        value=""
      />,
    )
    fireEvent.focus(screen.getByRole('textbox'))
    expect(screen.getByTestId('reference-picker')).toBeInTheDocument()
  })

  it('preserves a selected variable chip after the parent stores the dynamic value', async () => {
    function Harness() {
      const [value, setValue] = useState<unknown>('')
      return (
        <ParameterField
          name="userQuestion"
          onChange={setValue}
          parameters={{}}
          referenceCatalog={referenceCatalog}
          schema={{ type: 'string', "x-agentx-dynamicValue": { modes: ['literal', 'reference'], allowedNamespaces: ['inputs'], acceptedCardinality: ['single'], missingPolicies: ['error'], recursive: false } }}
          ui={{ control: 'text' }}
          value={value}
        />
      )
    }
    render(<Harness />)

    fireEvent.focus(screen.getByRole('textbox', { name: 'Value' }))
    fireEvent.click(screen.getByRole('button', { name: /输入|Inputs/ }))
    fireEvent.click(screen.getByRole('button', { name: /question/i }))

    await waitFor(() => {
      expect(screen.getByTestId('parameter-userQuestion').querySelector('[data-agentx-variable]')).toBeInTheDocument()
      expect(screen.getByRole('textbox', { name: 'Value' })).not.toHaveTextContent('[object Object]')
      expect(screen.queryByTestId('reference-picker')).not.toBeInTheDocument()
    })
  })

  it('shows units next to numeric node parameters', () => {
    render(<ParameterField name="maxOutputTokens" onChange={vi.fn()} parameters={{}} schema={{ type: 'integer' }} ui={{ control: 'number', unit: 'tokens' }} value={4096} />)

    expect(screen.getByText('tokens')).toBeInTheDocument()
  })
})
