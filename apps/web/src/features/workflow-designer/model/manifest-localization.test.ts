import { describe, expect, it } from 'vitest'

import { localizeManifest, localizedNodeLabel, manifestSearchText } from './manifest-localization'
import type { NodeManifest } from './types'

const manifest: NodeManifest = {
  protocolVersion: '1.0', nodeType: 'if', version: 1, displayName: 'If', description: 'Route items', category: 'flow', keywords: ['condition'], iconKey: 'split', executionStyle: 'action', capability: 'builtin', readiness: 'any', inputPorts: [{ name: 'main', kind: 'main', required: false, variadic: false }], outputPorts: [{ name: 'true', kind: 'main', required: false, variadic: false }], bindingSlots: [], parameterSchema: {}, uiSchema: { canvas: { role: 'branch' } }, providers: [], credentials: [], retryPolicy: { retryable: false, maxAttempts: 1, initialBackoffMs: 0, maxBackoffMs: 0 }, sandboxRequired: false, supportsMock: true, sideEffectLevel: 'none', localizations: { 'zh-CN': { displayName: '条件', description: '根据条件路由', keywords: ['判断'], inputPortLabels: { main: '输入' }, outputPortLabels: { true: '满足' } }, 'en-US': { displayName: 'If', description: 'Route items', keywords: ['condition'], inputPortLabels: { main: 'Input' }, outputPortLabels: { true: 'True' } } },
}

describe('manifest localization', () => {
  it('resolves labels and protocol port ids without a type allowlist', () => {
    const localized = localizeManifest(manifest, 'zh-CN')
    expect(localized.displayName).toBe('条件')
    expect(localized.inputPortLabel('main')).toBe('输入')
    expect(localized.outputPortLabel('true')).toBe('满足')
    expect(localized.outputPortLabel('missing')).toBe('missing')
  })

  it('changes default labels with language but preserves custom names', () => {
    expect(localizedNodeLabel(manifest, 'If', 'zh-CN')).toBe('条件')
    expect(localizedNodeLabel(manifest, 'Custom rule', 'zh-CN')).toBe('Custom rule')
  })

  it('indexes base fields and every localization for language-independent search', () => {
    const text = manifestSearchText(manifest)
    expect(text).toContain('route items')
    expect(text).toContain('根据条件路由')
    expect(text).toContain('判断')
  })
})
