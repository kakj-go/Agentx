import { useQuery } from '@tanstack/react-query'
import { useCallback, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { buildDefaultNameIndex, resolveNodeDisplayName, resolveSpanDisplayName, type NodeDisplayTexts } from '../model/node-display'
import type { NodeManifest } from '../model/types'
import { loadNodeCatalog } from './studio-api'

/** Shares the workflow-canvas catalog cache (`node-definitions`) so trace/execution views reuse it. */
export function useNodeCatalog() {
  return useQuery({ queryKey: ['node-definitions'], queryFn: loadNodeCatalog, staleTime: 60_000 })
}

export type NodeNameResolver = {
  /** Maps a stored node name (+ optional node type) to the current display language. */
  resolveNodeName: (nodeName: string | null | undefined, nodeType?: string | null) => string
  /** Maps a trace span name to the current display language (node names + backend span labels). */
  resolveSpanName: (spanName: string) => string
}

export function useNodeNames(): NodeNameResolver {
  const { t, i18n } = useTranslation()
  const catalog = useNodeCatalog()
  const manifestsByType = useMemo(() => new Map<string, NodeManifest>((catalog.data ?? []).map((item) => [item.manifest.nodeType, item.manifest])), [catalog.data])
  const defaultsByName = useMemo(() => buildDefaultNameIndex(manifestsByType.values()), [manifestsByType])
  const texts = useMemo<NodeDisplayTexts>(() => ({
    language: i18n.language,
    exitDefaultName: t('studio.exit.defaultName'),
    spanKindLabel: (kind) => t(`trace.kinds.${kind}`),
    waitApprovalLabel: (title) => t('trace.waitApproval', { title }),
    waitLabel: (title) => t('trace.waitGeneric', { title }),
  }), [i18n.language, t])
  const resolveNodeName = useCallback((nodeName: string | null | undefined, nodeType?: string | null) => resolveNodeDisplayName(nodeName, nodeType, manifestsByType, defaultsByName, texts), [defaultsByName, manifestsByType, texts])
  const resolveSpanName = useCallback((spanName: string) => resolveSpanDisplayName(spanName, manifestsByType, defaultsByName, texts), [defaultsByName, manifestsByType, texts])
  return { resolveNodeName, resolveSpanName }
}
