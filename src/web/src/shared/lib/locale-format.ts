import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

export function resolveDisplayLocale(language?: string) {
  return language?.toLowerCase().startsWith('en') ? 'en-US' : 'zh-CN'
}

export function formatDateTime(value: string | number | Date | null | undefined, language?: string) {
  const date = toValidDate(value)
  return date ? date.toLocaleString(resolveDisplayLocale(language)) : '—'
}

export function formatTime(value: string | number | Date | null | undefined, language?: string) {
  const date = toValidDate(value)
  return date ? date.toLocaleTimeString(resolveDisplayLocale(language)) : '—'
}

export function formatNumber(value: number, language?: string) {
  return new Intl.NumberFormat(resolveDisplayLocale(language)).format(value)
}

export function useLocaleFormat() {
  const { i18n } = useTranslation()
  const locale = resolveDisplayLocale(i18n.resolvedLanguage)
  return useMemo(() => ({
    formatDateTime: (value: string | number | Date | null | undefined) => formatDateTime(value, locale),
    formatTime: (value: string | number | Date | null | undefined) => formatTime(value, locale),
    formatNumber: (value: number) => formatNumber(value, locale),
  }), [locale])
}

function toValidDate(value: string | number | Date | null | undefined) {
  if (value === null || value === undefined || value === '') return undefined
  const date = value instanceof Date ? value : new Date(value)
  return Number.isNaN(date.getTime()) ? undefined : date
}
