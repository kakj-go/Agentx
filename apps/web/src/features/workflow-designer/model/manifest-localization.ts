import type { NodeManifest, NodeManifestLocalization } from './types'

export type LocalizedManifest = {
  displayName: string
  description: string
  keywords: string[]
  inputPortLabel: (name: string) => string
  outputPortLabel: (name: string) => string
  bindingSlotLabel: (name: string) => string
}

export function localizeManifest(manifest: NodeManifest, language?: string): LocalizedManifest {
  const locale = normalizeLocale(language)
  const current = manifest.localizations?.[locale]
  const english = manifest.localizations?.['en-US']
  const resolve = <T>(read: (value?: NodeManifestLocalization) => T | undefined, fallback: T) => read(current) ?? read(english) ?? fallback
  return {
    displayName: resolve((value) => value?.displayName, manifest.displayName),
    description: resolve((value) => value?.description, manifest.description),
    keywords: [...new Set([
      ...manifest.keywords,
      ...Object.values(manifest.localizations ?? {}).flatMap((value) => value.keywords ?? []),
      manifest.displayName,
      manifest.nodeType,
    ])],
    inputPortLabel: (name) => resolve((value) => value?.inputPortLabels?.[name], name === 'main' ? (locale === 'zh-CN' ? '输入' : 'Input') : fallbackPortLabel(name, locale)),
    outputPortLabel: (name) => resolve((value) => value?.outputPortLabels?.[name], fallbackPortLabel(name, locale)),
    bindingSlotLabel: (name) => resolve((value) => value?.bindingSlotLabels?.[name], fallbackSlotLabel(name, locale)),
  }
}

export function localizedNodeLabel(manifest: NodeManifest, savedLabel: string, language?: string) {
  const localized = localizeManifest(manifest, language)
  const defaultNames = new Set([manifest.displayName, ...Object.values(manifest.localizations ?? {}).map((value) => value.displayName).filter(Boolean)])
  return !savedLabel || defaultNames.has(savedLabel) ? localized.displayName : savedLabel
}

export function manifestSearchText(manifest: NodeManifest) {
  return [
    manifest.nodeType,
    manifest.displayName,
    manifest.description,
    ...manifest.keywords,
    ...Object.values(manifest.localizations ?? {}).flatMap((value) => [
      value.displayName,
      value.description,
      ...(value.keywords ?? []),
    ]),
  ].filter((value): value is string => Boolean(value)).join(' ').toLowerCase()
}

function normalizeLocale(language?: string) {
  return language?.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en-US'
}

function fallbackPortLabel(name: string, locale: string) {
  const labels: Record<string, [string, string]> = {
    main: ['输出', 'Output'], input: ['输入', 'Input'], error: ['错误', 'Error'], true: ['为真', 'True'], false: ['为假', 'False'],
    loop: ['循环', 'Loop'], done: ['完成', 'Done'], recovered: ['已恢复', 'Recovered'], approved: ['已批准', 'Approved'], rejected: ['已拒绝', 'Rejected'],
  }
  const label = labels[name]
  return label ? label[locale === 'zh-CN' ? 0 : 1] : name.replaceAll('_', ' ')
}

function fallbackSlotLabel(name: string, locale: string) {
  const key = name.replace(/^ai_/, '')
  const labels: Record<string, [string, string]> = {
    model: ['模型', 'Model'], tool: ['工具', 'Tool'], memory: ['记忆', 'Memory'], rag: ['知识', 'Knowledge'], knowledge: ['知识', 'Knowledge'], skill: ['技能', 'Skill'],
  }
  const label = labels[key]
  return label ? label[locale === 'zh-CN' ? 0 : 1] : key.replaceAll('_', ' ')
}
