/* oxlint-disable react/only-export-components */
import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'

import type { ThemePreference } from '../../shared/types/app'

const themeStorageKey = 'agentx.theme'
const themeMediaQuery = '(prefers-color-scheme: dark)'

type ThemeContextValue = {
  preference: ThemePreference
  resolvedTheme: 'light' | 'dark'
  setPreference: (preference: ThemePreference) => void
}

const ThemeContext = createContext<ThemeContextValue | null>(null)

function readPreference(): ThemePreference {
  const stored = window.localStorage.getItem(themeStorageKey)
  return stored === 'light' || stored === 'dark' || stored === 'system' ? stored : 'system'
}

function resolveTheme(preference: ThemePreference, systemDark: boolean) {
  return preference === 'system' ? (systemDark ? 'dark' : 'light') : preference
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [preference, setPreferenceState] = useState<ThemePreference>(readPreference)
  const [systemDark, setSystemDark] = useState(() => window.matchMedia(themeMediaQuery).matches)
  const resolvedTheme = resolveTheme(preference, systemDark)

  useEffect(() => {
    const media = window.matchMedia(themeMediaQuery)
    const listener = (event: MediaQueryListEvent) => setSystemDark(event.matches)
    media.addEventListener('change', listener)
    return () => media.removeEventListener('change', listener)
  }, [])

  useEffect(() => {
    document.documentElement.classList.toggle('dark', resolvedTheme === 'dark')
    document.documentElement.dataset.theme = resolvedTheme
    document.documentElement.style.colorScheme = resolvedTheme
  }, [resolvedTheme])

  const value = useMemo<ThemeContextValue>(() => ({
    preference,
    resolvedTheme,
    setPreference: (next) => {
      window.localStorage.setItem(themeStorageKey, next)
      setPreferenceState(next)
    },
  }), [preference, resolvedTheme])

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
}

export function useTheme() {
  const context = useContext(ThemeContext)
  if (!context) throw new Error('useTheme must be used inside ThemeProvider')
  return context
}
