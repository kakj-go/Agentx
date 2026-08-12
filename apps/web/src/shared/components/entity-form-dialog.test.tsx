import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { describe, expect, it, vi } from 'vitest'

import { ApiClientError } from '../api/client'
import { EntityFormDialog } from './entity-form-dialog'

describe('EntityFormDialog field errors', () => {
  it('renders, focuses and clears only the edited field error', async () => {
    const submit = vi.fn().mockRejectedValue(new ApiClientError(409, {
      code: 'MODEL_NAME_EXISTS', message: 'duplicate', requestId: 'request-1',
      fieldErrors: [
        { field: 'alias', code: 'MODEL_NAME_EXISTS', message: 'duplicate alias' },
        { field: 'description', code: 'INVALID_DESCRIPTION', message: 'bad description' },
      ],
    }))
    render(<EntityFormDialog cancelLabel="Cancel" fields={[{ name: 'alias', label: 'Model name' }, { name: 'description', label: 'Description', type: 'textarea' }]} onClose={vi.fn()} onSubmit={submit} open submitLabel="Save" title="Create model" />)

    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    const alias = await screen.findByRole('textbox', { name: 'Model name' })
    await waitFor(() => expect(alias).toHaveFocus())
    expect(alias).toHaveAttribute('aria-invalid', 'true')
    expect(screen.getByText('A model with this name already exists.')).toBeInTheDocument()
    expect(screen.getByText('bad description')).toBeInTheDocument()

    fireEvent.change(alias, { target: { value: 'new-name' } })
    expect(screen.queryByText('A model with this name already exists.')).not.toBeInTheDocument()
    expect(screen.getByText('bad description')).toBeInTheDocument()
  })

  it('keeps values and field errors when the parent rerenders with a new fields array', async () => {
    const duplicate = new ApiClientError(409, {
      code: 'MODEL_NAME_EXISTS', message: 'duplicate', requestId: 'request-1',
      fieldErrors: [{ field: 'alias', code: 'MODEL_NAME_EXISTS', message: 'duplicate alias' }],
    })
    function Harness() {
      const [, rerender] = useState(0)
      return <EntityFormDialog cancelLabel="Cancel" fields={[{ name: 'alias', label: 'Model name' }]} onClose={vi.fn()} onSubmit={async () => { rerender((value) => value + 1); throw duplicate }} open submitLabel="Save" title="Create model" />
    }
    render(<Harness />)
    const alias = screen.getByRole('textbox', { name: 'Model name' })
    fireEvent.change(alias, { target: { value: 'duplicate-name' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    expect(await screen.findByText('A model with this name already exists.')).toBeInTheDocument()

    expect(alias).toHaveValue('duplicate-name')
    expect(alias).toHaveAttribute('aria-invalid', 'true')
    expect(screen.getByText('A model with this name already exists.')).toBeInTheDocument()
  })

  it('connects a select error to the control and clears it after selection', async () => {
    const submit = vi.fn().mockRejectedValue(new ApiClientError(409, {
      code: 'INVALID_OWNER', message: 'invalid owner', requestId: 'request-2',
      fieldErrors: [{ field: 'department', code: 'INVALID_OWNER', message: 'Choose another department' }],
    }))
    render(<EntityFormDialog cancelLabel="Cancel" fields={[{ name: 'department', label: 'Department', type: 'select', defaultValue: 'one', options: [{ value: 'one', label: 'One' }, { value: 'two', label: 'Two' }] }]} onClose={vi.fn()} onSubmit={submit} open submitLabel="Save" title="Create" />)

    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    const select = await screen.findByRole('combobox', { name: 'Department' })
    await waitFor(() => expect(select).toHaveFocus())
    expect(select).toHaveAttribute('aria-invalid', 'true')
    expect(select).toHaveAttribute('aria-describedby', 'department-error')
    expect(screen.getByText('Choose another department')).toHaveAttribute('id', 'department-error')

    fireEvent.click(select)
    fireEvent.click(await screen.findByRole('option', { name: 'Two' }))
    expect(screen.queryByText('Choose another department')).not.toBeInTheDocument()
    expect(select).toHaveAttribute('aria-invalid', 'false')
  })

  it('keeps non-field failures in the form summary', async () => {
    render(<EntityFormDialog cancelLabel="Cancel" fields={[{ name: 'name', label: 'Name' }]} onClose={vi.fn()} onSubmit={vi.fn().mockRejectedValue(new Error('Service is unavailable'))} open submitLabel="Save" title="Create" />)

    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    expect(await screen.findByText('Service is unavailable')).toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: 'Name' })).toHaveAttribute('aria-invalid', 'false')
  })
})
