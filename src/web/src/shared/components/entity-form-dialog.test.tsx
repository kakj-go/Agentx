import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { describe, expect, it, vi } from 'vitest'

import { ApiClientError } from '../api/client'
import { EntityFormDialog } from './entity-form-dialog'

describe('EntityFormDialog field errors', () => {
  it('does not validate a required field hidden by another field value', async () => {
    const submit = vi.fn().mockResolvedValue(undefined)
    render(<EntityFormDialog cancelLabel="Cancel" fields={[
      { name: 'transport', label: 'Transport', defaultValue: 'http' },
      { name: 'runtimeSandbox', label: 'Runtime Sandbox', required: true, visible: (values) => values.transport === 'stdio' },
    ]} onClose={vi.fn()} onSubmit={submit} open submitLabel="Save" title="Create" />)
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(submit).toHaveBeenCalledWith({ transport: 'http', runtimeSandbox: '' }))
  })

  it('marks and validates required input and select fields before submission', async () => {
    const submit = vi.fn()
    render(<EntityFormDialog cancelLabel="Cancel" fields={[{ name: 'name', label: 'Name', required: true }, { name: 'department', label: 'Department', type: 'select', required: true, options: [{ value: 'one', label: 'One' }] }]} onClose={vi.fn()} onSubmit={submit} open submitLabel="Save" title="Create" />)

    expect(screen.getAllByText('*', { exact: true })).toHaveLength(2)
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))

    const name = screen.getByRole('textbox', { name: 'Name' })
    const department = screen.getByRole('combobox', { name: 'Department' })
    await waitFor(() => expect(name).toHaveFocus())
    expect(name).toHaveAttribute('aria-invalid', 'true')
    expect(department).toHaveAttribute('aria-invalid', 'true')
    expect(department).toHaveAttribute('aria-required', 'true')
    expect(screen.getAllByText(/This field is required|此项为必填项/)).toHaveLength(2)
    expect(submit).not.toHaveBeenCalled()

    fireEvent.change(name, { target: { value: 'Example' } })
    fireEvent.click(department)
    fireEvent.click(await screen.findByRole('option', { name: 'One' }))
    expect(screen.queryByText(/This field is required|此项为必填项/)).not.toBeInTheDocument()
  })

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

  it('maps API field names and keeps unmatched field failures in the summary', async () => {
    const submit = vi.fn().mockRejectedValue(new ApiClientError(422, {
      code: 'INVALID_REQUEST_BODY', message: 'Invalid request body', requestId: 'request-3',
      fieldErrors: [
        { field: 'ownerDepartmentId', code: 'INVALID_FIELD', message: 'invalid department' },
        { field: 'unknownField', code: 'INVALID_FIELD', message: 'invalid unknown field' },
      ],
    }))
    render(<EntityFormDialog cancelLabel="Cancel" fields={[{ name: 'department', apiName: 'ownerDepartmentId', label: 'Department', type: 'select', defaultValue: 'one', options: [{ value: 'one', label: 'One' }] }]} onClose={vi.fn()} onSubmit={submit} open submitLabel="Save" title="Create" />)

    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    expect(await screen.findByText(/This field has an invalid value|此项的格式不正确/)).toBeInTheDocument()
    expect(screen.getByText(/The submitted form contains missing or invalid values|提交内容存在缺失项或格式错误/)).toBeInTheDocument()
  })
})
