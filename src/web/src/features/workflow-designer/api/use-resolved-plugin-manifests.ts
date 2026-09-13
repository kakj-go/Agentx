import { useQueries } from '@tanstack/react-query'
import { useMemo } from 'react'

import type { NodeManifest, StudioEdge, StudioNode } from '../model/types'
import { resolveNodeDefinition } from './studio-api'

export type PluginDefinitionState = {
  status: 'complete' | 'incomplete' | 'invalid'
  issues: Array<{ path: string; code: string; message: string }>
}

export function useResolvedPluginManifests(
  nodes: StudioNode[],
  edges: StudioEdge[],
  manifests: Map<string, NodeManifest>,
) {
  const candidates = nodes.flatMap((node) => {
    if (node.data.editorKind !== 'action') return []
    const manifest = manifests.get(`${node.data.nodeType}@${node.data.typeVersion}`)
    if (!manifest?.plugin || manifest.capability !== 'plugin_nodejs' || manifest.plugin.packageId.startsWith('agentx/')) return []
    return [{ node, manifest, upstreamContracts: upstreamContracts(nodes, edges, manifests, node.id) }]
  })
  const queries = useQueries({
    queries: candidates.map(({ node, manifest, upstreamContracts: contracts }) => ({
      queryKey: ['resolved-plugin-definition', node.id, manifest.plugin?.bundleDigest, node.data.parameters, contracts],
      queryFn: ({ signal }: { signal: AbortSignal }) => resolveNodeDefinition(
        manifest.nodeType,
        manifest.version,
        node.data.parameters,
        contracts,
        signal,
      ),
      staleTime: Infinity,
      retry: false,
    })),
  })
  return useMemo(() => {
    const resolved = new Map<string, NodeManifest>()
    const states = new Map<string, PluginDefinitionState>()
    candidates.forEach(({ node, manifest }, index) => {
      const value = queries[index]?.data
      if (!value) return
      resolved.set(node.id, {
        ...manifest,
        inputPorts: value.inputPorts ?? manifest.inputPorts,
        outputPorts: value.outputPorts ?? manifest.outputPorts,
        outputSchema: value.outputSchema ?? manifest.outputSchema,
        outputPortSchemas: value.outputPortSchemas ?? manifest.outputPortSchemas,
      })
      states.set(node.id, { status: value.status, issues: value.issues ?? [] })
    })
    return { resolved, states }
  }, [candidates, queries])
}

function upstreamContracts(
  nodes: StudioNode[],
  edges: StudioEdge[],
  manifests: Map<string, NodeManifest>,
  targetNodeId: string,
) {
  const nodeById = new Map(nodes.map((node) => [node.id, node]))
  return Object.fromEntries(edges.filter((edge) => edge.target === targetNodeId).map((edge) => {
    const source = nodeById.get(edge.source)
    const manifest = source?.data.editorKind === 'action'
      ? manifests.get(`${source.data.nodeType}@${source.data.typeVersion}`)
      : undefined
    const sourcePort = edge.sourceHandle ?? 'main'
    return [edge.targetHandle ?? 'main', {
      sourceNodeId: edge.source,
      sourcePort,
      schema: manifest?.outputPortSchemas?.[sourcePort] ?? manifest?.outputSchema ?? {},
    }]
  }))
}
