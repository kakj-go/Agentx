import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { vi, describe, expect, it } from 'vitest'

import { ThemeProvider } from '../../../../app/providers/theme-provider'
import { studioManifest } from '../../testing/studio-catalog'
import { PluginPanel } from './plugin-panel'

vi.mock('../../../../shared/plugin-ui-loader', () => {
  return {
    installPluginStyles: () => () => undefined,
    loadPluginUi: async () => ({
      createUi: () => ({
        Panel: ({ readOnly, updateParameters }: { readOnly: boolean; updateParameters: (patch: Record<string, unknown>) => void }) => <button disabled={readOnly} onClick={() => updateParameters({ label: 'changed' })}>Plugin edit</button>,
      }),
    }),
    PluginUiBoundary: ({ children }: { children: ReactNode }) => children,
  }
})

describe('Plugin panel host context', () => {
  it('keeps an imported panel read-only and rejects its parameter patch', async () => {
    const manifest = { ...studioManifest('set'), nodeType: 'acme.readonly', capability: 'plugin_nodejs' as const, plugin: { packageId: 'acme/readonly', packageVersion: '1.0.0', bundleDigest: `sha256:${'a'.repeat(64)}`, runtimeEntry: 'runtime.js', runtimeSource: 'export function execute(){}', uiEntry: 'ui.js', uiSource: 'export function createUi(){}', uiStyles: '', uiAssets: {}, traceRenderers: [] } }
    const onChange = vi.fn()
    render(<ThemeProvider><PluginPanel data={{ editorKind: 'action', nodeType: manifest.nodeType, typeVersion: 1, label: 'Read only', key: 'read_only', parameters: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }} fieldErrors={{}} manifest={manifest} onChange={onChange} providerOptions={{}} readOnly resources={{}} /></ThemeProvider>)
    const button = await screen.findByRole('button', { name: 'Plugin edit' })
    expect(button).toBeDisabled()
    fireEvent.click(button)
    await waitFor(() => expect(onChange).not.toHaveBeenCalled())
  })
})
