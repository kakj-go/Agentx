import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ConfirmDialog } from './confirm-dialog'

describe('ConfirmDialog', () => {
  it('keeps destructive work behind explicit cancel and confirm actions', async () => {
    const close = vi.fn()
    const confirm = vi.fn(async () => undefined)
    render(<ConfirmDialog cancelLabel="Cancel" confirmLabel="Delete" description="Cannot be undone" onClose={close} onConfirm={confirm} open title="Delete case" />)

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(close).toHaveBeenCalledOnce()
    expect(confirm).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'Delete' }))
    await waitFor(() => expect(confirm).toHaveBeenCalledOnce())
  })
})
