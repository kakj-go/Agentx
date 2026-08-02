import { act, fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it } from 'vitest'

import { ThemeProvider, useTheme } from './theme-provider'

function ThemeProbe() {
  const { preference, resolvedTheme, setPreference } = useTheme()
  return <button onClick={() => setPreference('dark')}>{preference}:{resolvedTheme}</button>
}

describe('ThemeProvider', () => {
  beforeEach(() => {
    window.localStorage.clear()
    document.documentElement.className = ''
  })

  it('defaults to the system preference and persists an override', () => {
    render(<ThemeProvider><ThemeProbe /></ThemeProvider>)
    expect(screen.getByRole('button')).toHaveTextContent('system:light')
    act(() => fireEvent.click(screen.getByRole('button')))
    expect(screen.getByRole('button')).toHaveTextContent('dark:dark')
    expect(window.localStorage.getItem('agentx.theme')).toBe('dark')
    expect(document.documentElement).toHaveClass('dark')
  })
})
