import type { StudioEdge, StudioNode } from '../model/types'

export type StudioClipboardFragment = { sourceWorkflowId: string; nodes: StudioNode[]; edges: StudioEdge[] }

const STORAGE_KEY = 'agentx:studio:clipboard'

export function writeStudioClipboard(sourceWorkflowId: string, nodes: StudioNode[], edges: StudioEdge[]) {
  try {
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify({ sourceWorkflowId, nodes, edges }))
  } catch {
    // Clipboard persistence is best effort; large fragments may exceed browser storage.
  }
}

export function readStudioClipboard(): StudioClipboardFragment | undefined {
  try {
    const value = JSON.parse(sessionStorage.getItem(STORAGE_KEY) ?? 'null') as Partial<StudioClipboardFragment> | null
    if (!value?.sourceWorkflowId || !Array.isArray(value.nodes) || !Array.isArray(value.edges)) return undefined
    return { sourceWorkflowId: value.sourceWorkflowId, nodes: value.nodes, edges: value.edges }
  } catch {
    return undefined
  }
}

export function cloneStudioFragment(fragment: Pick<StudioClipboardFragment, 'nodes' | 'edges'>): [StudioNode[], StudioEdge[]] {
  const ids = new Map<string, string>()
  const nodes = fragment.nodes.map((source) => {
    const data = structuredClone(source.data)
    const id = crypto.randomUUID()
    ids.set(source.id, id)
    return { ...structuredClone(source), id, position: { x: source.position.x + 36, y: source.position.y + 36 }, selected: true, data }
  })
  // Membership survives copy/paste only when the container travels with its children.
  const pastedIds = new Set<string>(nodes.map((node) => node.id))
  for (const node of nodes) {
    const data = node.data
    if (data.editorKind !== 'action') continue
    const parentId: string | undefined = data.parentId
    const remapped = parentId ? ids.get(parentId) : undefined
    node.data = remapped && pastedIds.has(remapped)
      ? { ...data, parentId: remapped }
      : { ...data, parentId: undefined }
  }
  const edges = fragment.edges.flatMap((source) => {
    const sourceId = ids.get(source.source)
    const targetId = ids.get(source.target)
    return sourceId && targetId ? [{ ...structuredClone(source), id: crypto.randomUUID(), source: sourceId, target: targetId, selected: false }] : []
  })
  return [nodes, edges]
}
