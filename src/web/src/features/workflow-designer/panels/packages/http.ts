import { HttpPanel } from '../inspectors/http-panel'
import type { BuiltinNodeUiPackage } from './types'

export const httpNodeUiPackage: BuiltinNodeUiPackage = {
  packageId: 'agentx/http',
  packageVersion: '1.0.0',
  panels: { declarative_http: HttpPanel },
}
