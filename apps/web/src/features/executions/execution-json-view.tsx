import { Binary, Braces, Download } from 'lucide-react'

import { Button } from '../../shared/ui/button'

type JsonViewProps = {
  value: unknown
  emptyLabel: string
  onDownloadArtifact?: (artifactId: string) => void
}

function artifactId(value: unknown) {
  if (!value || typeof value !== 'object') return undefined
  const record = value as Record<string, unknown>
  const candidate = record.artifactId ?? record.artifactHandle ?? record.artifact_handle
  return typeof candidate === 'string' ? candidate : undefined
}

function binaryReferences(value: unknown, results: Array<{ path: string; id: string }>, path = '$') {
  const id = artifactId(value)
  if (id) results.push({ path, id })
  if (Array.isArray(value)) {
    value.forEach((item, index) => binaryReferences(item, results, `${path}[${index}]`))
  } else if (value && typeof value === 'object') {
    Object.entries(value as Record<string, unknown>).forEach(([key, item]) => binaryReferences(item, results, `${path}.${key}`))
  }
  return results
}

export function ExecutionJsonView({ value, emptyLabel, onDownloadArtifact }: JsonViewProps) {
  if (value == null) return <div className="grid min-h-64 place-items-center text-xs text-muted-foreground"><Braces className="mb-3 size-5" /><span>{emptyLabel}</span></div>
  const serialized = JSON.stringify(value, null, 2)
  const binaries = binaryReferences(value, [])
  return <div className="min-w-0">
    {binaries.length > 0 && <div className="border-b border-border bg-muted/35 px-4 py-3">
      <p className="mb-2 flex items-center gap-2 text-[11px] font-semibold"><Binary className="size-3.5 text-primary" />Binary / Artifact</p>
      <div className="flex flex-wrap gap-2">{binaries.map((binary) => <Button key={`${binary.path}:${binary.id}`} onClick={() => onDownloadArtifact?.(binary.id)} size="sm" variant="secondary"><Download className="size-3.5" /><span className="max-w-48 truncate">{binary.path}</span></Button>)}</div>
    </div>}
    <pre className="max-h-[54vh] min-h-64 overflow-auto whitespace-pre-wrap break-all bg-canvas px-4 py-4 font-mono text-[11px] leading-5 text-foreground" data-testid="execution-json">{serialized}</pre>
  </div>
}
