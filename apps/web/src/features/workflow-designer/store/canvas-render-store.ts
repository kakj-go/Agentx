import { create } from 'zustand'

import type { NodeManifest } from '../model/types'
import type { NodeBindingSummary } from '../utils/graph-index'

export type CanvasZoomTier = 'full' | 'compact' | 'minimal'

type CanvasRenderState = {
  manifests: Map<string, NodeManifest>
  runtimeStatuses: Map<string, string>
  bindingSummaries: Map<string, NodeBindingSummary[]>
  occupiedHandlesByNodeId: Map<string, string>
  zoomTier: CanvasZoomTier
  onQuickAdd?: (nodeId: string, handleId: string, mode: 'output' | 'binding') => void
  onSourceHover?: (nodeId: string, handleId: string, active: boolean) => void
}

export const useCanvasRenderStore = create<CanvasRenderState>(() => ({
  manifests: new Map(), runtimeStatuses: new Map(), bindingSummaries: new Map(), occupiedHandlesByNodeId: new Map(), zoomTier: 'full',
}))

export function syncCanvasRenderState(value: CanvasRenderState) {
  useCanvasRenderStore.setState(value)
}

export const canvasZoomTier = (zoom: number): CanvasZoomTier => zoom < 0.35 ? 'minimal' : zoom < 0.65 ? 'compact' : 'full'
