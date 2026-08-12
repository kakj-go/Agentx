import type { TFunction } from 'i18next'

export function permissionLabel(t: TFunction, key: string) {
  const translated = t(`roles.permissionLabels.${key.replace(':', '.')}`)
  return translated === `roles.permissionLabels.${key.replace(':', '.')}` ? t('common.unknownValue', { value: key }) : translated
}

export function dataScopeLabel(t: TFunction, scope: string) {
  const translated = t(`organization.dataScopes.${scope}`)
  return translated === `organization.dataScopes.${scope}` ? t('common.unknownValue', { value: scope }) : translated
}

export function roleLabel(t: TFunction, code: string, fallback?: string) {
  const translated = t(`roles.names.${code}`)
  return translated === `roles.names.${code}` ? (fallback ?? t('common.unknownValue', { value: code })) : translated
}
