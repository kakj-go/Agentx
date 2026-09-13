import { readFile } from 'node:fs/promises'

const root = new URL('../', import.meta.url)
const manifest = JSON.parse(await readFile(new URL('manifest.json', root)))
if (manifest.protocolVersion !== 1 || manifest.sdkApiVersion !== 1) throw new Error('Plugin protocolVersion and sdkApiVersion must be 1')
if (!/^[a-z0-9][a-z0-9._-]*\/[a-z0-9][a-z0-9._-]*$/.test(manifest.packageId) || manifest.packageId.startsWith('agentx/')) throw new Error('packageId must be a non-reserved lowercase publisher/name')
if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(manifest.packageVersion)) throw new Error('packageVersion must be semantic version')
if (!Array.isArray(manifest.nodes) || !manifest.nodes.length || new Set(manifest.nodes).size !== manifest.nodes.length) throw new Error('nodes must contain unique manifest paths')
const nodeTypes = new Set()
for (const path of manifest.nodes) {
  if (path.includes('..') || path.startsWith('/')) throw new Error(`Unsafe node path: ${path}`)
  const node = JSON.parse(await readFile(new URL(path, root)))
  if (node.protocolVersion !== '3.0' || node.capability !== 'plugin_nodejs' || !node.nodeType || node.version < 1) throw new Error(`Invalid Node Manifest: ${path}`)
  if (nodeTypes.has(node.nodeType)) throw new Error(`Duplicate nodeType: ${node.nodeType}`)
  nodeTypes.add(node.nodeType)
}
for (const renderer of manifest.traceRenderers ?? []) {
  if (!renderer.contentType || renderer.contentVersion < 1 || !renderer.exportName || renderer.schema?.type !== 'object') throw new Error('Invalid trace renderer declaration')
}
