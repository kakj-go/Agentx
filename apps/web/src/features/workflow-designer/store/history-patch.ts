import type { StudioDocument, StudioEdge, StudioNode } from '../model/types'

type DocumentState = Pick<StudioDocument, 'start' | 'nodes' | 'edges' | 'end' | 'boundaryLayouts' | 'annotations' | 'groups' | 'settings'>
type Annotation = StudioDocument['annotations'][number]
type Group = StudioDocument['groups'][number]

type EntityPatch<T> = {
  id: string
  before?: T
  after?: T
  beforeIndex?: number
  afterIndex?: number
}

export type HistoryPatch = {
  nodes: EntityPatch<StudioNode>[]
  edges: EntityPatch<StudioEdge>[]
  annotations: EntityPatch<Annotation>[]
  groups: EntityPatch<Group>[]
  settings?: { before: StudioDocument['settings']; after: StudioDocument['settings'] }
  start?: { before: StudioDocument['start']; after: StudioDocument['start'] }
  end?: { before: StudioDocument['end']; after: StudioDocument['end'] }
  boundaryLayouts?: { before: StudioDocument['boundaryLayouts']; after: StudioDocument['boundaryLayouts'] }
  graphChanged: boolean
}

export function createHistoryPatch(before: DocumentState, after: DocumentState): HistoryPatch | undefined {
  const nodes = entityPatches(before.nodes, after.nodes, nodeValue)
  const edges = entityPatches(before.edges, after.edges, edgeValue)
  const annotations = entityPatches(before.annotations, after.annotations)
  const groups = entityPatches(before.groups, after.groups)
  const settings = equal(before.settings, after.settings) ? undefined : { before: before.settings, after: after.settings }
  const start = equal(before.start, after.start) ? undefined : { before: before.start, after: after.start }
  const end = equal(before.end, after.end) ? undefined : { before: before.end, after: after.end }
  const boundaryLayouts = equal(before.boundaryLayouts, after.boundaryLayouts) ? undefined : { before: before.boundaryLayouts, after: after.boundaryLayouts }
  if (!nodes.length && !edges.length && !annotations.length && !groups.length && !settings && !start && !end && !boundaryLayouts) return undefined
  return {
    nodes,
    edges,
    annotations,
    groups,
    settings,
    start,
    end,
    boundaryLayouts,
    graphChanged: nodes.some((patch) => !patch.before || !patch.after || nodeStructure(patch.before) !== nodeStructure(patch.after)) || edges.length > 0,
  }
}

export function applyHistoryPatch(state: DocumentState, patch: HistoryPatch, direction: 'undo' | 'redo'): DocumentState {
  return {
    start: patch.start ? patch.start[direction === 'undo' ? 'before' : 'after'] : state.start,
    nodes: applyEntities(state.nodes, patch.nodes, direction, mergeNodeInteraction),
    edges: applyEntities(state.edges, patch.edges, direction, mergeEdgeInteraction),
    end: patch.end ? patch.end[direction === 'undo' ? 'before' : 'after'] : state.end,
    boundaryLayouts: patch.boundaryLayouts ? patch.boundaryLayouts[direction === 'undo' ? 'before' : 'after'] : state.boundaryLayouts,
    annotations: applyEntities(state.annotations, patch.annotations, direction),
    groups: applyEntities(state.groups, patch.groups, direction),
    settings: patch.settings ? patch.settings[direction === 'undo' ? 'before' : 'after'] : state.settings,
  }
}

function entityPatches<T extends { id: string }>(before: T[], after: T[], project: (value: T) => unknown = identity): EntityPatch<T>[] {
  const beforeById = new Map(before.map((value, index) => [value.id, { value, index }]))
  const afterById = new Map(after.map((value, index) => [value.id, { value, index }]))
  const ids = new Set([...beforeById.keys(), ...afterById.keys()])
  const result: EntityPatch<T>[] = []
  for (const id of ids) {
    const previous = beforeById.get(id)
    const next = afterById.get(id)
    if (previous && next && equal(project(previous.value), project(next.value))) continue
    result.push({ id, before: previous?.value, after: next?.value, beforeIndex: previous?.index, afterIndex: next?.index })
  }
  return result
}

function applyEntities<T extends { id: string }>(current: T[], patches: EntityPatch<T>[], direction: 'undo' | 'redo', merge?: (value: T, current?: T) => T): T[] {
  if (!patches.length) return current
  const desired = direction === 'undo' ? 'before' : 'after'
  const desiredIndex = direction === 'undo' ? 'beforeIndex' : 'afterIndex'
  const changed = new Set(patches.map((patch) => patch.id))
  const currentById = new Map(current.map((value) => [value.id, value]))
  const result = current.filter((value) => !changed.has(value.id))
  const insertions = patches
    .flatMap((patch) => patch[desired] ? [{ value: merge ? merge(patch[desired]!, currentById.get(patch.id)) : patch[desired]!, index: patch[desiredIndex] ?? result.length }] : [])
    .sort((left, right) => left.index - right.index)
  for (const insertion of insertions) result.splice(Math.min(insertion.index, result.length), 0, insertion.value)
  return result
}

function nodeValue(node: StudioNode) {
  return { id: node.id, type: node.type, position: node.position, width: node.width, height: node.height, data: node.data }
}

function edgeValue(edge: StudioEdge) {
  return { id: edge.id, type: edge.type, source: edge.source, sourceHandle: edge.sourceHandle, target: edge.target, targetHandle: edge.targetHandle, data: edge.data }
}

function mergeNodeInteraction(value: StudioNode, current?: StudioNode): StudioNode {
  return { ...value, selected: current?.selected, dragging: current?.dragging, measured: current?.measured }
}

function mergeEdgeInteraction(value: StudioEdge, current?: StudioEdge): StudioEdge {
  return { ...value, selected: current?.selected }
}

function nodeStructure(node: StudioNode) {
  return node.data.editorKind === 'action'
    ? `${node.data.editorKind}:${node.data.nodeType}:${node.data.typeVersion}`
    : `${node.data.editorKind}:${'resourceType' in node.data ? `${node.data.resourceType}:${node.data.bindingRole}:${node.data.resourceName}:${node.data.label}` : 'label' in node.data ? node.data.label : ''}`
}

function equal(left: unknown, right: unknown) {
  return left === right || JSON.stringify(left) === JSON.stringify(right)
}

function identity<T>(value: T) { return value }
