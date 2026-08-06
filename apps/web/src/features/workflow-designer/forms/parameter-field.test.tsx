import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ParameterField } from './parameter-field'

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
    render(<ParameterField name="values" onChange={onChange} parameters={{}} schema={{ type: 'object' }} ui={{ control: 'mapper' }} value={{ total: '=$json.amount' }} />)

    expect(screen.getByTestId('mapper-control')).toBeInTheDocument()
    fireEvent.change(screen.getByLabelText('Value'), { target: { value: '=$json.net' } })
    expect(onChange).toHaveBeenCalledWith({ total: '=$json.net' })
  })

  it('preserves an invalid JSON intermediate value until it becomes valid', async () => {
    const onChange = vi.fn()
    render(<ParameterField name="arguments" onChange={onChange} parameters={{}} schema={{ type: 'object' }} ui={{ control: 'json' }} value={{}} />)

    const editor = screen.getByLabelText('code-editor')
    fireEvent.change(editor, { target: { value: '{"text":' } })
    await waitFor(() => expect(editor).toHaveValue('{"text":'))
    expect(onChange).not.toHaveBeenCalled()

    fireEvent.change(editor, { target: { value: '{"text":"Agentx E2E"}' } })
    expect(onChange).toHaveBeenCalledWith({ text: 'Agentx E2E' })
  })
})
