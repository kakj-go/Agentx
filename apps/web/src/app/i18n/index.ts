import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'

import type { SupportedLocale } from '../../shared/types/app'
import { resources } from './resources'

const localeStorageKey = 'agentx.locale'
const supportedLocales: SupportedLocale[] = ['zh-CN', 'en-US']

function initialLocale(): SupportedLocale {
  const stored = window.localStorage.getItem(localeStorageKey)
  if (supportedLocales.includes(stored as SupportedLocale)) return stored as SupportedLocale
  return window.navigator.language.toLowerCase().startsWith('en') ? 'en-US' : 'zh-CN'
}

void i18n.use(initReactI18next).init({
  resources,
  lng: initialLocale(),
  fallbackLng: 'zh-CN',
  interpolation: { escapeValue: false },
})

document.documentElement.lang = i18n.language

i18n.on('languageChanged', (locale) => {
  const supported = supportedLocales.includes(locale as SupportedLocale) ? locale : 'zh-CN'
  window.localStorage.setItem(localeStorageKey, supported)
  document.documentElement.lang = supported
})

export { i18n, supportedLocales }
