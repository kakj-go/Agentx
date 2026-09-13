import catalog from './studio-catalog.fixture.json'

import type { NodeManifest } from '../model/types'

const manifests = catalog as unknown as NodeManifest[]

export function studioManifest(nodeType: string): NodeManifest {
  const manifest = manifests.find((candidate) => candidate.nodeType === nodeType)
  if (!manifest) throw new Error(`Studio Manifest '${nodeType}' is missing from the generated fixture`)
  return manifest
}

export const studioManifestTypes = manifests.map((manifest) => manifest.nodeType)
