import {
  BlockTypeSelect,
  BoldItalicUnderlineToggles,
  CreateLink,
  InsertTable,
  ListsToggle,
  MDXEditor,
  Separator,
  UndoRedo,
  codeBlockPlugin,
  codeMirrorPlugin,
  headingsPlugin,
  imagePlugin,
  linkDialogPlugin,
  linkPlugin,
  listsPlugin,
  markdownShortcutPlugin,
  quotePlugin,
  tablePlugin,
  toolbarPlugin,
  type MDXEditorMethods,
  type Translation,
} from '@mdxeditor/editor'
import '@mdxeditor/editor/style.css'
import { Link2 } from 'lucide-react'
import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef } from 'react'
import { useTranslation } from 'react-i18next'

type ToolbarAction = {
  disabled?: boolean
  label: string
  onClick: () => void
}

type Props = {
  onChange: (value: string) => void
  readOnly?: boolean
  toolbarAction?: ToolbarAction
  value: string
}
export type MarkdownEditorHandle = { insertText: (value: string) => void }

export const MarkdownEditor = forwardRef<MarkdownEditorHandle, Props>(function MarkdownEditor({ onChange, readOnly = false, toolbarAction, value }, ref) {
  const { i18n, t } = useTranslation()
  const editor = useRef<MDXEditorMethods>(null)
  const externalValue = useRef(value)
  const onChangeRef = useRef(onChange)
  const toolbarActionRef = useRef(toolbarAction)
  const initialValue = useRef(value)
  onChangeRef.current = onChange
  toolbarActionRef.current = toolbarAction
  externalValue.current = value
  useImperativeHandle(ref, () => ({
    insertText: (text) => editor.current?.focus(() => editor.current?.insertMarkdown(text), { preventScroll: true }),
  }), [])

  const translate = useCallback<Translation>((key, defaultValue, interpolations) => (
    t(`skills.markdownEditor.${key}`, {
      defaultValue: i18n.resolvedLanguage?.startsWith('en')
        ? defaultValue
        : t('common.unknownValue', { value: key }),
      ...interpolations,
    })
  ), [i18n.resolvedLanguage, t])
  const toolbarActionDisabled = toolbarAction?.disabled
  const toolbarActionLabel = toolbarAction?.label
  const plugins = useMemo(() => [
    headingsPlugin(),
    quotePlugin(),
    listsPlugin(),
    linkPlugin(),
    linkDialogPlugin(),
    imagePlugin(),
    tablePlugin(),
    codeBlockPlugin({ defaultCodeBlockLanguage: 'text' }),
    codeMirrorPlugin({ codeBlockLanguages: { text: 'Plain text', json: 'JSON', javascript: 'JavaScript', typescript: 'TypeScript', python: 'Python', rust: 'Rust', shell: 'Shell' } }),
    markdownShortcutPlugin(),
    ...(!readOnly ? [toolbarPlugin({
      toolbarClassName: 'agentx-markdown-toolbar',
      toolbarContents: () => <>
        <UndoRedo />
        <Separator />
        <BlockTypeSelect />
        <Separator />
        <BoldItalicUnderlineToggles />
        <Separator />
        <ListsToggle />
        <Separator />
        <CreateLink />
        <InsertTable />
        {toolbarActionLabel && <><Separator /><ToolbarActionButton disabled={toolbarActionDisabled} label={toolbarActionLabel} onClick={() => toolbarActionRef.current?.onClick()} /></>}
      </>,
    })] : []),
  ], [readOnly, toolbarActionDisabled, toolbarActionLabel])

  useEffect(() => {
    const instance = editor.current
    if (!instance || instance.getMarkdown() === value) return
    instance.setMarkdown(value)
  }, [value])

  return <MDXEditor
    className="agentx-markdown-editor mdxeditor-full-height"
    contentEditableClassName="agentx-markdown-content"
    markdown={initialValue.current}
    onChange={(markdown, initialNormalize) => { if (!initialNormalize && markdown !== externalValue.current) onChangeRef.current(markdown) }}
    plugins={plugins}
    readOnly={readOnly}
    ref={editor}
    translation={translate}
  />
})

function ToolbarActionButton({ disabled, label, onClick }: { disabled?: boolean; label: string; onClick: () => void }) {
  return <button aria-label={label} className="agentx-markdown-toolbar-action" disabled={disabled} onClick={onClick} title={label} type="button"><Link2 className="size-3.5" /><span>{label}</span></button>
}
