import { describe, expect, it } from 'vitest'

import { resources } from './i18n/resources'
import { navigationItems } from './navigation'

describe('navigation contract', () => {
  it('contains every primary route exactly once', () => {
    const paths = navigationItems.map((item) => item.path)
    expect(paths).toHaveLength(17)
    expect(new Set(paths).size).toBe(paths.length)
    expect(paths).toEqual(expect.arrayContaining(['/', '/workflows', '/applications', '/playground', '/executions', '/approvals', '/datasets', '/evaluations', '/credentials', '/models', '/mcp', '/skills', '/knowledge', '/memory', '/resource-grants', '/organization', '/roles']))
  })

  it('provides a Chinese and English label for every item', () => {
    for (const item of navigationItems) {
      const key = item.labelKey.replace('nav.', '') as keyof typeof resources['zh-CN']['translation']['nav']
      expect(resources['zh-CN'].translation.nav[key]).toBeTruthy()
      expect(resources['en-US'].translation.nav[key]).toBeTruthy()
    }
  })
})
