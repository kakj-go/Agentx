import { localizedNodeLabel } from './manifest-localization'
import type { NodeManifest } from './types'

/**
 * Node display names are persisted as language-neutral defaults (English base
 * manifest names, `End N` exits, backend-generated span names). These helpers map
 * them to the viewer's language at render time; user-renamed values pass through.
 */

const EXIT_DEFAULT_PATTERN = /^End(?: (\d+))?$/
const ATTEMPT_PATTERN = /^Attempt (\d+)$/
const ITERATION_PATTERN = /^Iteration (\d+)$/
const APPROVAL_PATTERN = /^Approval · (.+)$/
const WAIT_PATTERN = /^Wait · (.+)$/

export type NodeDisplayTexts = {
  language: string
  exitDefaultName: string
  spanKindLabel: (kind: string) => string
  waitApprovalLabel: (title: string) => string
  waitLabel: (title: string) => string
}

/** Reverse index from every locale's default name to its manifest, for name-only lookups (e.g. span names). */
export function buildDefaultNameIndex(manifests: Iterable<NodeManifest>) {
  const index = new Map<string, NodeManifest>()
  for (const manifest of manifests) {
    const names = [manifest.displayName, ...Object.values(manifest.localizations ?? {}).map((value) => value.displayName)]
    for (const name of names) if (name && !index.has(name)) index.set(name, manifest)
  }
  return index
}

export function resolveNodeDisplayName(
  nodeName: string | null | undefined,
  nodeType: string | null | undefined,
  manifestsByType: Map<string, NodeManifest>,
  defaultsByName: Map<string, NodeManifest>,
  texts: NodeDisplayTexts,
): string {
  const saved = (nodeName ?? '').trim()
  if (nodeType) {
    const manifest = manifestsByType.get(nodeType)
    if (manifest) return localizedNodeLabel(manifest, saved, texts.language)
  }
  const byName = defaultsByName.get(saved)
  if (byName) return localizedNodeLabel(byName, saved, texts.language)
  return localizedExitLabel(saved, texts.exitDefaultName) || saved || nodeType || ''
}

/** Span names carry node names plus backend-generated labels (`Attempt 1`, `Agent run`, …). */
export function resolveSpanDisplayName(
  spanName: string,
  manifestsByType: Map<string, NodeManifest>,
  defaultsByName: Map<string, NodeManifest>,
  texts: NodeDisplayTexts,
): string {
  const nodeName = resolveNodeDisplayName(spanName, undefined, manifestsByType, defaultsByName, texts)
  if (nodeName !== spanName) return nodeName
  const attempt = ATTEMPT_PATTERN.exec(spanName)
  if (attempt) return withSuffix(texts.spanKindLabel('attempt'), attempt[1])
  const iteration = ITERATION_PATTERN.exec(spanName)
  if (iteration) return withSuffix(texts.spanKindLabel('agent_iteration'), iteration[1])
  const approval = APPROVAL_PATTERN.exec(spanName)
  if (approval) return texts.waitApprovalLabel(approval[1])
  const wait = WAIT_PATTERN.exec(spanName)
  if (wait) return texts.waitLabel(wait[1])
  if (spanName === 'Agent run') return texts.spanKindLabel('agent_run')
  if (spanName === 'Runtime call') return texts.spanKindLabel('runtime_call')
  if (spanName === 'OpenSandbox execution') return texts.spanKindLabel('sandbox')
  return spanName
}

export function localizedExitLabel(savedLabel: string, defaultName: string) {
  const match = EXIT_DEFAULT_PATTERN.exec(savedLabel.trim())
  return match ? withSuffix(defaultName, match[1]) : savedLabel
}

function withSuffix(label: string, suffix?: string) {
  return suffix ? `${label} ${suffix}` : label
}
