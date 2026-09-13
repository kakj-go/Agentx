import { Check, Copy } from 'lucide-react'
import { useEffect, useState } from 'react'

import { Button } from '../../shared/ui/button'
import { useApplicationIntegrationDocsText } from './use-application-integration-docs'

export function CodeSnippet({ code, language }: { code: string; language: string }) {
  const text = useApplicationIntegrationDocsText()
  const [copied, setCopied] = useState(false)

  useEffect(() => setCopied(false), [code])

  const copy = async () => {
    await navigator.clipboard.writeText(code)
    setCopied(true)
  }

  return <div className="overflow-hidden rounded-lg border border-border bg-canvas">
    <div className="flex h-10 items-center justify-between border-b border-border bg-muted/40 px-3">
      <span className="font-mono text-[10px] uppercase text-muted-foreground">{language}</span>
      <Button aria-label={copied ? text.copied : text.copyCode} onClick={() => void copy()} size="sm" variant="ghost">
        {copied ? <Check className="size-3.5 text-success" /> : <Copy className="size-3.5" />}{copied ? text.copied : text.copyCode}
      </Button>
    </div>
    <pre className="max-h-96 overflow-auto p-4 text-[11px] leading-5"><code>{code}</code></pre>
  </div>
}
