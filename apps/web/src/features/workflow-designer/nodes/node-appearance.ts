import type { NodeManifest, ResourceType } from '../model/types'

export type NodeCategory = 'triggers' | 'flow' | 'ai' | 'data' | 'integrations' | 'code' | 'other'

const categoryOrder: NodeCategory[] = ['triggers', 'flow', 'ai', 'data', 'integrations', 'code', 'other']

export function nodeCategory(manifest: NodeManifest): NodeCategory {
  const category = manifest.category.toLowerCase()
  if (manifest.executionStyle === 'trigger') return 'triggers'
  if (['rag', 'memory'].includes(manifest.capability)) return 'data'
  if (manifest.capability === 'agent' || ['model', 'mcp_tool', 'skill'].includes(manifest.capability)) return 'ai'
  if (manifest.capability === 'sandbox') return 'code'
  if (categoryOrder.includes(category as NodeCategory)) return category as NodeCategory
  if (manifest.executionStyle === 'action') return 'integrations'
  return 'other'
}

export function nodeShape(manifest?: NodeManifest) {
  if (!manifest) return 'action'
  if (manifest.executionStyle === 'trigger') return 'trigger'
  if (manifest.nodeType === 'if' || manifest.nodeType === 'switch') return 'branch'
  if (manifest.category === 'flow' || manifest.executionStyle === 'suspend' || manifest.executionStyle === 'sub_workflow') return 'flow'
  if (manifest.capability === 'agent') return 'agent'
  if (manifest.capability === 'sandbox') return 'code'
  if (['model', 'mcp_tool', 'skill'].includes(manifest.capability)) return 'ai'
  if (['rag', 'memory'].includes(manifest.capability)) return 'data'
  return 'action'
}

export function categoryLabel(category: NodeCategory, translate: (key: string) => string) {
  return translate(`studio.palette.categories.${category}`)
}

export const resourceIcon = (type: ResourceType) => ({ model: 'brain-circuit', mcp_tool: 'wrench', memory: 'memory-stick', rag: 'database', skill: 'sparkles' } as Partial<Record<ResourceType, string>>)[type] ?? 'box'
