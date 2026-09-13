import { ListPanel } from '../inspectors/list-panel'
import { SetPanel } from '../inspectors/set-panel'
import type { BuiltinNodeUiPackage } from './types'

export const dataNodeUiPackage: BuiltinNodeUiPackage = {
  packageId: 'agentx/data',
  packageVersion: '1.0.0',
  panels: { list: ListPanel, set: SetPanel },
}
