import type { ComponentType } from 'react'

export type ThemePreference = 'system' | 'light' | 'dark'

export type SupportedLocale = 'zh-CN' | 'en-US'

export type NavigationItem = {
  labelKey: string
  path: string
  icon: ComponentType<{ className?: string }>
  badge?: string
  keywords?: string[]
}

export type NavigationGroup = {
  labelKey: string
  items: NavigationItem[]
}

export type NotificationItem = {
  id: string
  titleKey: string
  descriptionKey: string
  path: string
  tone: 'primary' | 'warning' | 'success'
  timeKey: string
}
