import { coreNodeUiPackage } from './core'
import { dataNodeUiPackage } from './data'
import { httpNodeUiPackage } from './http'
import type { BuiltinNodePanel, BuiltinNodeUiPackage } from './types'

const packages: BuiltinNodeUiPackage[] = [coreNodeUiPackage, dataNodeUiPackage, httpNodeUiPackage]

export type { BuiltinNodePanel }

export function resolveBuiltinNodePanel(packageId: string, packageVersion: string, nodeType: string) {
  return packages.find((item) => item.packageId === packageId && item.packageVersion === packageVersion)?.panels[nodeType]
}
