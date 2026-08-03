import { fireEvent, render, screen } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { MemoryRouter } from 'react-router-dom'
import { describe, expect, it, vi } from 'vitest'

import { i18n } from '../../app/i18n'
import { PrerequisiteAction } from './prerequisite-action'

function renderAction(met: boolean, ready = vi.fn()) {
  render(<I18nextProvider i18n={i18n}><MemoryRouter><PrerequisiteAction onReady={ready} requirements={[{ key: 'workflow', label: 'Published workflow', met, href: '/workflows', actionLabel: 'Create workflow' }]}>Create</PrerequisiteAction></MemoryRouter></I18nextProvider>)
  return ready
}

describe('PrerequisiteAction', () => {
  it('explains missing data and links to the dependency', () => {
    const ready = renderAction(false)
    fireEvent.click(screen.getByRole('button', { name: 'Create' }))
    expect(screen.getByRole('dialog')).toBeVisible()
    expect(screen.getByRole('link', { name: 'Create workflow' })).toHaveAttribute('href', '/workflows')
    expect(ready).not.toHaveBeenCalled()
  })

  it('runs the action immediately when every dependency is ready', () => {
    const ready = renderAction(true)
    fireEvent.click(screen.getByRole('button', { name: 'Create' }))
    expect(ready).toHaveBeenCalledOnce()
  })
})
