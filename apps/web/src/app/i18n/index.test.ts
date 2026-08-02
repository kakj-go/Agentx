import { afterAll, describe, expect, it } from 'vitest'

import { i18n } from './index'

describe('locale preference', () => {
  afterAll(async () => {
    await i18n.changeLanguage('zh-CN')
  })

  it('updates the document language and persists a manual selection', async () => {
    await i18n.changeLanguage('en-US')
    expect(document.documentElement.lang).toBe('en-US')
    expect(window.localStorage.getItem('agentx.locale')).toBe('en-US')
    expect(i18n.t('nav.organization')).toBe('Departments & Users')

    await i18n.changeLanguage('zh-CN')
    expect(document.documentElement.lang).toBe('zh-CN')
    expect(window.localStorage.getItem('agentx.locale')).toBe('zh-CN')
    expect(i18n.t('nav.organization')).toBe('部门与用户')
  })
})
