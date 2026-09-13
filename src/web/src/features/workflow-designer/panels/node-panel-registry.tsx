import type { NodeManifest } from '../model/types'
import { PluginPanel } from './inspectors/plugin-panel'
import { resolveBuiltinNodePanel, type BuiltinNodePanel } from './packages'

export type NodePanelComponent = BuiltinNodePanel

export function resolveNodePanel(manifest: NodeManifest, definitionNodeType = manifest.nodeType): NodePanelComponent | undefined {
  const packageId = manifest.plugin?.packageId
  const packageVersion = manifest.plugin?.packageVersion
  return (packageId && packageVersion ? resolveBuiltinNodePanel(packageId, packageVersion, definitionNodeType) : undefined)
    ?? (manifest.capability === 'plugin_nodejs' ? PluginPanel : undefined)
}
