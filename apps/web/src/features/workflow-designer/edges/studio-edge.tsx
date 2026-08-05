import { BaseEdge, getBezierPath, type EdgeProps } from '@xyflow/react'

import type { StudioEdge } from '../model/types'

export function StudioEdgeComponent(props: EdgeProps<StudioEdge>) {
  const [path] = getBezierPath(props)
  const binding = props.data?.edgeKind === 'binding'
  return <BaseEdge id={props.id} markerEnd={props.markerEnd} path={path} style={{ stroke: binding ? 'var(--ui-warning)' : props.selected ? 'var(--ui-primary)' : 'var(--ui-border-strong)', strokeDasharray: binding ? '5 4' : undefined, strokeWidth: props.selected ? 2.5 : 1.8 }} />
}
