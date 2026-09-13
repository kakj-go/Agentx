import { describe, expect, it } from 'vitest'

import { formatDateTime, formatNumber, resolveDisplayLocale } from './locale-format'

describe('locale formatting', () => {
  it('normalizes supported display locales', () => {
    expect(resolveDisplayLocale('zh-Hans')).toBe('zh-CN')
    expect(resolveDisplayLocale('en-GB')).toBe('en-US')
  })

  it('formats numbers with the selected locale', () => {
    expect(formatNumber(1234567, 'en-US')).toBe('1,234,567')
    expect(formatNumber(1234567, 'zh-CN')).toBe('1,234,567')
  })

  it('never renders an invalid date', () => {
    expect(formatDateTime('not-a-date', 'zh-CN')).toBe('—')
    expect(formatDateTime(undefined, 'en-US')).toBe('—')
  })
})
