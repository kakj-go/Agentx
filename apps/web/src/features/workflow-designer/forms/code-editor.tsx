import { lazy, Suspense, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'

const Monaco = lazy(() => import('@monaco-editor/react'))
let expressionLanguageRegistered = false

export type EditorCompletion = { label: string; insertText: string; detail?: string }

export function CodeEditor({ value, language = 'plaintext', height = '150px', completionItems = [], readOnly = false, onChange }: { value: string; language?: string; height?: string; completionItems?: EditorCompletion[]; readOnly?: boolean; onChange: (value: string) => void }) {
  const { t } = useTranslation()
  const theme = typeof document !== 'undefined' && document.documentElement.classList.contains('dark') ? 'vs-dark' : 'light'
  const disposable = useRef<{ dispose: () => void }>()
  useEffect(() => () => disposable.current?.dispose(), [])
  return <Suspense fallback={<div className="grid place-items-center border border-border bg-muted/20 text-[11px] text-muted-foreground" style={{ height }}>{t('studio.editorLoading')}</div>}><Monaco beforeMount={(monaco) => {
    if (language === 'agentx-expression' && !expressionLanguageRegistered) {
      monaco.languages.register({ id: 'agentx-expression' })
      monaco.languages.setMonarchTokensProvider('agentx-expression', { tokenizer: { root: [[/\$[a-zA-Z][\w]*/, 'variable'], [/'[^']*'|"[^"]*"/, 'string'], [/\b\d+(?:\.\d+)?\b/, 'number'], [/[a-zA-Z_]\w*(?=\()/, 'function']] } })
      expressionLanguageRegistered = true
    }
  }} height={height} language={language} onChange={(next) => onChange(next ?? '')} onMount={(_, monaco) => {
    disposable.current?.dispose()
    if (!completionItems.length) return
    const provider: Parameters<typeof monaco.languages.registerCompletionItemProvider>[1] = {
      provideCompletionItems(model: { getWordUntilPosition: (position: EditorPosition) => { startColumn: number; endColumn: number } }, position: EditorPosition) {
        const word = model.getWordUntilPosition(position)
        const range = { startLineNumber: position.lineNumber, endLineNumber: position.lineNumber, startColumn: word.startColumn, endColumn: word.endColumn }
        return { suggestions: completionItems.map((item) => ({ ...item, kind: monaco.languages.CompletionItemKind.Variable, range })) }
      },
    }
    disposable.current = monaco.languages.registerCompletionItemProvider(language, provider)
  }} options={{ minimap: { enabled: false }, fontSize: 12, lineNumbers: language === 'json' || language === 'plaintext' || language === 'agentx-expression' ? 'off' : 'on', readOnly, scrollBeyondLastLine: false, wordWrap: 'on' }} theme={theme} value={value} /></Suspense>
}

type EditorPosition = { lineNumber: number; column: number }
