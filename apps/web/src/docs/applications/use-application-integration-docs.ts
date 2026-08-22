import { useTranslation } from 'react-i18next'

import { applicationIntegrationDocs as enUS } from './locales/en-US'
import { applicationIntegrationDocs as zhCN } from './locales/zh-CN'

export function useApplicationIntegrationDocsText() {
  const { i18n } = useTranslation()
  return i18n.resolvedLanguage?.toLowerCase().startsWith('en') ? enUS : zhCN
}
