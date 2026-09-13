import type { TFunction } from 'i18next'

export function localizedValue(t: TFunction, path: string, value: string | null | undefined) {
  if (!value) return '—'
  return t(`${path}.${value}`, { defaultValue: t('common.unknownValue', { value }) })
}
