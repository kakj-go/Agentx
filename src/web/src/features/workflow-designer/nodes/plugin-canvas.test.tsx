import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { expect, it, vi } from 'vitest'

import { ThemeProvider } from '../../../app/providers/theme-provider'
import { studioManifest } from '../testing/studio-catalog'
import { PluginCanvas } from './plugin-canvas'

const created = vi.hoisted(() => vi.fn())
vi.mock('../../../shared/plugin-ui-loader', () => ({
  installPluginStyles: () => () => undefined,
  loadPluginUi: async () => ({ createUi: () => {
    created()
    return { Canvas: function Canvas() {
      const [count, setCount] = useState(0)
      return <button onClick={() => setCount(count + 1)}>Local count {count}</button>
    } }
  } }),
  PluginUiBoundary: ({ children }: { children: React.ReactNode }) => children,
}))

it('keeps canvas component state when only resolved ports and schemas change', async () => {
  const base = studioManifest('set')
  const manifest = { ...base, plugin: { ...base.plugin!, uiSource: 'source', bundleDigest: 'sha256:fixed' } }
  const view = render(<ThemeProvider><PluginCanvas manifest={manifest} parameters={{}} /></ThemeProvider>)
  fireEvent.click(await screen.findByRole('button', { name: 'Local count 0' }))
  view.rerender(<ThemeProvider><PluginCanvas manifest={{ ...manifest, outputSchema: { type: 'number' } }} parameters={{ changed: true }} /></ThemeProvider>)
  expect(screen.getByRole('button', { name: 'Local count 1' })).toBeInTheDocument()
  expect(created).toHaveBeenCalledTimes(1)
})
