import type { CanvasNodeFamily, CanvasNodeRole, NodeManifest, ResourceType } from '../model/types'

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

export function canvasNodeRole(manifest?: NodeManifest): CanvasNodeRole {
  return manifest?.uiSchema.canvas?.role ?? 'default'
}

export function nodeShape(manifest?: NodeManifest) {
  return canvasNodeRole(manifest)
}

export function canvasNodeFamily(role: CanvasNodeRole): CanvasNodeFamily {
  return role === 'agent' ? 'agent' : 'compact'
}

export type CanvasNodePorts = {
  inputs?: number
  outputs?: number
  bindings?: number
  richHeight?: number
  kind?: 'action' | 'binding' | 'group'
}

export type CanvasPlacementRect = { x: number; y: number; width: number; height: number }

const PLACEMENT_GAP = 40

export function findOpenCanvasPosition(anchor: { x: number; y: number }, size: { width: number; height: number }, occupied: CanvasPlacementRect[], bounds?: CanvasPlacementRect) {
  const origin = { x: anchor.x - size.width / 2, y: anchor.y - size.height / 2 }
  const stepX = Math.max(160, size.width + 72)
  const stepY = Math.max(152, size.height + 64)
  const candidates = [origin]
  for (let radius = 1; radius <= 8; radius += 1) {
    candidates.push(
      { x: origin.x + radius * stepX, y: origin.y },
      { x: origin.x, y: origin.y + radius * stepY },
      { x: origin.x - radius * stepX, y: origin.y },
      { x: origin.x, y: origin.y - radius * stepY },
    )
    for (let offset = 1; offset < radius; offset += 1) {
      candidates.push(
        { x: origin.x + radius * stepX, y: origin.y + offset * stepY },
        { x: origin.x + offset * stepX, y: origin.y + radius * stepY },
        { x: origin.x - radius * stepX, y: origin.y - offset * stepY },
        { x: origin.x - offset * stepX, y: origin.y - radius * stepY },
      )
    }
  }
  const available = candidates.find((candidate) => (!bounds || rectangleInsideBounds({ ...candidate, ...size }, bounds)) && occupied.every((rect) => !rectanglesOverlap(
    { ...candidate, width: size.width, height: size.height },
    rect,
  )))
  if (available) return available
  if (!bounds) return candidates.at(-1)!
  const minX = bounds.x + PLACEMENT_GAP
  const minY = bounds.y + PLACEMENT_GAP
  const maxX = bounds.x + bounds.width - size.width - PLACEMENT_GAP
  const maxY = bounds.y + bounds.height - size.height - PLACEMENT_GAP
  const visibleCandidates = []
  for (let y = minY; y <= maxY; y += 32) {
    for (let x = minX; x <= maxX; x += 32) visibleCandidates.push({ x, y })
  }
  visibleCandidates.sort((left, right) => squaredDistance(left, origin) - squaredDistance(right, origin) || left.y - right.y || left.x - right.x)
  const visible = visibleCandidates.find((candidate) => occupied.every((rect) => !rectanglesOverlap({ ...candidate, ...size }, rect)))
  if (visible) return visible
  return {
    x: Math.min(Math.max(origin.x, minX), maxX),
    y: Math.min(Math.max(origin.y, minY), maxY),
  }
}

function squaredDistance(left: { x: number; y: number }, right: { x: number; y: number }) {
  return (left.x - right.x) ** 2 + (left.y - right.y) ** 2
}

function rectanglesOverlap(left: CanvasPlacementRect, right: CanvasPlacementRect) {
  return left.x < right.x + right.width + PLACEMENT_GAP
    && left.x + left.width + PLACEMENT_GAP > right.x
    && left.y < right.y + right.height + PLACEMENT_GAP
    && left.y + left.height + PLACEMENT_GAP > right.y
}

function rectangleInsideBounds(rect: CanvasPlacementRect, bounds: CanvasPlacementRect) {
  return rect.x >= bounds.x + PLACEMENT_GAP
    && rect.y >= bounds.y + PLACEMENT_GAP
    && rect.x + rect.width <= bounds.x + bounds.width - PLACEMENT_GAP
    && rect.y + rect.height <= bounds.y + bounds.height - PLACEMENT_GAP
}

export function canvasNodeMetrics(role: CanvasNodeRole, ports: CanvasNodePorts = {}, collapsed = false) {
  if (ports.kind === 'binding') return { width: 96, height: 96, labelBelow: true }
  if (ports.kind === 'group') return collapsed
    ? { width: 240, height: 64, labelBelow: false }
    : { width: 240, height: 160, labelBelow: false }
  if (role === 'agent') return { width: 224, height: 96, labelBelow: true }
  return { width: 96, height: 96, labelBelow: true }
}

export function categoryLabel(category: NodeCategory, translate: (key: string) => string) {
  return translate(`studio.palette.categories.${category}`)
}

export const resourceIcon = (type: ResourceType) => ({ model: 'brain-circuit', mcp_tool: 'wrench', memory: 'memory-stick', rag: 'database', skill: 'sparkles' } as Partial<Record<ResourceType, string>>)[type] ?? 'box'
