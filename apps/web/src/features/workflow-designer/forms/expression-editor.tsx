import { Eye, LoaderCircle, ShieldCheck } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '../../../shared/ui/button'
import { previewExpression } from '../api/studio-api'
import { CodeEditor, type EditorCompletion } from './code-editor'

const COMPLETIONS: EditorCompletion[] = [
  { label: '$json', insertText: '$json', detail: 'Current item JSON' },
  { label: '$input', insertText: '$input', detail: 'Current node input items' },
  { label: '$itemIndex', insertText: '$itemIndex', detail: 'Current item index' },
  { label: '$runIndex', insertText: '$runIndex', detail: 'Current node run index' },
  { label: 'node()', insertText: "node('Node').all('main', 0)", detail: 'Linked node output' },
]

export function ExpressionEditor({ workflowId, value, height = '110px', onChange }: { workflowId?: string; value: string; height?: string; onChange: (value: string) => void }) {
  const { t } = useTranslation()
  const [sample, setSample] = useState('{}')
  const [result, setResult] = useState<unknown>()
  const [redacted, setRedacted] = useState(false)
  const [error, setError] = useState<string>()
  const [loading, setLoading] = useState(false)
  const preview = async () => {
    if (!workflowId) return
    setLoading(true)
    setError(undefined)
    try {
      const json = JSON.parse(sample) as unknown
      const response = await previewExpression(workflowId, value, json)
      setResult(response.value)
      setRedacted(response.redacted)
    } catch (cause) {
      setResult(undefined)
      setRedacted(false)
      setError((cause as Error).message)
    } finally {
      setLoading(false)
    }
  }
  return <div><CodeEditor completionItems={COMPLETIONS} height={height} language="agentx-expression" onChange={onChange} value={value} /><p className="mt-1 text-[10px] text-muted-foreground">$json, $input, $itemIndex, $runIndex, node(...)</p>{workflowId && <details className="mt-2 border-t border-border pt-2"><summary className="cursor-pointer text-[10px] font-medium text-muted-foreground">{t('studio.expressionPreview.input')}</summary><div className="mt-2"><CodeEditor height="90px" language="json" onChange={setSample} value={sample} /><div className="mt-2 flex items-center gap-2"><Button disabled={loading || !value.trim()} onClick={() => void preview()} size="sm" variant="secondary">{loading ? <LoaderCircle className="size-3.5 animate-spin" /> : <Eye className="size-3.5" />}{t('studio.expressionPreview.run')}</Button>{redacted && <span className="flex items-center gap-1 text-[10px] text-success"><ShieldCheck className="size-3.5" />{t('studio.expressionPreview.redacted')}</span>}</div>{error && <p className="mt-2 text-[10px] text-danger">{error}</p>}{result !== undefined && <pre className="mt-2 max-h-28 overflow-auto bg-canvas p-2 font-mono text-[10px] leading-4 text-muted-foreground">{JSON.stringify(result, null, 2)}</pre>}</div></details>}</div>
}
