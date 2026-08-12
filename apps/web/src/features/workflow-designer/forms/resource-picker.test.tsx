import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { describe, expect, it, vi } from 'vitest'

import type { ResourceOption } from '../model/types'
import { ResourcePicker } from './resource-picker'

const grantable: ResourceOption = {
  value: 'model-1',
  label: 'GPT Model',
  resourceType: 'model',
  accessState: 'grantable',
  requirements: [{ resourceType: 'model', resourceId: 'model-1', operation: 'use', name: 'GPT Model', authorized: false, active: true }],
}

describe('ResourcePicker', () => {
  it('keeps a grantable row unselectable while its authorization action remains interactive', async () => {
    const onChange = vi.fn()
    const onAuthorize = vi.fn().mockResolvedValue(undefined)
    render(<MemoryRouter><ResourcePicker onAuthorize={onAuthorize} onChange={onChange} options={[grantable]} /></MemoryRouter>)

    fireEvent.click(screen.getByRole('combobox', { name: /Select a resource|选择资源/ }))
    expect(screen.getByRole('option', { name: /GPT Model/ })).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: /Authorize|授权/ }))
    fireEvent.click(screen.getByRole('button', { name: /Authorize|授权/ }))

    await waitFor(() => expect(onAuthorize).toHaveBeenCalledWith(grantable))
    expect(onChange).not.toHaveBeenCalled()
  })

  it('selects only an authorized resource', () => {
    const onChange = vi.fn()
    render(<MemoryRouter><ResourcePicker onChange={onChange} options={[{ ...grantable, accessState: 'authorized' }]} /></MemoryRouter>)

    fireEvent.click(screen.getByRole('combobox', { name: /Select a resource|选择资源/ }))
    fireEvent.click(screen.getByRole('option', { name: /GPT Model/ }))
    expect(onChange).toHaveBeenCalledWith('model-1', undefined, 'GPT Model')
  })

  it('retains and marks a selected resource that is no longer returned by the server', () => {
    render(<MemoryRouter><ResourcePicker onChange={vi.fn()} options={[]} value="hidden-model" /></MemoryRouter>)

    const trigger = screen.getByRole('combobox', { name: /Selected resource unavailable|已选资源不可用/ })
    expect(trigger).toHaveAttribute('aria-invalid', 'true')
  })

  it('marks a still-visible selected resource after its grant is revoked', () => {
    render(<MemoryRouter><ResourcePicker onChange={vi.fn()} options={[{ ...grantable, accessState: 'requestable' }]} value="model-1" /></MemoryRouter>)

    const trigger = screen.getByRole('combobox', { name: /Selected resource unavailable|已选资源不可用/ })
    expect(trigger).toHaveAttribute('aria-invalid', 'true')
    expect(trigger).not.toHaveTextContent('GPT Model')
  })

  it('submits an optional request note without selecting the resource', async () => {
    const onChange = vi.fn()
    const onRequest = vi.fn().mockResolvedValue(undefined)
    render(<MemoryRouter><ResourcePicker onChange={onChange} onRequest={onRequest} options={[{ ...grantable, accessState: 'requestable' }]} /></MemoryRouter>)

    fireEvent.click(screen.getByRole('combobox', { name: /Select a resource|选择资源/ }))
    fireEvent.click(screen.getByRole('button', { name: /Request|申请/ }))
    fireEvent.change(screen.getByLabelText(/Request note|申请说明/), { target: { value: 'Needed by the agent node' } })
    fireEvent.click(screen.getByRole('button', { name: /Submit request|提交申请/ }))

    await waitFor(() => expect(onRequest).toHaveBeenCalledWith(expect.objectContaining({ value: 'model-1' }), 'Needed by the agent node'))
    expect(onChange).not.toHaveBeenCalled()
  })
})
