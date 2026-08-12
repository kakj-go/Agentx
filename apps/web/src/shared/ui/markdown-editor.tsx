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
import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef } from 'react'
import { useTranslation } from 'react-i18next'

type Props = {
  onChange: (value: string) => void
  readOnly?: boolean
  value: string
}
export type MarkdownEditorHandle = { insertText: (value: string) => void }

export const MarkdownEditor = forwardRef<MarkdownEditorHandle, Props>(function MarkdownEditor({ onChange, readOnly = false, value }, ref) {
  const { i18n, t } = useTranslation()
  const editor = useRef<MDXEditorMethods>(null)
  const externalValue = useRef(value)
  const onChangeRef = useRef(onChange)
  const initialValue = useRef(value)
  onChangeRef.current = onChange
  externalValue.current = value
  useImperativeHandle(ref, () => ({ insertText: (text) => editor.current?.insertMarkdown(text) }), [])

  const translate = useCallback<Translation>((key, defaultValue, interpolations) => (
    t(`skills.markdownEditor.${key}`, {
      defaultValue: i18n.resolvedLanguage?.startsWith('en')
        ? defaultValue
        : t('common.unknownValue', { value: key }),
      ...interpolations,
    })
  ), [i18n.resolvedLanguage, t])
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
      </>,
    })] : []),
  ], [readOnly])

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
