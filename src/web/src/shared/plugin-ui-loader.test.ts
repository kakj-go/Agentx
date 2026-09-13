import { afterEach, expect, it } from 'vitest'

import { installPluginStyles, loadPluginUi, pluginUiLoaderDiagnostics } from './plugin-ui-loader'

afterEach(() => {
  for (const element of document.head.querySelectorAll('[data-agentx-plugin]')) element.remove()
})

it('reference-counts plugin CSS across repeated mount and unmount', () => {
  const first = installPluginStyles('.plugin { color: red }', 'sha256:styles')
  const second = installPluginStyles('.plugin { color: red }', 'sha256:styles')
  expect(document.head.querySelectorAll('[data-agentx-plugin="sha256:styles"]')).toHaveLength(1)
  first()
  expect(document.head.querySelectorAll('[data-agentx-plugin="sha256:styles"]')).toHaveLength(1)
  second()
  expect(document.head.querySelectorAll('[data-agentx-plugin="sha256:styles"]')).toHaveLength(0)
  expect(pluginUiLoaderDiagnostics().installedStylesheets).toBe(0)
})

it('bounds immutable ESM module references while retaining hot entries', async () => {
  const source = 'export const createUi=()=>({Panel:()=>null})'
  for (let index = 0; index < 70; index += 1) await loadPluginUi(source, `sha256:${index}`)
  expect(pluginUiLoaderDiagnostics().cachedModules).toBeLessThanOrEqual(64)
  await expect(loadPluginUi(source, 'sha256:69')).resolves.toHaveProperty('createUi')
})
