import type { TFunction } from 'i18next'

export function permissionLabel(t: TFunction, key: string) {
  const translated = t(`permissionLabels.${key.replace(':', '.')}`)
  return translated === `permissionLabels.${key.replace(':', '.')}` ? key : translated
}

export function dataScopeLabel(t: TFunction, scope: string) {
  const translated = t(`dataScopes.${scope}`)
  return translated === `dataScopes.${scope}` ? scope : translated
}

export function roleLabel(t: TFunction, code: string, fallback?: string) {
  const translated = t(`roleNames.${code}`)
  return translated === `roleNames.${code}` ? (fallback ?? code) : translated
}
