import type { ActionPanelProps } from '../inspectors/panel-shell'

export type BuiltinNodePanel = (props: ActionPanelProps) => React.ReactNode

export type BuiltinNodeUiPackage = {
  packageId: string
  packageVersion: string
  panels: Record<string, BuiltinNodePanel>
}
