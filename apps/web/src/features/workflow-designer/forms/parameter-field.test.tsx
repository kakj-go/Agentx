import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ParameterField } from './parameter-field'

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
})
