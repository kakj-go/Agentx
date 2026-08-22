import { Download } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { TraceContent, TraceSpanDetail } from '../../shared/api/types'
import { Button } from '../../shared/ui/button'
import { JsonBlock, TraceSemanticValue } from './trace-semantic-value'

export type ContentKind = TraceContent['kind']

export function TraceContents({ contents, advanced = false, onDownloadArtifact }: { contents: TraceContent[]; advanced?: boolean; onDownloadArtifact?: (artifactId: string) => void }) {
  const { t } = useTranslation()
  if (!contents.length) return <p className="text-xs text-muted-foreground">{t('trace.noDiagnosticContent')}</p>
  const groups = groupContents(contents)
  return <div className="space-y-4">{groups.map(([kind, entries]) => <section className="space-y-2" key={kind}>
    <h4 className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{t(`trace.contentKinds.${kind}`)}</h4>
    {entries.map((entry) => <div className="space-y-2 rounded-lg border border-border p-3" key={entry.eventId}>
      {advanced ? <JsonBlock value={entry.preview} /> : <TraceSemanticValue value={entry.preview} />}
      {entry.contentRef && onDownloadArtifact && <Button onClick={() => onDownloadArtifact(entry.contentRef!)} size="sm" variant="secondary"><Download className="size-3.5" />{t('trace.downloadArtifact')}</Button>}
    </div>)}
  </section>)}</div>
}

export function contentPreview(detail: TraceSpanDetail | undefined, kind: ContentKind) {
  return detail?.contents?.find((content) => content.kind === kind)?.preview
}

export function contentsForKinds(contents: TraceContent[], kinds: ContentKind[]) {
  const selected = new Set(kinds)
  return contents.filter((content) => selected.has(content.kind))
}

export function groupContents(contents: TraceContent[]) {
  const groups = new Map<ContentKind, TraceContent[]>()
  for (const content of contents) {
    const group = groups.get(content.kind) ?? []
    group.push(content)
    groups.set(content.kind, group)
  }
  return [...groups.entries()]
}
